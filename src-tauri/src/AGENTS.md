# Backend (Rust / Tauri)

## Purpose

All OS integration and the dictation pipeline: global push-to-talk hotkey, mic capture,
resample/encode, Groq transcription, deterministic text cleanup + Groq Llama AI polish,
and text injection into the focused app. Exposes commands and emits `session://*` events
to the frontend. Owns no UI.

## Entry Points

- `lib.rs` — app builder: plugins, the global-shortcut handler (dispatches to `hotkey.rs`),
  `setup` (load settings, register shortcut, tray, position flowbar), and the
  `generate_handler!` command list.
- `main.rs` — binary shim → `eve_lib::run()`.
- `commands.rs` — `#[tauri::command]` functions invoked from the frontend.
- `pipeline.rs::process` — the post-key-release flow.

## The dictation flow (across files)

`hotkey::on_press` (guard key-repeat, capture foreground HWND, show bar, start
`audio::start_capture`) → while held the cpal thread accumulates f32 samples + pushes
amplitude ~30×/s → `hotkey::on_release` spawns `pipeline::process` → resample to 16 kHz +
WAV-encode (`audio.rs`) → `Transcriber` (`transcription.rs`, Groq Whisper) →
`text_processing::course_correct` → `Polisher` (`polish.rs`, Groq Llama; no-op for
`CleanupLevel::None`) → `text_processing::finalize` (spoken punctuation + lists) →
`injection::inject` (clipboard + `SetForegroundWindow` + Ctrl+V) → emit `done`.
`Esc` → `hotkey::on_cancel` (clear buffer, hide bar). `copy_shortcut` →
`hotkey::on_copy` (copy `last_transcript` to clipboard).

## Contracts & Invariants

- **Never hold a `parking_lot` guard across `.await`.** Snapshot the `Arc` clones from
  `AppState` up front, then drop the guard — see the opening block of `pipeline::process`.
- `AppState` (`state.rs`) is the single Tauri-managed state; all mutable fields are
  `Arc`-backed so the cpal capture thread (owner of the `!Send` stream) can hold clones.
- Event names (`events.rs`) and the `Settings` shape (`config.rs`, `serde camelCase`) must
  match `../../src/lib/api.ts`. **Adding a command = define here + register in `lib.rs`
  `generate_handler!` + wrapper in `api.ts` + permission in `capabilities/default.json`.**
- CPU/blocking work (resample, encode, injection) runs under `spawn_blocking`, off the async runtime.
- Secrets: the Groq API key lives **only** in the OS keychain (`secrets.rs`), never on disk.
  Settings persist as JSON (`config.rs`).

## Patterns

- Swap transcription/polish backends behind the `Transcriber` / `Polisher` traits, selected
  in `AppState::new`. `LocalTranscriber` is the remaining deferred slot (on-device Whisper).
- Windows-only OS calls (HWND, `SendInput`, keychain) go behind `#[cfg(windows)]`.

## Anti-patterns

- Don't block the global-shortcut callback thread — Esc registration and the pipeline are
  spawned off it (`tauri::async_runtime`).
- Don't read the API key from settings/JSON — always go through `secrets.rs`.
- Don't add a window or command without updating the frontend mirror in `api.ts`.

## Related Context

- Frontend + IPC mirror: `../../src/AGENTS.md`
- Project overview + full sync table: `../../CLAUDE.md`
- Roadmap & deferred phases: `../../plan.md`
