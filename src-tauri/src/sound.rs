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

#[cfg(not(windows))]
pub fn play_start_sound(_settings: &Settings) {
    // No-op on non-Windows platforms
}
