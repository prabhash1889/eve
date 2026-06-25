# Eve — Build Plan & Progress

A Wispr Flow–style system-wide AI voice dictation app. Hold a hotkey anywhere,
speak, release → cleaned-up text is typed into whatever app has focus.

**Stack (locked):** Tauri 2 (Rust backend + React/TypeScript/Vite frontend) ·
Groq Whisper `whisper-large-v3-turbo` for transcription · Groq Llama
`llama-3.1-8b-instant` for AI polish (Phase 2+) · Windows-first.
Local on-device Whisper is deferred behind a trait (not built in v1).

> Full original plan: `~/.claude/plans/i-want-to-build-frolicking-sunset.md`
> Reference clones to study: `cjpais/handy`, `Open-Less/openless`,
> `moinulmoin/voicetypr`, `xarthurx/whisperi`.

---

## Status at a glance

| Phase | Title | Status |
|------:|-------|--------|
| 0 | Scaffolding | ✅ Done |
| 1 | Walking-skeleton MVP (hold → Groq → inject) | ✅ Done (compiles & links; live dictation untested) |
| 2 | AI polish + Flow Bar UX | ✅ Done (build-verified; live dictation untested) |
| 3 | History & DB (SQLite) | ⬜ Next |
| 4 | Dictionary | ⬜ |
| 5 | Snippets | ⬜ |
| 6 | Flow Styles + context awareness | ⬜ |
| 7 | Command Mode + Transforms | ⬜ |
| 8 | Insights + vibe-coding | ⬜ |
| 9 | Scratchpad | ⬜ |
| 10 | Onboarding + languages + auto-pause | ⬜ |
| 11 | Packaging + signing + auto-update | ⬜ |

**Verification done:** `cargo check` + `cargo build` (debug `eve.exe`, 19.3 MB) →
0 errors / 0 warnings. `npm run build` (tsc + vite multi-page) → clean.
**Not yet done:** running the live GUI and dictating (needs a Groq key, a mic,
and a physical key-hold).

---

## ✅ Phase 0 — Scaffolding (DONE)

- Tauri 2 + React-TS + Vite baseline scaffolded and relocated into repo root
  (project renamed `eve_scaffold` → `eve`; package, Cargo, identifier
  `com.eve.dictation`).
- Two windows in `src-tauri/tauri.conf.json`: `main` (Hub) and `flowbar`
  (frameless, transparent, always-on-top, skip-taskbar, non-focusable, hidden).
- Capability `src-tauri/capabilities/default.json` covers both windows.
- Tailwind v4 (`@tailwindcss/vite`) design tokens in `src/styles/globals.css`:
  Figtree + Fraunces, soft neutrals, green accent, class-based dark mode.
- System tray (`src-tauri/src/tray.rs`); close-Hub-hides-to-tray.
- Settings persisted as JSON; API key in Windows Credential Manager (`keyring`).

> Deviation from original Phase 0: **SQLite is deferred to Phase 3** (it's first
> needed there). v1 settings use a JSON file via `config.rs`.

## ✅ Phase 1 — Walking-skeleton MVP (DONE — build-verified)

End-to-end pipeline implemented:

- **Hotkey hold** — `tauri-plugin-global-shortcut` handler in `lib.rs` routes
  `ShortcutState::Pressed/Released` to `hotkey.rs`. Default **F8**. Key-repeat
  guarded by an atomic. **Esc** registered during recording to cancel.
- **Audio** — `audio.rs` spawns a `cpal` capture thread (owns the !Send stream),
  accumulates mono f32, pushes live amplitude to the Flow Bar ~30×/s; on stop,
  linear-resamples to 16 kHz and WAV-encodes (`hound`).
  > Chose Rust `cpal` over webview `getUserMedia` to avoid WebView2 mic-permission
  > friction and to control sample rate.
- **Transcription** — `transcription.rs`: `Transcriber` trait + `GroqTranscriber`
  (multipart POST to `/openai/v1/audio/transcriptions`). `LocalTranscriber` is a
  stub for future on-device Whisper.
- **Polish** — `polish.rs`: `Polisher` trait + `NoOpPolisher` (passes raw text
  through in v1).
- **Injection** — `injection.rs`: save clipboard → write text → `SetForegroundWindow`
  to the window captured at key-down → Win32 `SendInput` Ctrl+V → restore clipboard.
  `type` strategy (enigo char-by-char) available as fallback.
- **Orchestration** — `pipeline.rs::process()` drains buffer → resample/encode →
  transcribe → polish → inject → emit `done`; friendly error mapping; never holds
  a state guard across `await`.
- **Flow Bar** — `src/flow-bar.tsx` listens to `session://*` events and renders
  idle/listening(waveform)/processing/done/error.
- **Hub** — `src/Hub.tsx`: Dashboard + Settings (Groq key, hotkey, language,
  cleanup level), dark/light toggle.
- **IPC contract** — `src/lib/api.ts` mirrors Rust commands + event names.

### How to run / verify Phase 1
```sh
npm install
npm run tauri dev
```
Settings → paste Groq key → Save · focus Notepad · **hold F8, speak, release** →
text pastes at the cursor. Esc cancels.
- If "No microphone found": Windows Settings → Privacy → Microphone → allow
  desktop apps.

---

## Current file map

```
src/
  main.tsx            Hub entry (fonts, theme, render Hub)
  Hub.tsx             Hub window: sidebar + Dashboard + Settings
  flow-bar.tsx        Flow Bar widget (event-driven states + waveform)
  lib/api.ts          Typed IPC: Settings type, EVT names, command wrappers
  styles/globals.css  Tailwind v4 tokens + light/dark
index.html            Hub HTML entry
flowbar.html          Flow Bar HTML entry
vite.config.ts        React + Tailwind + 2-page rollup input

src-tauri/src/
  main.rs             bin entry → eve_lib::run()
  lib.rs              Builder: plugins, global-shortcut handler, setup, tray, windows
  state.rs            AppState (Arc-backed) + parse_shortcut()
  config.rs           Settings struct + JSON load/save + CleanupLevel
  secrets.rs          keyring get/set/has/delete API key
  events.rs           Event-name consts + payload structs (sync w/ api.ts)
  hotkey.rs           on_press / on_release / on_cancel
  audio.rs            cpal capture thread, resample_to_16k, encode_wav
  transcription.rs    Transcriber trait, GroqTranscriber, LocalTranscriber (stub)
  polish.rs           Polisher trait, GroqPolisher (llama-3.1-8b-instant), NoOpPolisher
  text_processing.rs  deterministic course-correction, spoken punctuation, list formatting
  injection.rs        clipboard + Win32 Ctrl+V + focus restore; enigo type fallback
  pipeline.rs         process(): full post-release flow
  window_mgmt.rs      show/hide/position flowbar + fail() helper
  commands.rs         get/update_settings, set_shortcut, store/has/clear_api_key
  tray.rs             tray icon + menu
  tauri.conf.json     2 windows, bundle, identifier
  capabilities/default.json
```

### Key dependencies already wired
- Rust: `tauri` (tray-icon), `tauri-plugin-global-shortcut`,
  `tauri-plugin-clipboard-manager`, `tauri-plugin-opener`, `cpal`, `hound`,
  `enigo`, `reqwest` (multipart+json), `keyring` (windows-native), `async-trait`,
  `anyhow`, `parking_lot`, `windows` (Foundation, WindowsAndMessaging,
  Input_KeyboardAndMouse).
- npm: `react`, `@tauri-apps/api`, `tailwindcss` + `@tailwindcss/vite`,
  `zustand` (added, not yet used), `lucide-react`, `clsx`, `@fontsource/figtree`,
  `@fontsource/fraunces`.

---

## ✅ Phase 2 — AI polish + Flow Bar UX (DONE — build-verified)

Goal: real "flow" cleanup + a Flow Bar that feels like Wispr.

1. **`GroqPolisher`** (`polish.rs`) implements `Polisher`:
   - POST `/openai/v1/chat/completions`, model `llama-3.1-8b-instant`, temp 0.2.
   - `system_prompt(CleanupLevel)` scales the instruction Light→High; `strip_wrapping`
     defends against quote/preamble wrapping.
   - **Always installed in `AppState::new`** (not gated on level): it short-circuits
     to a pass-through for `CleanupLevel::None`, so the level can change at runtime.
     `pipeline::process` still falls back to raw on any API error.
2. **Deterministic pre/post-processing** (`text_processing.rs`, 11 unit tests):
   - `course_correct` (**pre-LLM**): excises spoken retractions —
     "scratch that / strike that / delete that / …" — and the clause they cancel.
   - `finalize` (**post-LLM**): `apply_spoken_punctuation` ("new line"→\n,
     "new paragraph"→\n\n, "period"/"comma"/"question mark"/… → symbols),
     `format_lists` (conservative: ordinals always, cardinals only at clause
     boundaries; needs a ≥3 sequential run), then whitespace normalization.
3. **Flow Bar polish** (`flow-bar.tsx`): CSS transitions on the bubble; raw→polished
   preview via new `transcript-raw` then `transcript-polished` events (emitted in
   `pipeline.rs`); bubble **size + opacity** applied from a `start` payload pushed by
   Rust (keeps the bar event-only). New `copied` state.
4. **copy-last-transcript** shortcut: `Settings.copy_shortcut` (default
   `CmdOrCtrl+Shift+C`) registered in `lib.rs`; `hotkey::on_copy` writes
   `AppState.last_transcript` to the clipboard and flashes "Copied". Configurable via
   the new `set_copy_shortcut` command.

New settings (with `#[serde(default)]` so old config files still load): `copyShortcut`,
`bubbleScale`, `bubbleOpacity`. New command: `set_copy_shortcut`. New events:
`transcript-raw`, `transcript-polished`, `copied`; `start` now carries a `StartPayload`.

**Verification:** `cargo build` + `cargo test` (11/11 text-processing tests pass) → 0
errors / 0 warnings; `npm run build` → clean. **Not yet done:** live dictation with a
mic + Groq key (filler words at level Medium → polished text injected).

## Roadmap notes for later phases
- **P3 SQLite**: add `rusqlite` + migrations; tables transcripts(+FTS5),
  dictionary, snippets, flow_styles, transforms, settings, daily_stats,
  scratchpad_tabs (schema in the original plan). Build History page.
- **P4–P5**: Dictionary (Whisper `prompt` hints already plumbed via
  `transcribe(hints)`), Snippets expansion before injection.
- **P6**: Win32 foreground process/title → AppCategory (HWND capture already
  exists in `hotkey.rs`); per-app tone styles feed the polish system prompt.
- **P7**: Command Mode shortcut + selection capture (Ctrl+C) + Transforms.
- **P8**: Insights (recharts/d3) + vibe-coding. **P9**: Scratchpad (Tiptap).
  **P10**: Onboarding + languages + auto-pause. **P11**: signing + updater +
  autostart.

## Known risks / things to watch (from the plan)
- Injection reliability per app (terminals need Shift+Insert; UAC-elevated targets
  can't be injected from a non-elevated process) — three-tier strategy planned.
- Clipboard restore skips non-text content (only text handled today).
- F8 may need Fn on some laptops — hotkey is configurable.
- Flow Bar focus stealing mitigated by HWND restore before paste.
