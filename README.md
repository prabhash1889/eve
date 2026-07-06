# Eve

System-wide AI voice dictation for Windows, built with Tauri 2, Rust, React, and
TypeScript.

Hold a shortcut anywhere, speak, release, and Eve transcribes the audio, cleans up
the text, and inserts it into the focused app. Eve can use Groq for cloud
transcription and polish, or a local whisper.cpp build for on-device
speech-to-text.

## Status

Eve is an early Windows-first desktop app. The main dictation flow, history,
snippets, dictionary, scratchpad, file transcription, local Whisper builds, and
release packaging are in progress or implemented. macOS and Linux support are
planned but not production-ready.

## Features

- Push-to-talk dictation with a floating flow bar
- Cloud transcription through Groq Whisper
- AI cleanup and deterministic text transforms
- Optional local whisper.cpp speech-to-text builds
- History, snippets, dictionary, transforms, and scratchpad surfaces
- File transcription queue
- Windows tray app with updater-enabled release bundles
- API key storage through the OS credential store, not project files

## Prerequisites

- Windows 10/11
- Rust stable and Cargo
- Node.js 20+ and npm
- WebView2 runtime
- Groq API key for the default cloud build

Local Whisper builds additionally require CMake and a C/C++ toolchain. On Windows,
Visual Studio Build Tools with the "Desktop development with C++" workload is the
recommended setup.

## Development

```sh
npm install
npm run tauri dev
```

Useful checks:

```sh
npm run build
cd src-tauri
cargo check
```

The frontend-only Vite server can be started with:

```sh
npm run dev
```

That is useful for UI work, but it does not run the Rust backend or OS integration.

## Local Speech-To-Text Builds

The default build uses Groq for speech-to-text and polish. To build Eve with
on-device speech-to-text, enable the local Whisper feature:

```sh
npm run tauri dev -- --features local-whisper
npm run tauri build -- --features local-whisper
```

CUDA builds are available for NVIDIA systems:

```sh
npm run tauri:dev:local-cuda
npm run tauri:build:local-cuda
```

Local Whisper and local LLM polish are separate build variants. They cannot be
linked into the same binary because whisper.cpp and llama.cpp both vendor ggml
symbols. Use one of these feature sets per build:

```sh
npm run tauri dev -- --features local-whisper
npm run tauri dev -- --features local-whisper-cuda
npm run tauri dev -- --features local-llm
```

## Release Builds

Create a local release bundle with:

```sh
npm run release
```

The release script collects installers into `build/<version>/`. Release artifacts,
signing keys, local model files, and generated build output are intentionally
ignored by git.

The GitHub release workflow builds the CPU `local-whisper` Windows updater channel.
It expects updater signing material to be configured as GitHub Actions secrets:

- `TAURI_SIGNING_PRIVATE_KEY`
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`

Do not commit private signing keys. The updater public key in
`src-tauri/tauri.conf.json` is safe to publish.

## Platform Support

Windows is the primary, fully featured platform. macOS and Linux (X11 and
Wayland, via runtime detection) are supported through a cross-platform seam; see
`cross-platform-plan.md`.

Permissions each platform needs before dictation works:

| Capability | Windows | macOS | Linux (X11) | Linux (Wayland) |
| --- | --- | --- | --- | --- |
| Microphone | granted on first use | Microphone (system prompt) | ALSA/PulseAudio | ALSA/PulseAudio |
| Global triggers | none | Accessibility (onboarding step) | none (XI2 raw) | GlobalShortcuts portal (prompt on first run) |
| Paste / focus | none | Accessibility | none | virtual-keyboard protocol (KDE, wlroots) |
| Key storage | Credential Manager | Keychain | Secret Service | Secret Service |

### Known Wayland degradations

The compositor does not expose foreign-window focus, so on Wayland:

- Bare-modifier and mouse-button triggers are hidden (only portal accelerators).
- Privacy pause cannot match the focused app, Flow Styles use the default style,
  and history app attribution is blank.
- Esc-cancel is unavailable; cancel by toggling the main trigger instead.
- GNOME lacks the virtual-keyboard protocol; paste falls back to typing (an
  `ydotool` opt-in is documented as a follow-up).

### Installing on macOS

Builds are not yet Apple-notarized. On first launch, right-click the app and
choose **Open**, or clear the quarantine attribute:

```sh
xattr -d com.apple.quarantine /Applications/Eve.app
```

### Installing on Linux

Install the `.deb`, `.rpm`, or AppImage for your distro. The in-app updater only
updates AppImage installs; `.deb`/`.rpm` users update through their package
manager or by downloading a new release.

## Privacy And Secrets

- User API keys are stored in the operating system credential store.
- Settings are stored as local app configuration.
- Audio files and generated artifacts are local runtime/build data and should not
  be committed.
- `.env*`, signing keys, release output, local models, and Rust/Node build output
  are gitignored.

Before publishing a fork, run a secret scan over both the working tree and git
history.

## Architecture

```text
src/
  main.tsx / Hub.tsx          Hub window
  flow-bar.tsx                Floating dictation widget
  scratchpad.tsx              Scratchpad window
  lib/api.ts                  Typed Tauri IPC wrappers
  styles/globals.css          Tailwind v4 design tokens

src-tauri/src/
  lib.rs                      App builder, plugins, windows, tray
  hotkey.rs / hooks.rs        Shortcut and trigger handling
  audio.rs                    Microphone capture and WAV encoding
  transcription.rs            Cloud/local transcription boundary
  polish.rs                   Cloud/local cleanup boundary
  pipeline.rs                 Dictation processing pipeline
  injection.rs                Clipboard and text insertion
  config.rs / secrets.rs      Settings and OS credential-store access
  db/                         SQLite persistence
```

Core flow:

```text
shortcut down -> capture audio -> shortcut up -> transcribe -> polish/finalize
-> restore focus -> paste/type text -> restore clipboard
```

## Contributing

This repository does not currently have a full automated test suite. At minimum,
run `npm run build` and `cargo check` before submitting changes. For OS-integration
changes, manually smoke test dictation into a normal text field on Windows.
