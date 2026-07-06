use crate::config::Settings;

#[cfg(windows)]
fn generate_start_sound() -> Vec<u8> {
    let sample_rate: u32 = 16000;
    let num_samples = 2400; // 0.15 seconds
    let num_channels: u16 = 1;
    let bits_per_sample: u16 = 16;
    
    let subchunk2_size = num_samples * num_channels as u32 * (bits_per_sample as u32 / 8);
    let chunk_size = 36 + subchunk2_size;
    
    let mut wav = Vec::with_capacity(44 + subchunk2_size as usize);
    
    // RIFF header
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&chunk_size.to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    
    // "fmt " subchunk
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes()); // subchunk1_size (16 for PCM)
    wav.extend_from_slice(&1u16.to_le_bytes());  // audio_format (1 for PCM)
    wav.extend_from_slice(&num_channels.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * num_channels as u32 * (bits_per_sample as u32 / 8)).to_le_bytes()); // byte_rate
    wav.extend_from_slice(&(num_channels * (bits_per_sample / 8)).to_le_bytes()); // block_align
    wav.extend_from_slice(&bits_per_sample.to_le_bytes());
    
    // "data" subchunk
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&subchunk2_size.to_le_bytes());
    
    // Sine wave with exponential decay (nice soft sound)
    use std::f32::consts::PI;
    let freq = 880.0; // Hz (A5)
    let decay_rate = 15.0; // Decay factor
    for i in 0..num_samples {
        let t = i as f32 / sample_rate as f32;
        let amplitude = 0.5 * (-decay_rate * t).exp();
        let val = (amplitude * (2.0 * PI * freq * t).sin() * 32767.0) as i16;
        wav.extend_from_slice(&val.to_le_bytes());
    }
    
    wav
}

#[cfg(windows)]
pub fn play_start_sound(settings: &Settings) {
    if !settings.sound_on_start {
        return;
    }
    
    use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};
    use windows::core::PCWSTR;
    
    static WAV_BYTES: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    let wav = WAV_BYTES.get_or_init(generate_start_sound);
    
    unsafe {
        let pszsound = PCWSTR(wav.as_ptr() as *const u16);
        let _ = PlaySoundW(pszsound, None, SND_ASYNC | SND_MEMORY | SND_NODEFAULT);
    }
}

/// Non-Windows start sound: the same 880 Hz decaying-sine chirp as the Windows
/// `PlaySoundW` path, synthesized as f32 samples and played through a cpal
/// default output stream. cpal is already compiled in (mic capture), so this adds
/// no new crate. The 12-ish duplicated tone constants are deliberate: the Windows
/// `generate_start_sound`/`PlaySoundW` path is left byte-identical.
#[cfg(not(windows))]
pub fn play_start_sound(settings: &Settings) {
    if !settings.sound_on_start {
        return;
    }
    // A cpal output stream is `!Send`/`!Sync` and must stay alive while it plays,
    // so a short-lived thread owns it and sleeps just past the tone before
    // dropping it. Best-effort: a missing/failing output device must never break
    // dictation.
    std::thread::spawn(|| {
        if let Err(e) = play_tone() {
            eprintln!("start sound failed: {e}");
        }
    });
}

#[cfg(not(windows))]
fn play_tone() -> anyhow::Result<()> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use std::f32::consts::PI;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    // 880 Hz (A5), 0.15 s, exponential decay - matches `generate_start_sound`.
    const FREQ: f32 = 880.0;
    const DECAY: f32 = 15.0;
    const DURATION: f32 = 0.15;

    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| anyhow::anyhow!("no default output device"))?;
    let config = device.default_output_config()?;
    let sample_rate = config.sample_rate().0 as f32;
    let channels = config.channels() as usize;
    let total_frames = (sample_rate * DURATION) as usize;
    let sample_format = config.sample_format();
    let config: cpal::StreamConfig = config.into();

    // Monotonic frame counter shared with the audio callback; yields 0.0 once the
    // tone has fully played so the tail is silent.
    let frame = Arc::new(AtomicUsize::new(0));
    let next = move |frame: &AtomicUsize| -> f32 {
        let n = frame.fetch_add(1, Ordering::Relaxed);
        if n >= total_frames {
            return 0.0;
        }
        let t = n as f32 / sample_rate;
        0.5 * (-DECAY * t).exp() * (2.0 * PI * FREQ * t).sin()
    };

    let err_fn = |e| eprintln!("cpal stream error: {e}");
    let stream = match sample_format {
        cpal::SampleFormat::F32 => {
            let frame = frame.clone();
            device.build_output_stream(
                &config,
                move |data: &mut [f32], _| {
                    for f in data.chunks_mut(channels) {
                        let s = next(&frame);
                        f.iter_mut().for_each(|c| *c = s);
                    }
                },
                err_fn,
                None,
            )?
        }
        cpal::SampleFormat::I16 => {
            let frame = frame.clone();
            device.build_output_stream(
                &config,
                move |data: &mut [i16], _| {
                    for f in data.chunks_mut(channels) {
                        let s = (next(&frame) * i16::MAX as f32) as i16;
                        f.iter_mut().for_each(|c| *c = s);
                    }
                },
                err_fn,
                None,
            )?
        }
        cpal::SampleFormat::U16 => {
            let frame = frame.clone();
            device.build_output_stream(
                &config,
                move |data: &mut [u16], _| {
                    for f in data.chunks_mut(channels) {
                        let s = ((next(&frame) * 0.5 + 0.5) * u16::MAX as f32) as u16;
                        f.iter_mut().for_each(|c| *c = s);
                    }
                },
                err_fn,
                None,
            )?
        }
        other => anyhow::bail!("unsupported output sample format: {other:?}"),
    };

    stream.play()?;
    // Hold the stream open a touch past the tone so its tail isn't clipped.
    std::thread::sleep(std::time::Duration::from_millis((DURATION * 1000.0) as u64 + 60));
    Ok(())
}
