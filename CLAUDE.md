# CLAUDE.md

This file gives coding agents the repository context needed to work on Eve.

## Project

Eve is a Windows-first system-wide AI voice dictation app. Hold a shortcut,
speak, release, and Eve transcribes, cleans up, and inserts the result into the
focused app.

The app uses Tauri 2 with a Rust backend and a React/TypeScript/Vite frontend.
Cloud transcription and polish use Groq. Local Whisper and local LLM support are
available as separate Cargo feature builds.

## Read First

Before modifying code in a subdirectory, read the nearest `AGENTS.md`:

- `src/AGENTS.md` for frontend conventions and IPC expectations
- `src-tauri/src/AGENTS.md` for backend invariants and OS-integration rules

Root roadmap documents such as `plan.md`, `parity_plan.md`,
`optimization_plan.md`, `fix_plan1.md`, and `cross-platform-plan.md` are planning
references, not automatically current implementation truth. Verify against the
code before changing behavior.

## Commands

```sh
npm install
npm run tauri dev
npm run build
npm run release
```

Rust checks from `src-tauri/`:

```sh
cargo check
cargo build
cargo clippy
```

There is no comprehensive automated test suite. Treat `npm run build`,
`cargo check`, and manual Windows dictation smoke testing as the default
verification baseline.

## Architecture

The app has three Tauri windows:

- `index.html` -> `src/main.tsx` -> `src/Hub.tsx`: main Hub window
- `flowbar.html` -> `src/flow-bar.tsx`: floating dictation status widget
- `scratchpad.html` -> `src/scratchpad.tsx`: always-on-top scratchpad

The Rust backend owns OS integration:

- global shortcut and low-level trigger handling
- microphone capture
- transcription and polish pipeline
- clipboard/text insertion
- SQLite persistence
- tray, autostart, updater, and app windows

## Core Flow

```text
shortcut down
-> capture foreground target and start microphone
-> emit flow-bar state
-> shortcut up
-> drain and encode audio
-> transcribe
-> course-correct, polish, finalize
-> restore focus
-> paste/type text
-> restore clipboard
```

`Esc` during recording cancels the current session.

## IPC Contract

Rust and TypeScript IPC are hand mirrored. Keep these in sync:

| Rust | TypeScript | Purpose |
| --- | --- | --- |
| `events.rs` | `src/lib/api.ts` `EVT` | Event names and payloads |
| `commands.rs` and `lib.rs` handler list | `src/lib/api.ts` wrappers | Tauri commands |
| `config.rs` `Settings` | `src/lib/api.ts` `Settings` | Settings shape |

Adding a command requires four edits:

1. Define the command in `src-tauri/src/commands.rs`.
2. Register it in the `generate_handler!` list in `src-tauri/src/lib.rs`.
3. Add the TypeScript wrapper in `src/lib/api.ts`.
4. Grant permission in `src-tauri/capabilities/default.json`.

## Backend Invariants

- Do not store API keys in settings, JSON, logs, or project files. Use
  `src-tauri/src/secrets.rs`.
- Do not hold a mutex/parking_lot guard across `.await`.
- Snapshot shared `Arc` state before async work in the pipeline.
- Keep Windows OS-integration behavior behind the existing cfg boundaries unless
  deliberately working on platform support.
- Local Whisper and local LLM builds are mutually exclusive feature variants.

## Frontend Invariants

- Tailwind v4 is configured through the Vite plugin and CSS tokens in
  `src/styles/globals.css`; there is no `tailwind.config.js`.
- `tsconfig` is strict. Unused locals and unused parameters fail `npm run build`.
- The Flow Bar is event-driven. Avoid adding command calls from `flow-bar.tsx`
  unless the window contract changes deliberately.

## Public Repo Hygiene

- Keep `.env*`, signing keys, release output, local model files, `target/`,
  `dist/`, `build/`, and local tooling output untracked.
- GitHub Actions should reference secrets through `secrets.*`; never commit the
  secret values themselves.
- The updater public key in `tauri.conf.json` is public material. The updater
  private key is not.
