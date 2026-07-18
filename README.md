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

## Reclaiming Disk Space

Rust build output grows fast: multiple feature variants (CPU, CUDA, local LLM)
plus incremental-compile cache and debug symbols can push `src-tauri/target/`
past 25 GB. It is all regenerable and gitignored, so it is safe to delete
whenever you need the space back.

```sh
cd src-tauri
cargo clean          # remove all of target/ (debug + release)
```

Or delete just the debug tree, which is usually the bulk, and keep release
artifacts:

```sh
rm -rf src-tauri/target/debug   # PowerShell: Remove-Item src-tauri\target\debug -Recurse -Force
rm -rf build                    # old release installers, from `npm run release`
```

The next `cargo check` / `npm run tauri dev` rebuilds from scratch (slower once,
then incremental again).

Downloaded local models are **not** affected. They live in the OS app-data dir,
`%APPDATA%\com.eve.dictation\models` on Windows, entirely outside the project,
so cleaning build output never re-downloads them.

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

#### Fixing repeated permission prompts

Because the shipped bundles are unsigned, macOS cannot pin a stable code-signing
identity to the app. TCC stores permission grants (Microphone, Accessibility)
together with that identity, so with no valid signature the grant is discarded
and the system prompt reappears on every launch, even after clicking Allow.
Running the app straight from the DMG or from `~/Downloads` makes it worse:
Gatekeeper's App Translocation runs a quarantined unsigned app from a randomized
path each launch, so no grant can ever stick.

To make the grants persist:

```sh
# 1. Drag Eve.app from the DMG into /Applications with Finder first, then:
xattr -dr com.apple.quarantine /Applications/Eve.app
codesign --force --deep -s - /Applications/Eve.app   # stable ad-hoc signature
tccutil reset Microphone com.eve.dictation           # clear the broken grants
tccutil reset Accessibility com.eve.dictation
# 2. Launch Eve and grant Microphone (and Accessibility) once. They now persist.
```

An ad-hoc signature is keyed to the exact binary, so repeat the `codesign` step
after installing any new build. The permanent fix is Developer ID signing plus
notarization in the release workflow: the `APPLE_*` secrets are already
scaffolded (commented out) in `.github/workflows/release.yml`, and enabling them
requires an Apple Developer account. Note that notarization forces the hardened
runtime, which additionally needs a `com.apple.security.device.audio-input`
entitlement before microphone capture works in signed builds.

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
