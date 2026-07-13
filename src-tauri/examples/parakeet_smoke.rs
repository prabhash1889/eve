//! Smoke test for the local Parakeet backend: transcribe a 16 kHz mono WAV
//! through parakeet-rs the same way `src/parakeet.rs` does. Fails loudly if the
//! model files, the ONNX runtime, or the API contract break.
//!
//! ```sh
//! cargo run --example parakeet_smoke --features local-parakeet -- <model_dir> <wav>
//! ```

#[cfg(feature = "local-parakeet")]
fn main() -> anyhow::Result<()> {
    use parakeet_rs::Transcriber as _;

    let mut args = std::env::args().skip(1);
    let model_dir = args.next().expect("usage: parakeet_smoke <model_dir> <wav>");
    let wav_path = args.next().expect("usage: parakeet_smoke <model_dir> <wav>");

    let mut reader = hound::WavReader::open(&wav_path)?;
    let spec = reader.spec();
    assert_eq!(spec.sample_rate, 16_000, "test WAV must be 16 kHz");
    assert_eq!(spec.channels, 1, "test WAV must be mono");
    let samples: Vec<f32> = reader
        .samples::<i16>()
        .map(|s| s.map(|v| v as f32 / 32768.0))
        .collect::<Result<_, _>>()?;

    let t0 = std::time::Instant::now();
    let mut model = parakeet_rs::ParakeetTDT::from_pretrained(&model_dir, None)?;
    println!("loaded in {:?}", t0.elapsed());

    let t1 = std::time::Instant::now();
    let result = model.transcribe_samples(samples, 16_000, 1, None)?;
    println!("transcribed in {:?}", t1.elapsed());
    println!("text: {}", result.text);

    assert!(!result.text.trim().is_empty(), "empty transcription");
    Ok(())
}

#[cfg(not(feature = "local-parakeet"))]
fn main() {
    eprintln!("build with --features local-parakeet");
}
