//! Microphone capture via cpal (WASAPI on Windows). Recording runs on a
//! dedicated thread that owns the cpal stream (which is !Send), accumulates
//! mono f32 samples into a shared buffer, and emits an amplitude level to the
//! Flow Bar ~30×/sec for the waveform. Resampling to 16 kHz and WAV encoding
//! happen after the user releases the key (see `pipeline`).

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use parking_lot::Mutex;
use tauri::{AppHandle, Emitter};

use crate::events;
use crate::window_mgmt;

/// Hard ceiling on buffered capture samples (~15 min at 48 kHz) so a stuck key
/// can't grow the buffer without bound. Once reached we keep emitting amplitude
/// (so the waveform stays live) but stop appending; the pipeline then encodes
/// what we have and surfaces an over-length error for the cloud transcriber.
const MAX_CAPTURE_SAMPLES: usize = 48_000 * 60 * 15;

/// Names of available capture devices, for the Settings picker. The empty-string
/// choice ("system default") is added by the UI, not here. Best-effort: returns
/// an empty list if the host can't enumerate.
pub fn input_devices() -> Vec<String> {
    let host = cpal::default_host();
    host.input_devices()
        .map(|devs| devs.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default()
}

/// Resolve the configured device name to a cpal device. An empty name (or a name
/// that no longer matches any device, e.g. the mic was unplugged) falls back to
/// the system default input device.
fn select_input_device(host: &cpal::Host, name: &str) -> Option<cpal::Device> {
    if !name.is_empty() {
        if let Ok(mut devices) = host.input_devices() {
            if let Some(d) = devices.find(|d| d.name().map(|n| n == name).unwrap_or(false)) {
                return Some(d);
            }
        }
    }
    host.default_input_device()
}

/// A command to the persistent capture thread.
enum CaptureCmd {
    /// Begin capturing from `device_name` (empty = system default). Any stream
    /// already open is torn down first.
    Start { device_name: String },
    /// Stop capturing and release the device.
    Stop,
}

/// Owns the single capture thread. Recording is driven by `start`/`stop`
/// commands rather than by spawning a fresh thread (and cpal stream) per press.
///
/// One thread processing commands *serially* is the whole point: a `Start`
/// always fully drops the previous stream before building the next, so two
/// streams never hold the same device at once. That overlap - a stale
/// slow-starting thread plus a new one, both racing to open the mic - was the
/// source of the rapid-tap failures where the next press intermittently died
/// with a device-busy error or hijacked the wrong session.
pub struct CaptureHandle {
    tx: Mutex<Option<Sender<CaptureCmd>>>,
}

impl Default for CaptureHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl CaptureHandle {
    pub fn new() -> Self {
        Self {
            tx: Mutex::new(None),
        }
    }

    /// Start recording. Spawns the capture thread on first use (binding the app
    /// handle and shared buffers), then reuses it for every later session.
    pub fn start(
        &self,
        app: AppHandle,
        is_recording: Arc<AtomicBool>,
        buffer: Arc<Mutex<Vec<f32>>>,
        sample_rate: Arc<AtomicU32>,
        amp: Arc<Mutex<f32>>,
        device_name: String,
    ) {
        let mut guard = self.tx.lock();
        let tx = guard.get_or_insert_with(|| {
            spawn_capture_thread(app, is_recording, buffer, sample_rate, amp)
        });
        let _ = tx.send(CaptureCmd::Start { device_name });
    }

    /// Stop the current recording and release the device. No-op if the thread
    /// was never started.
    pub fn stop(&self) {
        if let Some(tx) = self.tx.lock().as_ref() {
            let _ = tx.send(CaptureCmd::Stop);
        }
    }
}

/// The capture thread: owns the (non-`Send`) cpal stream for its whole life and
/// serves `Start`/`Stop` commands. While recording it wakes every 33 ms to push
/// amplitude to the Flow Bar and warn as the buffer nears its ceiling.
fn spawn_capture_thread(
    app: AppHandle,
    is_recording: Arc<AtomicBool>,
    buffer: Arc<Mutex<Vec<f32>>>,
    sample_rate: Arc<AtomicU32>,
    amp: Arc<Mutex<f32>>,
) -> Sender<CaptureCmd> {
    let (tx, rx): (Sender<CaptureCmd>, Receiver<CaptureCmd>) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let host = cpal::default_host();
        let ready_sent = Arc::new(AtomicBool::new(false));
        // Holding the stream keeps the mic open; `Some` == recording. Dropping it
        // (via `take`) is what stops capture and releases the device.
        let mut stream: Option<cpal::Stream> = None;
        let mut warned = false;
        let mut warn_at = usize::MAX;

        loop {
            // While recording, tick every 33 ms for the waveform; while idle,
            // block until the next command so we don't spin.
            let cmd = if stream.is_some() {
                match rx.recv_timeout(Duration::from_millis(33)) {
                    Ok(c) => Some(c),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            } else {
                match rx.recv() {
                    Ok(c) => Some(c),
                    Err(_) => break,
                }
            };

            match cmd {
                Some(CaptureCmd::Start { device_name }) => {
                    // Serialized teardown: fully drop any prior stream before
                    // opening a new one so the device is never held by two
                    // streams at once.
                    drop(stream.take());
                    buffer.lock().clear();
                    *amp.lock() = 0.0;
                    ready_sent.store(false, Ordering::SeqCst);
                    warned = false;

                    match build_stream(
                        &host,
                        &device_name,
                        &buffer,
                        &amp,
                        &sample_rate,
                        &app,
                        &ready_sent,
                    ) {
                        Ok(s) => {
                            if let Err(e) = s.play() {
                                is_recording.store(false, Ordering::SeqCst);
                                window_mgmt::fail(
                                    &app,
                                    &format!("Could not start microphone: {e}"),
                                );
                            } else {
                                let rate = sample_rate.load(Ordering::SeqCst) as usize;
                                warn_at =
                                    MAX_CAPTURE_SAMPLES.saturating_sub(rate.max(8_000) * 60);
                                stream = Some(s);
                            }
                        }
                        Err(e) => {
                            is_recording.store(false, Ordering::SeqCst);
                            window_mgmt::fail(&app, &e);
                        }
                    }
                }
                Some(CaptureCmd::Stop) => {
                    drop(stream.take()); // releases the device
                    *amp.lock() = 0.0;
                }
                None => {
                    let level = *amp.lock();
                    let _ = app.emit_to(events::FLOWBAR, events::AMPLITUDE, level);
                    if !warned && buffer.lock().len() >= warn_at {
                        warned = true;
                        let _ = app.emit_to(events::FLOWBAR, events::LIMIT, ());
                    }
                }
            }
        }
    });
    tx
}

/// Open a cpal input stream on the selected device, wiring its callback to
/// append samples to `buffer`, track peak amplitude, and emit `READY` on the
/// first non-empty block. Stores the negotiated sample rate. The returned
/// stream is not yet playing.
fn build_stream(
    host: &cpal::Host,
    device_name: &str,
    buffer: &Arc<Mutex<Vec<f32>>>,
    amp: &Arc<Mutex<f32>>,
    sample_rate: &Arc<AtomicU32>,
    app: &AppHandle,
    ready_sent: &Arc<AtomicBool>,
) -> Result<cpal::Stream, String> {
    let device =
        select_input_device(host, device_name).ok_or_else(|| "No microphone found".to_string())?;
    let config = device
        .default_input_config()
        .map_err(|e| format!("Audio config error: {e}"))?;

    let sample_format = config.sample_format();
    let stream_config: cpal::StreamConfig = config.into();
    let channels = stream_config.channels as usize;
    sample_rate.store(stream_config.sample_rate.0, Ordering::SeqCst);

    let err_fn = |e| eprintln!("[audio] stream error: {e}");

    let stream_result = match sample_format {
        cpal::SampleFormat::F32 => {
            let b = buffer.clone();
            let a = amp.clone();
            let app_handle = app.clone();
            let ready_sent_clone = ready_sent.clone();
            device.build_input_stream(
                &stream_config,
                move |data: &[f32], _: &_| {
                    if !data.is_empty() && !ready_sent_clone.swap(true, Ordering::SeqCst) {
                        let _ = app_handle.emit_to(events::FLOWBAR, events::READY, ());
                    }
                    ingest_f32(data, channels, &b, &a);
                },
                err_fn,
                None,
            )
        }
        cpal::SampleFormat::I16 => {
            let b = buffer.clone();
            let a = amp.clone();
            let app_handle = app.clone();
            let ready_sent_clone = ready_sent.clone();
            device.build_input_stream(
                &stream_config,
                move |data: &[i16], _: &_| {
                    if !data.is_empty() && !ready_sent_clone.swap(true, Ordering::SeqCst) {
                        let _ = app_handle.emit_to(events::FLOWBAR, events::READY, ());
                    }
                    ingest_i16(data, channels, &b, &a);
                },
                err_fn,
                None,
            )
        }
        other => return Err(format!("Unsupported audio format: {other:?}")),
    };

    stream_result.map_err(|e| format!("Audio stream error: {e}"))
}

fn ingest_f32(data: &[f32], channels: usize, buffer: &Mutex<Vec<f32>>, amp: &Mutex<f32>) {
    let mut buf = buffer.lock();
    let full = buf.len() >= MAX_CAPTURE_SAMPLES;
    let mut peak = 0.0f32;
    if channels <= 1 {
        if !full {
            buf.extend_from_slice(data);
        }
        for &s in data {
            peak = peak.max(s.abs());
        }
    } else {
        for frame in data.chunks(channels) {
            let m = frame.iter().copied().sum::<f32>() / channels as f32;
            if !full {
                buf.push(m);
            }
            peak = peak.max(m.abs());
        }
    }
    *amp.lock() = peak;
}

fn ingest_i16(data: &[i16], channels: usize, buffer: &Mutex<Vec<f32>>, amp: &Mutex<f32>) {
    let mut buf = buffer.lock();
    let full = buf.len() >= MAX_CAPTURE_SAMPLES;
    let mut peak = 0.0f32;
    for frame in data.chunks(channels.max(1)) {
        let sum: f32 = frame.iter().map(|&s| s as f32 / 32768.0).sum();
        let m = sum / channels.max(1) as f32;
        if !full {
            buf.push(m);
        }
        peak = peak.max(m.abs());
    }
    *amp.lock() = peak;
}

/// Linear resample to 16 kHz mono (Whisper's native rate).
pub fn resample_to_16k(samples: &[f32], src_rate: u32) -> Vec<f32> {
    if src_rate == 16_000 || samples.is_empty() {
        return samples.to_vec();
    }
    let ratio = src_rate as f64 / 16_000.0;
    let out_len = ((samples.len() as f64) / ratio) as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let pos = i as f64 * ratio;
        let idx = pos.floor() as usize;
        let frac = (pos - idx as f64) as f32;
        let a = samples[idx];
        let b = if idx + 1 < samples.len() {
            samples[idx + 1]
        } else {
            a
        };
        out.push(a + (b - a) * frac);
    }
    out
}

// --- Phase 3 (optimization): audio preprocessing + VAD -----------------------

/// How aggressively to trim silence, derived from the performance profile.
/// Higher `threshold_mult` cuts more borderline-quiet audio; larger `pad_ms`
/// keeps more around detected speech (i.e. less aggressive).
#[derive(Clone, Copy)]
pub struct VadParams {
    pub threshold_mult: f32,
    pub pad_ms: u32,
    pub min_peak_for_gain: f32,
}

impl VadParams {
    /// Map a performance-profile id (`Settings::local_transcription_profile`) to
    /// trimming aggressiveness. Unknown values fall back to "balanced".
    pub fn for_profile(profile: &str, correctness_rescue: bool) -> Self {
        if correctness_rescue {
            return VadParams {
                threshold_mult: 1.1,
                pad_ms: 320,
                min_peak_for_gain: 0.03,
            };
        }
        match profile {
            "fast" => VadParams {
                threshold_mult: 2.2,
                pad_ms: 80,
                min_peak_for_gain: 0.005,
            },
            "accurate" => VadParams {
                threshold_mult: 1.25,
                pad_ms: 220,
                min_peak_for_gain: 0.01,
            },
            _ => VadParams {
                threshold_mult: 1.6,
                pad_ms: 120,
                min_peak_for_gain: 0.01,
            },
        }
    }
}

/// Output of [`preprocess_local`]: silence-trimmed, peak-normalized 16 kHz mono
/// samples plus whether any speech was detected (drives the no-speech guard).
pub struct Preprocessed {
    pub samples: Vec<f32>,
    pub speech_detected: bool,
    pub trimmed: bool,
}

/// Local-only preprocessing for the on-device Whisper path: trim leading and
/// trailing silence with an adaptive RMS energy gate, then apply conservative
/// peak normalization. Input/output are 16 kHz mono. When the whole clip looks
/// like silence, returns the original samples with `speech_detected = false` so
/// the caller can fail fast without having mangled the audio. The cloud path is
/// unaffected — it keeps the full WAV encoded before this runs.
pub fn preprocess_local(samples: &[f32], params: VadParams) -> Preprocessed {
    const RATE: usize = 16_000;
    const WIN: usize = RATE * 30 / 1000; // 30 ms analysis window = 480 samples

    let n_win = samples.len() / WIN;
    if n_win < 2 {
        // Too short to analyze — pass through (normalized), assume speech.
        return Preprocessed {
            samples: normalize_peak(samples, params.min_peak_for_gain),
            speech_detected: true,
            trimmed: false,
        };
    }

    // Per-window RMS energy.
    let mut rms = Vec::with_capacity(n_win);
    for w in 0..n_win {
        let slice = &samples[w * WIN..w * WIN + WIN];
        let sum_sq: f32 = slice.iter().map(|s| s * s).sum();
        rms.push((sum_sq / WIN as f32).sqrt());
    }

    // Estimate the noise floor from the first ~300 ms (10 windows) when present,
    // taking the median so a stray click in the lead-in doesn't skew it.
    let floor_n = (300 / 30).min(n_win).max(1);
    let mut floor_vals: Vec<f32> = rms[..floor_n].to_vec();
    floor_vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let noise_floor = floor_vals[floor_vals.len() / 2];
    // Absolute gate floor so a dead-silent lead-in (noise_floor ≈ 0) still needs
    // real energy to register as speech.
    let threshold = (noise_floor * params.threshold_mult).max(0.005);

    let first = rms.iter().position(|&r| r > threshold);
    let last = rms.iter().rposition(|&r| r > threshold);
    let (Some(first), Some(last)) = (first, last) else {
        return Preprocessed {
            samples: samples.to_vec(),
            speech_detected: false,
            trimmed: false,
        };
    };

    // Pad the speech span by the profile's window, in whole analysis windows.
    let pad_win = (params.pad_ms as usize * RATE / 1000) / WIN;
    let start = first.saturating_sub(pad_win) * WIN;
    let end = ((last + pad_win + 1).min(n_win) * WIN).min(samples.len());

    let trimmed = start > 0 || end < samples.len();
    Preprocessed {
        samples: normalize_peak(&samples[start..end], params.min_peak_for_gain),
        speech_detected: true,
        trimmed,
    }
}

/// Conservative peak normalization: amplify quiet recordings toward a target
/// peak, never attenuate already-loud audio, and cap the gain so background
/// noise in a near-silent clip isn't blown up.
fn normalize_peak(samples: &[f32], min_peak_for_gain: f32) -> Vec<f32> {
    const TARGET: f32 = 0.7;
    const MAX_GAIN: f32 = 6.0;
    let peak = samples.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
    if peak < min_peak_for_gain || peak >= TARGET {
        return samples.to_vec();
    }
    let gain = (TARGET / peak).min(MAX_GAIN);
    samples
        .iter()
        .map(|&s| (s * gain).clamp(-1.0, 1.0))
        .collect()
}

/// Encode mono f32 samples as 16-bit PCM WAV at 16 kHz. Returns an error
/// instead of panicking so a writer failure surfaces as a normal pipeline error.
pub fn encode_wav(samples: &[f32]) -> anyhow::Result<Vec<u8>> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut cursor = std::io::Cursor::new(Vec::<u8>::new());
    {
        let mut writer = hound::WavWriter::new(&mut cursor, spec)
            .map_err(|e| anyhow::anyhow!("create wav writer: {e}"))?;
        for &s in samples {
            let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
            writer
                .write_sample(v)
                .map_err(|e| anyhow::anyhow!("write wav sample: {e}"))?;
        }
        writer
            .finalize()
            .map_err(|e| anyhow::anyhow!("finalize wav: {e}"))?;
    }
    Ok(cursor.into_inner())
}
