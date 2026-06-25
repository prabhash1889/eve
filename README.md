# Eve — system-wide AI voice dictation

A Wispr Flow–style dictation app: hold a hotkey anywhere, speak, release, and the
cleaned-up text is typed into whatever app has focus. Built with **Tauri 2**
(Rust backend + React/TypeScript frontend). Transcription via the **Groq**
Whisper API; AI polish via Groq Llama (Phase 2+).

> Status: **Phase 0 (scaffolding) + Phase 1 (walking-skeleton MVP)** complete.
> See `~/.claude/plans/i-want-to-build-frolicking-sunset.md` for the full roadmap.

## Prerequisites

- Rust (stable) + Cargo
- Node 18+ and npm
- Windows 10/11 with WebView2 (preinstalled on Win11)
- A **Groq API key** — https://console.groq.com/keys

## Install & run (dev)

```sh
npm install
npm run tauri dev
```

The Hub window opens and a tray icon appears. The floating Flow Bar is hidden
until you start dictating.

## How to use (Phase 1)

1. In the Hub → **Settings**, paste your Groq API key and click **Save**
   (stored in Windows Credential Manager, never on disk).
2. Optionally pick a **language** and **push-to-talk hotkey** (default **F8**).
3. Focus any text field (Notepad, VS Code, a browser box…).
4. **Hold F8**, speak, then **release**. The Flow Bar shows a live waveform while
   listening, "Transcribing…", then the text is pasted at your cursor.
5. Press **Esc** while holding to cancel.

> Cleanup level is "None" in v1 (raw transcript). LLM polish — filler removal,
> punctuation, tone styles — arrives in Phase 2.

## Architecture

```
src/                      React frontend (two entry points)
  main.tsx / Hub.tsx        Hub window: Dashboard + Settings
  flow-bar.tsx              Floating Flow Bar widget (event-driven)
  lib/api.ts                Typed IPC: commands + event names (mirrors Rust)
  styles/globals.css        Tailwind v4 design tokens (light/dark)

src-tauri/src/            Rust backend
  lib.rs                    App builder: plugins, windows, tray, shortcut wiring
  hotkey.rs                 Push-to-talk press/release/cancel handlers
  audio.rs                  cpal capture thread → 16 kHz mono → WAV
  transcription.rs          Transcriber trait + GroqTranscriber (+ local stub)
  polish.rs                 Polisher trait + NoOpPolisher (Groq polish = Phase 2)
  injection.rs              Clipboard + Win32 Ctrl+V paste, focus restore
  pipeline.rs               Orchestrates drain → transcribe → polish → inject
  window_mgmt.rs            Show/hide/position the Flow Bar
  config.rs / secrets.rs    JSON settings + keychain API key
  commands.rs / tray.rs     Frontend commands + system tray
```

### The dictation flow

`F8 down` → Rust captures the foreground window + starts the cpal mic thread +
shows the Flow Bar → waveform animates from live amplitude → `F8 up` → audio is
resampled to 16 kHz WAV → POSTed to Groq Whisper → (no-op polish in v1) → focus
restored to the original window → text pasted via clipboard + `Ctrl+V` →
clipboard restored.

## Build a release bundle

```sh
npm run tauri build
```

(Windows code signing is configured in Phase 11; unsigned builds trigger a
SmartScreen "Run anyway" prompt.)
