# Local Dictation Optimization Plan

## Summary

Optimize the whole dictation pipeline in phases, starting with low-risk improvements
to the current Rust/whisper.cpp path, then moving toward deeper latency and quality
gains. The plan keeps the existing `Transcriber` routing model and prioritizes
local transcription speed, quality, and perceived responsiveness before considering
a larger backend replacement.

## Status

- Phase 1 — **Done** (timing visibility, per-session CSV metrics, stage events).
- Phase 2 — **Done** (prewarm, context cache state, greedy/thread/flag tuning,
  direct-sample local path).
- Phase 3 — **Done** (adaptive RMS silence trimming + no-speech guard +
  conservative peak normalization; local-only, applied to dictation and command
  modes; full WAV preserved for history/cloud).
- Phase 4 — **Done** (fast/balanced/accurate profiles, profile-driven model
  recommendations, thread override, VAD/prewarm toggles, status panel with last
  local timing).
- Phase 5 — **Done** (background prewarm honors the toggle, raw transcript emitted
  before polish, polish skipped on `None` / bounded by a timeout with raw fallback,
  injection sleeps reviewed and trimmed, `debugTiming` per-stage breakdown).
- Phase 6 — **Done (evaluation)**. Evaluated a `faster-whisper`/CTranslate2 sidecar;
  decision is to keep native whisper.cpp as the default local backend and defer the
  sidecar until its full lifecycle (install, downloads, fallback, updates) is
  specified. See the Phase 6 section for the decision and adoption gate.
- Phase 7 — **Done**. Inference-parameter tuning (Track A) and GPU enablement
  (Track B — CUDA sm_120 build via Ninja + `-allow-unsupported-compiler`, see
  `src-tauri/.cargo/config.toml`). Verified on the RTX 5060: `large-v3-turbo`
  went from 12–25 s/clip on CPU to **0.05–0.7 s/clip** on the CUDA build
  (`latency.csv`, 2026-07-02), total release-to-done ≈ 0.3–0.9 s. See the
  Phase 7 section.

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

## Phase 3: Audio Preprocessing and VAD — Done

Implemented in `audio.rs` (`preprocess_local` / `VadParams` / `normalize_peak`),
wired into `pipeline.rs` (local path only, before building `Audio`) and
`command_mode.rs`. The full WAV is encoded before trimming, so history replay and
the Groq fallback keep the original recording. Trimming is gated on
`local_vad_enabled` + the local backend; a clip that reads as all-silence fails
fast with "No speech detected".

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

## Phase 4: User-Selectable Performance Profiles — Done

New settings `localTranscriptionProfile`, `localWhisperThreads`,
`localVadEnabled`, and `localPrewarmEnabled` are mirrored in `config.rs` and
`lib/api.ts`. The profile tunes VAD aggressiveness (`VadParams::for_profile`) and
drives model recommendations on the Local Models page. `whisper_threads` now
honors the explicit thread override. `WhisperStatus` gained `lastTranscribeMs`,
shown in a new status panel (backend / model / readiness / last local timing)
alongside the profile selector and VAD/prewarm toggles.

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

## Phase 5: Pipeline Responsiveness — Done

Implemented across `pipeline.rs`, `injection.rs`, `lib.rs`, `timing.rs`,
`config.rs`, and the frontend (`lib/api.ts`, `Hub.tsx`).

- Background prewarm at startup (`lib.rs`) is spawned off the UI thread and now
  honors `local_prewarm_enabled`, so a user who opted out keeps a cold start
  instead of paying an unexpected load. The Local Models page already prewarms on
  backend switch / model select, also gated on the toggle.
- The raw transcript is emitted (`TRANSCRIPT_RAW`) the instant STT completes,
  before dictionary corrections, course-correction, polish, or transforms run, so
  the Flow Bar previews words as early as possible.
- Polish is optional and never blocks injection:
  - `CleanupLevel::None` skips the LLM call entirely (no round-trip).
  - Otherwise the polish call is bounded by a 20 s timeout; on timeout *or* error
    the pipeline injects the best available transcript (the course-corrected text)
    rather than making the user wait on a slow/hung model.
- Injection sleeps were reviewed and trimmed (`injection.rs`): the pre-paste
  settle stays at 40 ms; the post-paste settle dropped 150 → 120 ms, kept
  comfortably above typical clipboard-read latency so the target app reads our
  payload before the clipboard guard restores the prior contents. Both are named
  constants now (`PRE` / `PASTE_SETTLE`) with the reliability rationale documented.
- A `debugTiming` setting (off by default, mirrored Rust/TS, toggled in
  Settings → Startup & updates) makes `timing.rs` print a detailed per-stage
  breakdown — each stage's milliseconds and its share of total release-to-done
  latency, plus backend/model/profile — on top of the always-on one-line log and
  CSV row.

## Phase 6: High-Ceiling Backend Option — Done (evaluation)

This phase is an evaluation/decision gate, not an implementation: the plan
explicitly says **do not replace the existing local backend until fallback,
installation, model downloads, and updates are fully specified**. After the
Phase 1–5 native improvements, this is the outcome of that evaluation.

**Candidate evaluated:** `faster-whisper` (CTranslate2) running as a sidecar
process, exposed behind the existing `Transcriber` trait as a third backend
alongside `GroqTranscriber` and `LocalTranscriber`.

**Why it is attractive**

- CTranslate2 with INT8 quantization is typically 2–4× faster than whisper.cpp on
  CPU at comparable accuracy, and scales better on multi-core machines.
- It keeps the `Transcriber` seam intact — the pipeline already passes 16 kHz
  `Vec<f32>` samples + WAV, so a sidecar backend slots in without touching the
  cloud-upload path.

**Why it is deferred (cost on a Windows-first app)**

- *Packaging:* `faster-whisper` is Python + native CTranslate2 libs. Shipping it
  means either bundling a Python runtime / PyInstaller binary (tens to hundreds of
  MB) or a standalone CTranslate2 build — a large jump from the single self-
  contained whisper.cpp static link we have today.
- *Startup cost:* a sidecar adds process spawn + model load + an IPC handshake on
  the first dictation, which works against the very responsiveness Phase 5 just
  improved. Prewarm helps but adds lifecycle complexity (health checks, restart on
  crash, port/stdio management).
- *Lifecycle not yet specified:* separate model files (CTranslate2 format, not the
  GGML files the current downloader fetches), their download/verify/update flow,
  and crash-recovery / fallback semantics are all unspecified. The plan forbids
  adoption until they are.

**Decision:** keep native whisper.cpp as the default and only local backend. The
`Transcriber` trait already provides the seam, so a sidecar can be added later as
an additive backend without disrupting Groq or whisper.cpp.

**Adoption gate (must all be specified + benchmarked before building the sidecar):**

1. Benchmark the sidecar against whisper.cpp on the Phase 1 clips and the Phase 1
   metrics (cold/warm latency, STT-only latency, total release-to-injection). It
   must win decisively on the target Windows machines to justify the packaging cost.
2. Specify packaging: runtime bundling, binary size budget, code-signing, and the
   installer impact.
3. Specify the model lifecycle: CTranslate2 model catalog, download + checksum +
   update flow, and disk layout (the current downloader is GGML-only).
4. Specify reliability: sidecar spawn/health-check/restart, IPC protocol, and
   fallback to whisper.cpp (then Groq) on sidecar failure — fallback must never
   regress from today's behavior.

## Phase 7: Inference-Parameter Tuning + GPU Enablement

Driven by the measured `latency.csv`: with Phases 1–5 in place, every stage
except inference is already negligible (drain ≈60 ms, resample ≈5 ms, VAD ≈2 ms,
inject ≈200 ms, polish skipped at `None`). Inference is ~99% of release-to-done
latency. Observed: `large-v3-turbo` on the CPU build = **12–25 s/clip**;
`small.en` ≈ **2.5 s**, but with worst-case spikes to **19 s** from whisper's
temperature-fallback loop. So this phase targets inference directly, on two
tracks.

### Track A — inference parameters (Done)

All in `transcription.rs::run_inference` unless noted. Two regimes by profile:
fast/balanced optimize for low, *predictable* latency; accurate/correctness-rescue
optimize for quality.

- **Cap the encoder audio context** (`set_audio_ctx`, new `audio_ctx_for`). The
  encoder otherwise always processes the full 1500-token / 30 s context, so a
  short dictation pays for ~30 s of silence. We cap it to the clip length (~50
  tokens/sec + 20% headroom, clamped to [256, 1500]); VAD has already trimmed to
  speech so the cap never truncates. Quality keeps the full 1500. Biggest CPU win
  for short clips; helps every model and the GPU path too.
- **Greedy by default** (gating + `config.rs` default of `local_beam_search_enabled`
  flipped to `false`). Beam search (size 5) cost ~2–3× a greedy decode for a
  marginal dictation gain. Now: fast → greedy; balanced → greedy unless the toggle
  is on; accurate / correctness-rescue → beam search always.
- **Disable temperature fallback** outside the quality regime (`set_temperature_inc(0.0)`).
  This is the loop that re-decodes a hard clip up to ~6× at rising temperature —
  the source of the 19–25 s spikes. One pass keeps latency bounded; quality modes
  keep the fallback.
- **`set_no_context(true)`** — each dictation is an independent clip.
- **UI (`LocalModelsPage.tsx`)** — updated the beam-search toggle copy and added a
  loud warning when `large-v3-turbo` is selected on a **CPU** build (reads the
  `whisper.cpp CPU` vs `whisper.cpp CUDA` backend label), steering users to
  `small.en` or the CUDA build.

The `Tuning` struct now groups the per-call knobs (threads/profile/beam/rescue)
snapshotted from `Settings`, so the speed/quality decisions live in one place.

### Track B — GPU enablement (CUDA, done)

Target machine: RTX 5060 Laptop = **Blackwell, compute capability 12.0 (sm_120)**,
which needs CUDA ≥ 12.8 (CUDA 13.0 is installed). The `local-whisper-cuda` build
was failing; root causes found from the build logs:

1. **Stale CMake build dir** — a prior interrupted CUDA configure left a
   `CMakeCache.txt` using the **Ninja** generator; whisper-rs-sys/cmake-rs then
   auto-picked the **Visual Studio** generator, and CMake refuses to switch
   in-place. Fixed by `cargo clean -p whisper-rs-sys`.
2. **The VS generator can't build CUDA here** — CUDA 13.0 installs its MSBuild
   integration (`CUDA*.props`/`.targets`) only for VS 2019/2022, **not** the VS
   2026 (v18) preview, so the VS generator fails "no CUDA toolset found". Fix:
   force **Ninja** (invokes `nvcc` directly), built from an x64 MSVC env.
3. **Blackwell arch + new host compiler** — set `CUDAARCHS=120` (sm_120), and
   `NVCC_PREPEND_FLAGS=-allow-unsupported-compiler` because nvcc 13.0 rejects the
   MSVC 14.50 (cl 19.50) host. All three live in the gitignored, machine-specific
   `src-tauri/.cargo/config.toml [env]`.
- **`flash_attn`** is enabled on the CUDA context in `transcription.rs::ensure_context`
  (cfg-gated to `local-whisper-cuda`; no effect on the CPU build). Safe — it only
  conflicts with DTW, which we don't use.

Expected result once the CUDA build links: `large-v3-turbo` ≈ 1–2 s/clip at full
accuracy. Falls back to Groq exactly as before if the local backend errors.

## Public Interfaces and Types

- Add settings fields mirrored in Rust and TypeScript:
  - `localTranscriptionProfile: "fast" | "balanced" | "accurate"`
  - `localWhisperThreads: number | null`
  - `localVadEnabled: boolean`
  - `localPrewarmEnabled: boolean`
  - `debugTiming: boolean` (Phase 5: per-stage latency breakdown to the console)
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
