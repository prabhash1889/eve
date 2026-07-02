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
- A **Groq API key** — https://console.groq.com/keys — required for the default
  (cloud) build. Optional if you build with on-device speech-to-text; see
  [Offline / on-device models](#offline--on-device-models-no-api-key).

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

## Offline / on-device models (no API key)

By default Eve transcribes and polishes via **Groq**, so the default build needs a
Groq API key. You can instead run **speech-to-text fully on-device** (whisper.cpp)
with no key and no network — behind a Cargo feature that compiles the native
inference engine.

### Prerequisites (Windows)

On-device inference compiles whisper.cpp / llama.cpp via CMake + a C/C++ toolchain:

- **Visual Studio Build Tools** (or VS Community) with the **"Desktop development
  with C++"** workload — provides the MSVC compiler + Windows SDK:
  ```sh
  winget install Microsoft.VisualStudio.2022.BuildTools --override "--quiet --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
  ```
- **CMake** — use the one bundled with Visual Studio (the **"C++ CMake tools for
  Windows"** component). A standalone/MinGW CMake older than your installed VS may
  not know the `Visual Studio <N> <year>` generator and will fail to configure with
  *"Could not create named generator …"*.

> If the build picks up the wrong CMake (e.g. an old MinGW one earlier on PATH),
> point it at the VS-bundled CMake. The cleanest way is a **machine-specific**
> `src-tauri/.cargo/config.toml` (keep it gitignored — the path is per-machine):
> ```toml
> [env]
> CMAKE = "C:\\Program Files\\Microsoft Visual Studio\\18\\Community\\Common7\\IDE\\CommonExtensions\\Microsoft\\CMake\\CMake\\bin\\cmake.exe"
> ```
> Adjust the path to your VS edition/year. (`tauri` runs `cargo` from `src-tauri/`,
> so the config is discovered there.)

### Build with on-device speech-to-text

```sh
npm run tauri dev   -- --features local-models   # dev
npm run tauri build -- --features local-models   # release
```

For NVIDIA GPU acceleration, build the local Whisper backend with CUDA:

```sh
npm run tauri:dev:local-cuda
npm run tauri:build:local-cuda
```

This enables Cargo feature `local-whisper-cuda` (`local-whisper` plus
`whisper-rs/cuda`). It still uses the native whisper.cpp backend, but the Local
models status panel reports whether this binary was built as `whisper.cpp CUDA`
or `whisper.cpp CPU`.

Then in **Settings → Local models**: set **Speech-to-text → Local**, download a
Whisper model, and click **Use**. Transcription now runs entirely on your machine —
no Groq key needed.

### On-device polish is a separate build

The local polish LLM (llama.cpp) and local Whisper **cannot be linked into the same
binary** — both statically bundle `ggml`, whose symbols collide at link time
(`LNK2005` → `LNK1169`). They're therefore mutually-exclusive Cargo features:

| Feature | On-device | Builds |
| --- | --- | --- |
| `local-whisper` (alias `local-models`) | speech-to-text | whisper.cpp |
| `local-whisper-cuda` | speech-to-text | whisper.cpp + CUDA |
| `local-llm` | AI polish | llama.cpp |

Pick **one** per build:

```sh
npm run tauri dev -- --features local-whisper   # offline transcription
npm run tauri dev -- --features local-whisper-cuda # offline transcription, CUDA
npm run tauri dev -- --features local-llm        # offline polish
```

A build with neither (the default) uses Groq for both and needs an API key. When a
local backend is selected but unavailable, Eve falls back to Groq if a key is set;
otherwise it surfaces an error (transcription) or inserts the raw, unpolished text
(polish).

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
