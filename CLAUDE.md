# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

**Eve** is a Wispr Flow–style system-wide AI voice dictation app: hold a hotkey
anywhere, speak, release → the transcribed (and later AI-polished) text is typed
into whatever app has focus. **Tauri 2** (Rust backend + React/TypeScript/Vite
frontend), Windows-first. Transcription via the **Groq** Whisper API; AI polish
via Groq Llama (`llama-3.1-8b-instant`) shipped in Phase 2.

`plan.md` is the source of truth for roadmap and phase status (Phases 0–2 done;
Phase 3 = History & SQLite is next). Read it before starting feature work — each
phase section says what to build and exactly where.

## Intent Layer

**Before modifying code in a subdirectory, read its `AGENTS.md` first** for local
patterns and invariants. This CLAUDE.md is the only root context node; per-subsystem
detail lives in the child nodes below.

- **Frontend** — `src/AGENTS.md`: the two React webviews (Hub + event-driven Flow Bar) and the IPC mirror.
- **Backend** — `src-tauri/src/AGENTS.md`: Rust — global hotkey, mic capture, Groq calls, injection, the dictation pipeline.

### Global invariants

- The Rust↔TS IPC contract is **hand-mirrored** across three pairs (events, commands, `Settings`) — change both sides in the same edit. See the sync table under "Architecture".
- **Windows-first**: OS integration (HWND capture, `SendInput`, keychain) sits behind `#[cfg(windows)]`; cross-platform needs matching backends.
- Deferred work (SQLite, local Whisper, LLM polish) lives behind seams and is **not yet built** — `plan.md` says which phase adds each.

## Commands

```sh
npm install                 # install JS deps (also fetches Rust deps on first tauri run)
npm run tauri dev           # run the full app (spawns vite + builds/launches Rust) — primary dev loop
npm run tauri build         # release bundle (Windows installer/exe)

npm run dev                 # frontend only (vite, no Rust) — rarely useful alone
npm run build               # frontend typecheck + bundle: `tsc && vite build`. This is the JS lint/typecheck gate.
```

Rust checks (run from `src-tauri/`):

```sh
cargo check                 # fast type check
cargo build                 # debug build
cargo clippy                # lint
```

There is **no automated test suite**. Verification is: `npm run build` (clean
tsc + vite) + `cargo check`/`cargo build` (0 warnings expected), then manual —
run the app, paste a Groq key in Settings, hold F8 in Notepad, speak, release.

## Architecture

Two processes talking over Tauri IPC:

- **Rust backend** (`src-tauri/src/`) owns the OS integration: global hotkey,
  mic capture, HTTP to Groq, and text injection.
- **React frontend** (`src/`) is split across **two windows / two HTML entry
  points** (this is the non-obvious structural fact):
  - `index.html` → `main.tsx` → `Hub.tsx` — the **Hub** window (Dashboard +
    Settings), normal window, hides-to-tray on close.
  - `flowbar.html` → `flow-bar.tsx` — the **Flow Bar**, a frameless,
    transparent, always-on-top, non-focusable floating widget that is hidden
    until you dictate. It is purely **event-driven** (renders state from
    `session://*` events, sends no commands).

  Both windows are declared in `src-tauri/tauri.conf.json` (`label: "main"` /
  `"flowbar"`) and both are rollup inputs in `vite.config.ts`. Adding a window
  means: new `.html` + new rollup input + new window entry in `tauri.conf.json`.

### The dictation pipeline (the core flow)

`lib.rs` wires the global-shortcut handler, which dispatches to `hotkey.rs`:

1. **Key down** (`hotkey::on_press`) — guard against key-repeat via an atomic,
   capture the foreground window HWND (to paste back into later), show the Flow
   Bar, register **Esc** as a cancel shortcut, start the `audio.rs` capture
   thread.
2. **While held** — `audio.rs` runs a **`cpal` capture thread** (it owns the
   `!Send` audio stream; chosen over WebView `getUserMedia` to avoid permission
   friction and control sample rate), accumulates mono f32, and pushes live
   amplitude to the Flow Bar ~30×/s.
3. **Key up** (`hotkey::on_release`) — spawns `pipeline::process()`.
4. **`pipeline.rs::process`** — the post-release flow: drain buffer → resample
   to 16 kHz + WAV-encode (`audio.rs`, off the async runtime via
   `spawn_blocking`) → `transcriber.transcribe()` (Groq Whisper) →
   `text_processing::course_correct` → `polisher.polish()` (Groq Llama; no-op for
   `CleanupLevel::None`, falls back to raw on error) → `text_processing::finalize`
   (spoken punctuation + lists) → `injection::inject()` → emit `done`.
5. **`injection.rs`** — save clipboard → `SetForegroundWindow` to the captured
   HWND → write text → Win32 `SendInput` Ctrl+V (`paste` strategy) → restore
   clipboard. `enigo` char-by-char (`type` strategy) is the fallback.

**Esc** during recording → `hotkey::on_cancel` clears the buffer and hides the
bar.

### Trait seams for deferred work

Two pluggable boundaries let v1 ship without local models or LLM polish:

- `transcription.rs`: `Transcriber` trait — `GroqTranscriber` (live) +
  `LocalTranscriber` (stub for future on-device Whisper).
- `polish.rs`: `Polisher` trait — `GroqPolisher` (live, Groq Llama) +
  `NoOpPolisher` (fallback/tests). `AppState::new` always installs `GroqPolisher`;
  it short-circuits to a pass-through for `CleanupLevel::None`, so the level can
  change at runtime. Deterministic transforms live in `text_processing.rs`.

### State & concurrency

`state.rs::AppState` is Tauri-managed shared state. **All mutable fields are
`Arc`-backed** so the audio thread can own clones. **Rule:** snapshot the Arc
clones up front and **never hold a `parking_lot` guard across an `.await`**
(`pipeline::process` does this deliberately — see its opening block).

### The IPC contract — keep three pairs in sync

These Rust ↔ TypeScript pairs are hand-mirrored; changing one side requires
changing the other:

| Rust | TypeScript | What |
|---|---|---|
| `events.rs` (`START`/`PROCESSING`/`AMPLITUDE`/`DONE`/`ERROR`/`CANCEL`/`TRANSCRIPT_RAW`/`TRANSCRIPT_POLISHED`/`COPIED`) | `lib/api.ts` `EVT` map | `session://*` event names emitted to the Flow Bar (`START` carries `StartPayload`) |
| `commands.rs` + the `generate_handler!` list in `lib.rs` | `lib/api.ts` `api.*` wrappers | invokable commands |
| `config.rs` `Settings` (note `#[serde(rename_all = "camelCase")]`) | `lib/api.ts` `Settings` interface | settings shape — Rust uses `snake_case` fields serialized as `camelCase` |

**Adding a Tauri command** requires four edits: define it in `commands.rs`,
register it in the `generate_handler!` macro in `lib.rs`, add a wrapper in
`lib/api.ts`, and grant permission in `src-tauri/capabilities/default.json`.

### Persistence & secrets

- **Settings** → JSON file in the app config dir (`config.rs` load/save).
  SQLite is deferred to Phase 3.
- **Groq API key** → OS keychain only (Windows Credential Manager via
  `keyring`), never on disk (`secrets.rs`).

### Windows-specific code

HWND capture (`hotkey.rs`), `SendInput`/`SetForegroundWindow` (`injection.rs`),
and the keychain backend are Windows-only (behind `#[cfg(windows)]` / the
`windows` crate / `keyring` `windows-native` feature). Cross-platform support
would need matching backends.

## Frontend conventions

- React 19, **Tailwind v4** via `@tailwindcss/vite` (no `tailwind.config.js`;
  design tokens live in `src/styles/globals.css`, class-based dark mode).
  Fonts: Figtree + Fraunces. `zustand` is installed but not yet used.
- `tsconfig` is strict with `noUnusedLocals`/`noUnusedParameters` — unused
  symbols fail the `npm run build` gate.
