# Eve — Build Plan & Progress

A Wispr Flow–style system-wide AI voice dictation app. Hold a hotkey anywhere,
speak, release → cleaned-up text is typed into whatever app has focus.

**Stack (locked):** Tauri 2 (Rust backend + React/TypeScript/Vite frontend) ·
Groq Whisper `whisper-large-v3-turbo` for transcription · Groq Llama
`llama-3.1-8b-instant` for AI polish (Phase 2+) · Windows-first.
Local on-device Whisper is deferred behind a trait (not built in v1).

> Reference clones to study: `cjpais/handy`, `Open-Less/openless`,
> `moinulmoin/voicetypr`, `xarthurx/whisperi`.

---

## Status at a glance

| Phase | Title | Status |
|------:|-------|--------|
| 0 | Scaffolding | ✅ Done |
| 1 | Walking-skeleton MVP (hold → Groq → inject) | ✅ Done (compiles & links; live dictation untested) |
| 2 | AI polish + Flow Bar UX | ✅ Done (build-verified; live dictation untested) |
| 3 | History & DB (SQLite) | ✅ Done |
| 4 | Dictionary | ✅ Done (build-verified) |
| 5 | Snippets | ⬜ |
| 6 | Flow Styles + context awareness | ✅ Done (build-verified) |
| L | Local models (on-device transcription + polish) | ✅ Done (default build; `local-models` feature untested) |
| 7 | Command Mode + Transforms | ✅ Done (build-verified) |
| 8 | Insights + vibe-coding | ✅ Done (build-verified) |
| 9 | Scratchpad | ✅ Done (build-verified) |
| 10 | Onboarding + languages + auto-pause | ✅ Done (build-verified) |
| 11 | Packaging + signing + auto-update | ✅ Done (build-verified) |

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

## ✅ Phase 3 — History & DB (SQLite) — DONE

**Goal:** persist every dictation and surface it in a searchable History page.
This is where SQLite enters (deferred from Phase 0); it becomes the store for
Phases 4–9 too.

**Status:** built. `rusqlite` (`bundled`, FTS5) with a `PRAGMA user_version`
migration runner in new `src-tauri/src/db/` (`mod.rs`, `queries.rs`,
`migrations/001_initial.sql`); `db: Arc<Mutex<Connection>>` on `AppState`, opened
in `lib.rs` setup. `pipeline::process` persists each dictation after injection
(raw + polished, word/duration counts, optional saved WAV). Retention settings
(`audioStoragePolicy` / `audioRetentionHours`) prune audio on launch. Commands
`get_history` / `delete_transcript` (soft) / `recover_transcript` /
`clear_history` / `get_stats` wired through `api.ts`. History sidebar item is a
real page (`src/pages/HistoryPage.tsx`): paginated FTS search, per-card
raw↔polished toggle, audio replay via `convertFileSrc` (asset protocol enabled),
delete/recover. Build gates green: `npm run build`, `cargo check`.

**Deliverables:**
1. **DB layer** — add `rusqlite` (`bundled` feature) + a small migration runner
   (`refinery`, or hand-rolled `PRAGMA user_version`). New `src-tauri/src/db/`
   (`mod.rs`, `migrations/`, `queries.rs`). Open `app_data_dir/eve.db` in
   `lib.rs` setup; add `db: Arc<Mutex<rusqlite::Connection>>` to `AppState`.
2. **Schema (`001_initial.sql`)** — `transcripts` (id, created_at, raw_text,
   polished_text, cleanup_level, language, audio_path, app_process, app_title,
   app_category, word_count, duration_ms, was_polished, deleted_at) + FTS5
   `transcripts_fts` mirror for search; `daily_stats` (date PK, word_count,
   session_count, total_ms, correction_count, app_usage JSON).
3. **Persist on success** — in `pipeline.rs`, after injection, insert a row
   (raw, polished, word count = whitespace split, duration from the buffer
   length / rate, app info as plain "" until Phase 6). Optionally save the WAV to
   disk and store `audio_path`.
4. **Retention** — settings `audioStoragePolicy` (`store` | `delete24h` |
   `never`) + `audioRetentionHours`; a startup task in `lib.rs` prunes audio
   files + rows past the window.
5. **Commands** — `get_history(page, per_page, query)`, `delete_transcript(id)`
   (soft delete), `recover_transcript(id)`, `clear_history`, and extend
   `get_stats(range)`.
6. **History page** — promote the disabled "History" sidebar item in `Hub.tsx`
   to a real page (or `src/pages/HistoryPage.tsx`): paginated list, FTS search
   box, per-card raw↔polished toggle, audio replay (`<audio>` via
   `convertFileSrc`), delete/recover.

**Files:** new `db/`; modify `state.rs`, `lib.rs`, `pipeline.rs`, `commands.rs`,
`config.rs` (retention fields, `#[serde(default)]`), `Hub.tsx` + new page,
`lib/api.ts`.
**Verify:** run ~10 dictations → all appear in History, FTS search finds them,
audio replays, delete/recover work, retention prunes on the next launch.

## ✅ Phase 4 — Dictionary — DONE (build-verified)

**Goal:** word boosting + misspelling correction baked into every transcription.

**Status:** built. New migration `002_dictionary.sql` adds a `dictionary` table
(word UNIQUE NOCASE, nullable `replacement`, `is_starred`, `source`,
`learned_count`, timestamps); `db/dictionary.rs` holds the typed queries
(`upsert`/`list`/`delete`/`hints`/`corrections`). `pipeline.rs` now loads the
top-100 starred+recent terms and passes them to Whisper as the `prompt`
(boosting was already plumbed via `Transcriber::transcribe`'s `hints` arg), and
applies `text_processing::apply_corrections` (whole-word, case-insensitive,
longest-first) to the raw transcript before `course_correct`. Commands
`get_dictionary` / `upsert_dictionary_entry` / `delete_dictionary_entry` /
`import_dictionary_csv` / `export_dictionary_csv` are wired through
`generate_handler!` and `api.ts`. The Dictionary sidebar item is now a real page
(`src/pages/DictionaryPage.tsx`): searchable list, inline add/edit, star toggle,
delete, CSV import (file picker) + export (download). Auto-learn (deliverable 5)
was left as a future enhancement — the `source`/`learned_count` columns are in
place for it. Gates green: `cargo check`, `cargo test` (14/14 incl. 3 new
correction tests), `npm run build`.

**Deliverables:**
1. **Schema** — `dictionary` (id, word UNIQUE, replacement NULLABLE,
   is_starred, source `user|auto|import`, learned_count, timestamps).
2. **CRUD + CSV** — commands `upsert_dictionary_entry`, `get_dictionary`,
   `delete_dictionary_entry`, `import_dictionary_csv`, `export_dictionary_csv`.
3. **Boosting (already plumbed)** — `Transcriber::transcribe` already takes a
   `hints: Vec<String>` arg that maps to Whisper's `prompt`. In `pipeline.rs`
   load starred + recent words from the DB and pass them in (currently an empty
   `Vec`).
4. **Correction** — apply `replacement` mappings (misspelling → correct) as a
   post-transcription step. Add `apply_corrections(text, &dict)` to
   `text_processing.rs`, run before `course_correct`.
5. **Auto-learn (optional)** — after a session, detect proper nouns / repeated
   corrections; insert with `source='auto'` after N occurrences.
6. **Dictionary page** — promote the sidebar item: table with add/edit/delete,
   star, search, CSV import.

**Files:** db migration, `commands.rs`, `pipeline.rs`, `text_processing.rs`,
new `DictionaryPage.tsx`, `lib/api.ts`.
**Verify:** add "Tailwind" → dictating "tail wind" yields "Tailwind"; a
misspelling→correct mapping is applied in the injected text.

## ✅ Phase 5 — Snippets — DONE (build-verified)

**Goal:** spoken trigger phrases expand to long-form text.

**Deliverables:**
1. **Schema** — `snippets` (id, trigger_phrase UNIQUE, expansion, is_active,
   timestamps).
2. **CRUD + JSON import/export** commands.
3. **Expansion** — after `finalize`, before injection, scan for trigger phrases
   (case-insensitive; fuzzy ≤1 edit for short triggers) and substitute. New
   `expand_snippets(text, &snippets)` in `text_processing.rs` (unit-tested).
4. **Snippets page** — promote sidebar item; list + add/edit + JSON import.

**Files:** db migration, `commands.rs`, `pipeline.rs`, `text_processing.rs`,
new `SnippetsPage.tsx`, `lib/api.ts`.
**Verify:** define `my email → …@gmail.com`; dictating the phrase expands it
before injection.

## ✅ Phase 6 — Flow Styles + context awareness — DONE (build-verified)

**Goal:** per-app tone — the polish prompt adapts to the focused app.

**Status:** built. New `src-tauri/src/context/` (`mod.rs`, `active_window.rs`):
on Windows, `resolve(hwnd)` reads the title (`GetWindowTextW`) + owning process
(`GetWindowThreadProcessId` → `OpenProcess` → `QueryFullProcessImageNameW`) and
`classify(process, title)` maps it onto `AppCategory`
(`Email | WorkMsg | PersonalMsg | Code | Other`) via a process lookup table +
browser title/URL heuristics (added `windows` features `Win32_System_Threading`
+ `Win32_System_ProcessStatus`). `hotkey::on_press` resolves the context at
record start and stores it on `AppState.current_context`; `pipeline::process`
snapshots it, fills `app_process/app_title/app_category` when persisting (Phase
3 fields, previously empty), and looks up the active Flow Style for the
category. `polish.rs` gained a `StyleHint` (tone + category + custom prompt +
writing sample) that `system_prompt` weaves into the base cleanup prompt; the
trait/`pipeline` pass it through. New migration `004_flow_styles.sql` +
`db/flow_styles.rs` (`upsert` keyed by `app_category` UNIQUE / `list` /
`delete` / `active_for`). Commands `get_flow_styles` / `upsert_flow_style` /
`delete_flow_style` wired through `generate_handler!` + `api.ts`. The Styles
sidebar item is now a real page (`src/pages/StylesPage.tsx`): one card per
category with a 4-tone selector (preview text), an active toggle, a writing
sample, and extra-instructions field; styles only apply when cleanup is above
None. Gates green: `cargo check`, `cargo test` (22/22 incl. 3 new classify
tests), `npm run build`.

**Deliverables:**
1. **Active-window module** — new `src-tauri/src/context/active_window.rs`
   (Windows): `GetForegroundWindow` → `GetWindowThreadProcessId` →
   `QueryFullProcessImageNameW` (process name) + `GetWindowTextW` (title).
   Needs `windows` features `Win32_System_Threading` +
   `Win32_System_ProcessStatus`. `classify(process, title) -> AppCategory`
   (`Email | WorkMsg | PersonalMsg | Code | Other`) via a lookup table + browser
   title/URL heuristics (gmail/outlook → Email, etc.).
2. **Capture context at record start** — `hotkey::on_press` already grabs the
   HWND; also resolve process/title/category and store on `AppState`, then fill
   `app_process/app_title/app_category` when persisting (Phase 3).
3. **Schema** — `flow_styles` (id, name, app_category, tone
   `casual|formal|excited|very_casual`, system_prompt, writing_sample,
   is_active).
4. **Prompt builder** — extend `polish.rs::system_prompt` to take the active
   `FlowStyle` (tone + category + optional writing sample); `pipeline::process`
   looks up the style for the current category before polishing.
5. **Styles page** — 4×4 grid (category × tone) with text previews + a writing
   sample field; manual per-app override.

**Files:** new `context/active_window.rs` (+ `Cargo.toml` windows features),
`polish.rs`, `pipeline.rs`, `state.rs`, db migration, `commands.rs`, new
`StylesPage.tsx`, `lib/api.ts`.
**Verify:** Gmail tab focused → formal email tone; Slack → casual tone.

## ✅ Phase L — Local models (on-device transcription + polish) — DONE (default build)

**Goal:** run speech→text and AI polish fully on-device as an alternative to
Groq, selectable per-step in the UI, with in-app model downloads, instant
hot-swap, and automatic Groq fallback on local failure.

**Status:** built. Both AI steps now route through `RoutingTranscriber` /
`RoutingPolisher` (`transcription.rs` / `polish.rs`), which hold both the Groq
and local backends plus a clone of the live `Settings` and pick per call —
giving runtime hot-swap and Groq fallback without mutating `AppState`'s
`Arc<dyn>` fields. On-device inference (`whisper-rs` + `llama-cpp-2`) sits behind
the **`local-models` Cargo feature** (off by default so `cargo check`/`build`
need no C/C++ toolchain; the local backends then report "not built in" and
routing falls back to Groq).

1. **Settings:** `transcriptionBackend`/`polishBackend` (`"groq"|"local"`),
   `localWhisperModel`/`localLlmModel` (catalog ids) — `config.rs` + `lib/api.ts`.
2. **Download manager** (`models.rs`): static catalog (Whisper GGML + LLM GGUF),
   streamed `reqwest` download → `.part` → sha256 → atomic rename, cancel flag,
   `model://progress|done|error` events → `main` window. Weights in
   `app_data_dir/models/`. Commands `list_models`/`download_model`/
   `cancel_model_download`/`delete_model`.
3. **Engines:** `LocalTranscriber` (cached `WhisperContext`, WAV→f32 decode) and
   `LocalPolisher` (cached `LlamaModel`, reuses `system_prompt`/`strip_wrapping`),
   both lazily loaded + run under `spawn_blocking`.
4. **UI:** new `pages/LocalModelsPage.tsx` (backend selectors + model cards with
   download/delete + live progress bar), wired into `Hub.tsx` nav.

**Files:** new `models.rs`, `LocalModelsPage.tsx`; `config.rs`, `transcription.rs`,
`polish.rs`, `state.rs`, `lib.rs`, `commands.rs`, `events.rs`, `pipeline.rs`,
`Cargo.toml`, `lib/api.ts`, `Hub.tsx`.
**Verify (default build):** `cargo check`/`clippy` 0 warnings, `npm run build`
clean. **Untested:** `--features local-models` build (needs CMake + clang/MSVC;
the `whisper-rs`/`llama-cpp-2` glue may need minor API tweaks for the pinned
versions) and live on-device dictation.

## ✅ Phase 7 — Command Mode + Transforms — DONE (build-verified)

**Goal:** rewrite selected text (or generate inline) by voice; saved rewrite
prompts bound to shortcuts.

**Status:** built. New `llm.rs::chat`/`chat_with` factors the Groq
chat-completions round-trip out of `polish.rs` (GroqPolisher now just builds the
prompt + unwraps via the now-`pub strip_wrapping`); Command Mode and Transforms
reuse it. New `command_mode.rs` adds a second push-to-talk
(`Settings.commandShortcut`, default `CmdOrCtrl+Shift+Alt+Space`): hold → speak
an instruction → on release we transcribe it, read the focused selection via a
new `injection::capture_selection` (clear clipboard → `SetForegroundWindow` +
Ctrl+C → read → restore), then either rewrite the selection or generate inline
(`run_command`) and inject. The Flow Bar gets a distinct violet "Command" look
via a new `mode` field on `StartPayload`. Migration `005_transforms.sql` +
`db/transforms.rs` back the `transforms` table (name, system_prompt, shortcut,
auto_apply, app_category, is_active); active transforms with a shortcut are
registered as global accelerators at launch and re-registered after any edit
(`register_transform_shortcuts`, tracked in `AppState.transform_shortcuts`).
A transform shortcut rewrites the current selection; auto-apply transforms run
in `pipeline::process` after polish (scoped by app category). Commands
`set_command_shortcut` / `command_mode_rewrite` / `get_transforms` /
`upsert_transform` / `delete_transform` / `apply_transform` wired through
`generate_handler!` + `api.ts`. New `pages/TransformsPage.tsx` (create/edit/
delete, shortcut + auto-apply scope) + Hub nav item and a Command Mode settings
section. Gates green: `cargo check` (0 warnings), `cargo test` (22/22),
`npm run build` clean. **Untested:** live Command Mode / transform dictation
(needs a mic + Groq key + a real selection).

**Deliverables:**
1. **Shared LLM helper** — factor the Groq chat-completions call out of
   `polish.rs` into `llm.rs::chat(system, user)` so Command Mode/Transforms
   reuse it.
2. **Command-Mode shortcut** — a second global shortcut (e.g.
   `CmdOrCtrl+Shift+Alt+Space`) registered like the main one; gives the Flow Bar
   a distinct visual (e.g. purple). Handler in `hotkey.rs` / new
   `command_mode.rs`.
3. **Selection vs inline** — on activate, simulate Ctrl+C and read the clipboard;
   non-empty → "rewrite selection" mode, empty → "inline generation" mode. After
   the LLM returns, replace selection (clipboard + paste) or inject inline.
   Command `command_mode_rewrite(selected_text, instruction)`.
4. **Transforms** — `transforms` table (name, system_prompt, shortcut,
   auto_apply, app_category) + `transform_shortcuts`. Dynamically register each
   transform's shortcut; `apply_transform(id, text)`; `auto_apply` transforms run
   after dictation before injection.
5. **Transforms page** — create/edit named transforms, assign shortcut,
   auto-apply toggle, app-category filter.

**Files:** new `llm.rs`, `command_mode.rs`; modify `polish.rs`, `hotkey.rs`,
`lib.rs` (register), db migration, `commands.rs`, new `TransformsPage.tsx`,
`lib/api.ts`.
**Verify:** select text in VS Code → Command-Mode shortcut → say "make this more
concise" → selection is replaced.

## ✅ Phase 8 — Insights + vibe-coding — DONE (build-verified)

**Goal:** usage analytics dashboard + developer-specific niceties.

**Status:** built. `pipeline::process` now measures a per-session correction
count (`text_processing::count_edits` — word-level Levenshtein between the raw
transcript and the final injected text, a proxy for filler/punctuation/
dictionary/polish edits) and folds each finished dictation into the previously
unused `daily_stats` rollup via new `queries::record_daily` (counters via an
`ON CONFLICT(date)` upsert keyed on the session's *local* calendar day, plus a
read-modify-write of the `app_usage` JSON map). `queries::get_stats` was widened
to also return `corrections` (summed from `daily_stats`), an `app_usage`
breakdown and a per-day `daily` series (both grouped straight from
`transcripts` so history counts too). Vibe-coding: a new `Settings.vibe_coding`
toggle (default on) makes `text_processing::apply_vibe_coding` wrap spoken
"backtick X backtick" spans in literal backticks — applied in `pipeline` only
when the focused-app category is `code` (Phase 6 context), per line so
finalize's newlines survive. New `pages/InsightsPage.tsx` (range selector; WPM
radial gauge + "Top X%" badge vs. the 50 WPM typing benchmark; cleanup-per-100-
words card; app-usage horizontal bars; a 13-week streak heatmap with current-
streak count; and a "Your Voice" profile that unlocks at 2000 all-time words) +
Hub nav item and a Vibe-coding settings section. All charts are hand-rolled
SVG/CSS — **no `recharts`/`d3`/`date-fns`** were added (the rest of the UI is
Tailwind-only; pulling in charting libs for a gauge + a heatmap wasn't worth the
weight). Gates green: `cargo test` (26/26 incl. 4 new vibe/edit-count tests),
`cargo clippy` (no new warnings), `npm run build` clean. **Untested:** live
multi-session accumulation (needs a mic + Groq key over several dictations) and
vibe-coding backtick wrapping inside a real editor.

**Deliverables:**
1. **Aggregation** — track a per-session correction count in `pipeline.rs`
   (filler/punctuation/dictionary edits) into `daily_stats`. Extend `get_stats`
   to return WPM (words / (duration_ms/60000)), totals, app-usage breakdown,
   streak series.
2. **Insights page** — WPM radial gauge + "Top X%" badge (benchmark ≈ 50 WPM
   typed), corrections/total words, app-usage horizontal bar (`recharts`), streak
   heatmap (`d3`/SVG), "Your Voice" profile unlocked after 2000 words.
3. **Vibe-coding** — when the focused app is VS Code/Cursor (from Phase 6):
   backtick-wrap "backtick X backtick" → `` `X` `` and pass through `@file`
   tags; Settings toggle.

**Files:** `pipeline.rs`, `commands.rs`/`db`, new `InsightsPage.tsx`,
`text_processing.rs` (backticks), `config.rs` (vibe toggle), `lib/api.ts`. New
npm deps: `recharts`, `d3`, `date-fns`.
**Verify:** after several sessions Insights shows real WPM and filled streak
cells; backtick variables wrap in a code editor.

## ✅ Phase 9 — Scratchpad — DONE (build-verified)

**Goal:** a floating multi-tab rich-text notepad you can dictate into.

**Status:** built. A third window `scratchpad` (`tauri.conf.json`: resizable,
always-on-top, `skipTaskbar:false`, hidden until opened) joins `main`/`flowbar`
as a rollup input (`scratchpad.html` + `src/scratchpad.tsx`); closing it hides
rather than quits (like the Hub), so tabs stay loaded. New migration
`006_scratchpad.sql` + `db/scratchpad.rs` back `scratchpad_tabs` (title, HTML
content, position, timestamps) with `list`/`create`/`save`/`delete`; commands
`get_scratchpad_tabs` / `create_scratchpad_tab` / `save_scratchpad_tab`
(autosave) / `delete_scratchpad_tab` / `open_scratchpad` / `set_scratchpad_shortcut`
are wired through `generate_handler!` + `api.ts`. The editor is Tiptap
(`@tiptap/react` + `starter-kit` + `extension-image`, base64 paste handler),
single editor instance swapped per active tab, debounced autosave, inline title
+ multi-tab strip. **Focus-aware dictation:** `hotkey::on_press` compares the
foreground HWND against the Scratchpad window's `hwnd()` (new
`window_mgmt::scratchpad_hwnd`) and sets `AppState.to_scratchpad`; `pipeline`
snapshots it and, when set, emits the finished text as a new
`scratchpad://insert` event the window inserts at the cursor instead of
OS-pasting. **Entry points:** a global open shortcut (`Settings.scratchpad_shortcut`,
default `CmdOrCtrl+Shift+S`, registered in `lib.rs` + routed in the shortcut
handler) and a Hub sidebar item / Settings section that invoke `open_scratchpad`.
The new window is added to `capabilities/default.json` so it can listen for
events. Gates green: `cargo build` (0 warnings), `npm run build` clean.
**Untested:** live dictation into the editor + image paste (needs a mic + Groq
key + a running GUI).

**Deliverables:**
1. **New window** — `scratchpad` in `tauri.conf.json` (frameless-ish, resizable,
   always-on-top) + `scratchpad.html` vite entry + `src/scratchpad.tsx`.
2. **Schema** — `scratchpad_tabs` (id, title, content, position, timestamps) +
   CRUD commands; autosave.
3. **Editor** — Tiptap (`@tiptap/react`) rich text, multi-tab, image paste
   (base64 or file).
4. **Focus-aware dictation** — when the Scratchpad is focused, route the polished
   text into the active editor instead of OS paste.
5. **Entry points** — a Scratchpad open shortcut + Hub sidebar item.

**Files:** `tauri.conf.json` + `vite.config.ts` (new input), `scratchpad.html`,
`src/scratchpad.tsx`, `db/scratchpad.rs` + migration, `db/mod.rs`, `config.rs`,
`state.rs`, `events.rs`, `hotkey.rs`, `pipeline.rs`, `window_mgmt.rs`,
`commands.rs`, `lib.rs`, `capabilities/default.json`, `Hub.tsx`, `lib/api.ts`.
New npm deps: `@tiptap/react`, `@tiptap/starter-kit`, `@tiptap/extension-image`,
`@tiptap/pm`.
**Verify:** open Scratchpad, create 3 tabs, dictate into each, paste an image,
content survives a restart.

## ✅ Phase 10 — Onboarding + languages + auto-pause — DONE (build-verified)

**Goal:** first-run experience, multi-language selection, privacy guards.

**Status:** built. New first-run flow `src/components/onboarding/Onboarding.tsx`
gates the Hub while `settings.onboardingComplete` is false (a `#[serde(default)]`
bool added in `config.rs`): six steps — welcome, Groq key (reuses
`store_api_key`), hotkey pick, language multi-select, a **live mic test**
(browser `getUserMedia` → `AnalyserNode` amplitude meter, with a graceful
"denied" fallback since native dictation records via cpal anyway), and cleanup
level — then persists everything via `update_settings` + `set_shortcut` and
flips the flag. Shared option lists moved to `src/lib/options.ts`
(`LANGUAGES`/`CLEANUP`/`SHORTCUT_CHOICES`) so the Hub and onboarding share them.

**Languages:** new `languages: Vec<String>` setting drives a chip multi-select
(also surfaced in Settings). The frontend derives the single Whisper `language`
hint from it via `effectiveLanguage()` (one specific language → pinned; "auto"
or several → auto-detect), so the pipeline still just reads `language` — no
backend pipeline change.

**Auto-pause:** new `paused_apps: Vec<String>` (default-seeded with sensitive
desktop apps — password managers; process names only). `hotkey::on_press`
lowercases the resolved foreground process and, on a match, suppresses recording
(`is_recording` reset), emits a new `session://paused` event, and flashes a
"Paused here" pill on the Flow Bar (new `paused` state in `flow-bar.tsx`).
Editable chip list in Settings → Privacy.

**Privacy:** new `context_awareness: bool` (default on); when off, `on_press`
stores `AppContext::unknown()` instead of the resolved title/category (Flow
Styles stop adapting; History rows lose app info) — but auto-pause still reads
the bare process name. Settings → Privacy hosts the toggle, the paused-apps
editor, and the existing audio-retention controls.

**Files:** new `components/onboarding/Onboarding.tsx`, `lib/options.ts`;
`config.rs`, `events.rs`, `hotkey.rs`, `flow-bar.tsx`, `Hub.tsx`, `lib/api.ts`.
**Verify (build):** `cargo check`/`cargo test` (26/26) clean, `npm run build`
clean. **Untested:** the live onboarding flow + mic meter in a real WebView2,
and a paused app actually blocking a dictation.

## ✅ Phase 11 — Packaging + signing + auto-update — DONE (build-verified)

**Goal:** a distributable, self-updating Windows app.

**Status:** built. Added `tauri-plugin-updater` + `tauri-plugin-autostart`
(Cargo.toml), initialized both in `lib.rs`; `tauri.conf.json` gains
`plugins.updater` (GitHub Releases `latest.json` endpoint + a **placeholder
`pubkey`** that must be replaced with the real signing public key) and
`bundle.createUpdaterArtifacts: true`, and `capabilities/default.json` grants
`updater:default` + `autostart:default`.

**Auto-update:** commands `check_for_update` (returns the new version or `None`)
and `install_update` (download + install + relaunch) via `UpdaterExt`; the tray
gains a "Check for updates…" item that shows the Hub and emits a new
`app://check-update` event the Hub listens for — it jumps to Settings and runs
the check. Settings → "Startup & updates" has an `UpdateChecker` (Check now →
Install & restart) and a **Launch at startup** toggle wired to `set_autostart`
(reconciled with the saved `launch_at_startup` setting on launch via
`ManagerExt`).

**CI build + signing:** `.github/workflows/release.yml` builds NSIS + MSI +
updater artifacts on a `v*` tag via `tauri-action`, with env wiring for updater
signing (`TAURI_SIGNING_PRIVATE_KEY[_PASSWORD]`) and commented placeholders for
Azure Trusted Signing of the installer. Crash reporting (optional) was skipped.

**Files:** `Cargo.toml`, `lib.rs`, `commands.rs`, `tray.rs`, `events.rs`,
`tauri.conf.json`, `capabilities/default.json`, `config.rs`
(`launch_at_startup`), `Hub.tsx`, `lib/api.ts`, new
`.github/workflows/release.yml`.
**Verify (build):** `cargo check` (0 warnings, both new plugins compile),
`cargo test` (26/26), `npm run build` clean. **Untested (needs CI + a signing
key + a real release):** installer signing, the live updater feed/prompt, and
launch-at-startup registration. Replace the `pubkey` placeholder and set the CI
secrets before the first signed release.

## Known risks / things to watch (from the plan)
- Injection reliability per app (terminals need Shift+Insert; UAC-elevated targets
  can't be injected from a non-elevated process) — three-tier strategy planned.
- Clipboard restore skips non-text content (only text handled today).
- F8 may need Fn on some laptops — hotkey is configurable.
- Flow Bar focus stealing mitigated by HWND restore before paste.
