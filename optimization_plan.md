# Local Dictation Optimization Plan

## Summary

Optimize the whole dictation pipeline in phases, starting with low-risk improvements
to the current Rust/whisper.cpp path, then moving toward deeper latency and quality
gains. The plan keeps the existing `Transcriber` routing model and prioritizes
local transcription speed, quality, and perceived responsiveness before considering
a larger backend replacement.

## Phase 1: Baseline and Timing Visibility

- Add structured timing around capture drain, resampling, WAV encoding, local model
  load, local inference, polish, injection, and total release-to-done latency.
- Log timings in dev builds and persist lightweight per-session metrics for
  comparison.
- Add user-visible stage events only where they help diagnose delays: processing
  audio, loading local model, transcribing, polishing, injecting.
- Establish benchmark clips: short command, normal dictation, long dictation,
  silence-heavy recording, noisy recording.

## Phase 2: Low-Risk Local Whisper Speedups

- Prewarm the selected local Whisper model when the app starts, when the user
  selects a model, or when switching transcription backend to local.
- Keep the existing context cache, but add explicit cache state so the UI can show
  whether the selected local model is ready.
- Configure whisper.cpp inference for speed:
  - Use greedy decoding as the default fast path.
  - Set thread count from available CPU cores, capped conservatively.
  - Disable timestamps, progress output, realtime output, and special tokens.
  - Pin language when the user selected exactly one language.
- Avoid the local WAV roundtrip by passing 16 kHz `Vec<f32>` samples directly into
  the local transcriber while keeping WAV bytes only for Groq upload/history replay.
- Keep fallback to Groq unchanged when local transcription fails and an API key
  exists.

## Phase 3: Audio Preprocessing and VAD

- Add local-only silence trimming before inference using an adaptive RMS energy
  gate:
  - 30 ms analysis windows.
  - Estimate noise floor from the first 300 ms when available.
  - Trim leading and trailing silence with a small speech padding window.
  - Preserve the original full WAV for history replay if audio retention is enabled.
- Add a no-speech guard after trimming so silence-heavy recordings fail faster.
- Normalize clipped or quiet input before transcription with conservative peak
  normalization.
- Replace the current linear resampler only if benchmarks show it is either slow or
  quality-limiting; otherwise keep it for simplicity.
- Apply preprocessing consistently to dictation mode and command mode.

## Phase 4: User-Selectable Performance Profiles

- Add a local transcription profile setting:
  - `fast`: prefer `tiny.en` or `base.en`, greedy decoding, aggressive VAD.
  - `balanced`: prefer `small.en`, greedy decoding, normal VAD.
  - `accurate`: allow `large-v3-turbo`, less aggressive VAD, same fallback behavior.
- Surface model recommendations in the Local Models page based on the selected
  profile.
- Keep existing model selection valid; profile should guide defaults and warnings,
  not silently replace the user's selected model.
- Add a small status panel showing backend, selected model, model readiness, and
  last local transcription timing.

## Phase 5: Pipeline Responsiveness

- Start local model prewarm in the background without blocking the Hub UI.
- Emit raw transcript as soon as STT completes, before polish or transforms.
- Keep polish optional and non-blocking where possible:
  - If cleanup level is `None`, skip polish entirely.
  - If polish fails or times out, inject the best available transcript.
- Review injection sleeps and clipboard timing to reduce fixed delays while
  preserving reliability.
- Add a debug mode that prints the full timing breakdown for one dictation session.

## Phase 6: High-Ceiling Backend Option

- After native improvements are measured, evaluate adding an optional faster ASR
  backend behind the existing `Transcriber` trait.
- Preferred candidate: `faster-whisper` / CTranslate2 as a sidecar runtime, only if
  packaging and startup cost are acceptable.
- Keep the native whisper.cpp backend as the default local backend unless the
  sidecar is proven faster and reliable on the target Windows machines.
- Compare backends using the same benchmark clips and metrics from Phase 1.
- Do not replace the existing local backend until fallback, installation, model
  downloads, and updates are fully specified.

## Public Interfaces and Types

- Add settings fields mirrored in Rust and TypeScript:
  - `localTranscriptionProfile: "fast" | "balanced" | "accurate"`
  - `localWhisperThreads: number | null`
  - `localVadEnabled: boolean`
  - `localPrewarmEnabled: boolean`
- Add internal timing payloads for dev/debug use:
  - stage name
  - duration in milliseconds
  - backend
  - model id
  - profile
- Extend local model status with readiness:
  - installed
  - active
  - loading
  - ready
  - lastLoadMs
- Preserve the existing `Transcriber` trait shape unless direct sample input is
  added; if added, introduce a local-only internal method rather than changing
  cloud upload behavior.

## Test Plan

- Run `npm run build`.
- Run Rust checks for default features and local Whisper features.
- Manually verify:
  - Groq transcription still works.
  - Local transcription works after cold start.
  - Local transcription is faster after prewarm.
  - Missing local model still falls back to Groq when an API key exists.
  - No API key plus missing local model shows a clear error.
  - Dictation mode and command mode both use the optimized path.
  - Audio history replay still stores the original recording when enabled.
- Benchmark before and after each phase using the same clips:
  - cold first dictation latency
  - warm dictation latency
  - STT-only latency
  - total release-to-injection latency
  - no-speech detection time
  - subjective transcript quality

## Assumptions

- Optimize the whole dictation pipeline, not only local STT.
- Start with the existing Rust/whisper.cpp implementation and defer sidecar
  replacement to the final evaluation phase.
- Prioritize Windows behavior because the current app is Windows-first.
- Do not remove Groq fallback.
- Do not require local LLM polish to ship in the same binary as local Whisper.
