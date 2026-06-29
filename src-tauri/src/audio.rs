//! Microphone capture via cpal (WASAPI on Windows). Recording runs on a
//! dedicated thread that owns the cpal stream (which is !Send), accumulates
//! mono f32 samples into a shared buffer, and emits an amplitude level to the
//! Flow Bar ~30×/sec for the waveform. Resampling to 16 kHz and WAV encoding
//! happen after the user releases the key (see `pipeline`).

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use parking_lot::Mutex;
use tauri::{AppHandle, Emitter};

use crate::events;
use crate::window_mgmt;

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

/// Spawn the capture thread. Runs until `is_recording` is set to false.
/// `device_name` selects the capture device by name (empty = system default).
pub fn start_capture(
    app: AppHandle,
    is_recording: Arc<AtomicBool>,
    buffer: Arc<Mutex<Vec<f32>>>,
    sample_rate: Arc<AtomicU32>,
    amp: Arc<Mutex<f32>>,
    device_name: String,
) {
    thread::spawn(move || {
        buffer.lock().clear();
        *amp.lock() = 0.0;

        let host = cpal::default_host();
        let Some(device) = select_input_device(&host, &device_name) else {
            is_recording.store(false, Ordering::SeqCst);
            window_mgmt::fail(&app, "No microphone found");
            return;
        };
        let config = match device.default_input_config() {
            Ok(c) => c,
            Err(e) => {
                is_recording.store(false, Ordering::SeqCst);
                window_mgmt::fail(&app, &format!("Audio config error: {e}"));
                return;
            }
        };

        let sample_format = config.sample_format();
        let stream_config: cpal::StreamConfig = config.into();
        let channels = stream_config.channels as usize;
        sample_rate.store(stream_config.sample_rate.0, Ordering::SeqCst);

        let err_fn = |e| eprintln!("[audio] stream error: {e}");

        let stream_result = match sample_format {
            cpal::SampleFormat::F32 => {
                let b = buffer.clone();
                let a = amp.clone();
                device.build_input_stream(
                    &stream_config,
                    move |data: &[f32], _: &_| ingest_f32(data, channels, &b, &a),
                    err_fn,
                    None,
                )
            }
            cpal::SampleFormat::I16 => {
                let b = buffer.clone();
                let a = amp.clone();
                device.build_input_stream(
                    &stream_config,
                    move |data: &[i16], _: &_| ingest_i16(data, channels, &b, &a),
                    err_fn,
                    None,
                )
            }
            other => {
                is_recording.store(false, Ordering::SeqCst);
                window_mgmt::fail(&app, &format!("Unsupported audio format: {other:?}"));
                return;
            }
        };

        let stream = match stream_result {
            Ok(s) => s,
            Err(e) => {
                is_recording.store(false, Ordering::SeqCst);
                window_mgmt::fail(&app, &format!("Audio stream error: {e}"));
                return;
            }
        };
        if let Err(e) = stream.play() {
            is_recording.store(false, Ordering::SeqCst);
            window_mgmt::fail(&app, &format!("Could not start microphone: {e}"));
            return;
        }

        // Push the latest amplitude to the Flow Bar while recording.
        while is_recording.load(Ordering::SeqCst) {
            let level = *amp.lock();
            let _ = app.emit_to(events::FLOWBAR, events::AMPLITUDE, level);
            thread::sleep(Duration::from_millis(33));
        }
        drop(stream); // releases the device
    });
}

fn ingest_f32(data: &[f32], channels: usize, buffer: &Mutex<Vec<f32>>, amp: &Mutex<f32>) {
    let mut buf = buffer.lock();
    let mut peak = 0.0f32;
    if channels <= 1 {
        buf.extend_from_slice(data);
        for &s in data {
            peak = peak.max(s.abs());
        }
    } else {
        for frame in data.chunks(channels) {
            let m = frame.iter().copied().sum::<f32>() / channels as f32;
            buf.push(m);
            peak = peak.max(m.abs());
        }
    }
    *amp.lock() = peak;
}

fn ingest_i16(data: &[i16], channels: usize, buffer: &Mutex<Vec<f32>>, amp: &Mutex<f32>) {
    let mut buf = buffer.lock();
    let mut peak = 0.0f32;
    for frame in data.chunks(channels.max(1)) {
        let sum: f32 = frame.iter().map(|&s| s as f32 / 32768.0).sum();
        let m = sum / channels.max(1) as f32;
        buf.push(m);
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
        let b = if idx + 1 < samples.len() { samples[idx + 1] } else { a };
        out.push(a + (b - a) * frac);
    }
    out
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
