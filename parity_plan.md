# Feature Parity Plan (vs OpenSuperWhisper)

## Summary

Close the feature gaps identified in the comparison against
[OpenSuperWhisper](https://github.com/starmel/OpenSuperWhisper) (macOS,
local-first dictation). Scope decisions made up front:

- **Excluded by decision:** persisting recorded audio in history (and the
  playback UI that depends on it).
- **Deferred:** a second local engine (Parakeet). FluidAudio is Apple-only;
  the Windows path would be a large ONNX Runtime integration, and the CUDA
  Whisper build already covers the "fast local" need.
- **Best-effort only:** caret-anchored Flow Bar positioning. No Windows API
  reports the caret reliably in every app, so it ships default-off with a
  fixed-position fallback.

Everything else from the comparison is in scope, ordered by impact:
trigger flexibility (A), local-by-default packaging (B), file transcription
(C), translate mode (D), small polish items (E).

Invariants that apply to every phase:

- The Rust/TS IPC contract is hand-mirrored. Every new setting is a
  three-file edit (`config.rs` + `lib/api.ts` `Settings` + Settings UI).
  Every new command is a four-file edit (`commands.rs`, `generate_handler!`
  in `lib.rs`, `lib/api.ts` wrapper, `capabilities/default.json`). Every new
  event updates `events.rs` + the `EVT` map in `lib/api.ts`.
- OS integration stays behind `#[cfg(windows)]`.
- Never hold a `parking_lot` guard across an `.await`.
- Verification gate per phase: `npm run build` clean + `cargo check`/`cargo
  build` with 0 warnings, then a live dictation smoke test.

**Hard prerequisite before Phase A:** one full live end-to-end test of the
current app (mic + Groq key + F8 in Notepad). The app is build-verified but
has never been exercised live; trigger-handling changes must start from a
known-good baseline.

## Status

- Phase A (triggers) - **Done** (build-verified; live smoke test pending).
  A1: `activation_mode` hold/toggle/hybrid state machine in `hotkey.rs`
  (`on_main_pressed`/`on_main_released`, 300 ms hold threshold, `saw_release`
  key-repeat guard) + "tap to stop" hint and 15-min buffer warning on the Flow
  Bar (`session://limit`, `StartPayload.toggle_hint`). A2: free shortcut
  recorder (`src/components/ShortcutCapture.tsx`); `set_shortcut` now rejects
  unparseable accelerators instead of falling back to F8. A3/A4:
  `src-tauri/src/hooks.rs` - WH_KEYBOARD_LL + WH_MOUSE_LL hooks with a
  dispatcher thread; `modifier_trigger` / `mouse_trigger` settings; injected
  events ignored (so paste's Ctrl+V can't retrigger); bound mouse button is
  consumed. Hooks route through the same activation-mode entry points.
- Phase B (local-by-default) - **Done** (build-verified; live onboarding
  smoke test pending). B1: release builds are CPU `local-whisper`
  (`release.yml` + `scripts/release.mjs`); `scripts/release.mjs --cuda`
  produces the `local-whisper-cuda` variant into `build/<version>/cuda/`.
  B2: the CPU build is the sole updater channel; the CUDA build is a
  manually-attached power-user artifact kept out of `latest.json`
  (documented in `release.yml` + `release.mjs` headers). B3: onboarding
  forks after Welcome into Cloud (Groq key, explicitly skippable) vs
  Private (`ModeStep` + `LocalModelStep` in `Onboarding.tsx`); the Private
  branch reuses the `list_models`/`download_model` + `model://*` machinery,
  auto-selects the downloaded model, sets
  `transcription_backend = "local"`, and prewarms on finish. B4: all six
  catalog entries in `models.rs` now carry exact sizes + SHA-256 from the
  Hugging Face LFS pointers, so downloads are verified. Parakeet second
  engine stays deferred (see Summary).
- Phase C (file transcription) - **Done** (build-verified; live drop/transcribe
  smoke test pending). Migration 7 adds a `source_file` column to `transcripts`
  (`queries.rs` `Transcript`/`NewTranscript` carry it; mic dictations write
  `None`). New `src-tauri/src/file_transcribe.rs` decodes dropped/picked files
  with `symphonia` (wav/mp3/m4a/flac/ogg), downmixes to mono, reuses
  `audio::resample_to_16k`, and runs them through the existing
  `transcriber.transcribe_audio` -> dictionary/course-correct -> polish
  (time-bounded) -> `finalize` -> history path; injection is skipped. A
  `VecDeque<QueuedFile>` in `AppState` is drained serially by one worker task
  (`queue_worker_running` guards against a second worker; the flag is cleared
  under the queue lock so no wakeup is lost); `cancel_queue_item` drops a pending
  file and abandons an in-flight one at the next stage boundary
  (`queue_cancelled`). Groq's 25 MB WAV cap is pre-checked per file (shared
  `transcription::GROQ_MAX_WAV_BYTES`, also now used by the mic pipeline); long
  files error clearly (chunking is a follow-up). IPC: `queue://progress | done |
  error` events + `transcribe_files`/`cancel_queue_item` commands (four-file
  adds, `dialog:default` capability, `tauri-plugin-dialog`). UI: `FileQueue.tsx`
  on the Dashboard - window `onDragDropEvent` drop zone + "Transcribe files…"
  picker, per-item stage/cancel card; finished items reload the embedded History
  list and show a source-file badge (`HistoryPage.tsx`).
- Phase D (translate mode) - **Done** (build-verified; local and Groq backends support translate and initial prompt settings).
- Phase E (polish batch) - **Done** (build-verified; includes min-duration guard, start sound, mic ready event, search highlight, CJK autocorrect, and caret-anchored Flow Bar).

## Phase A: Trigger Overhaul

The biggest daily-use gap. Four independent tracks; A1 and A2 are small,
A3 and A4 share new hook machinery.

### A1. Activation modes: hold / toggle / hybrid

New setting `activation_mode: "hold" | "toggle" | "hybrid"` (default
`"hold"`, the current behavior).

- `hotkey.rs`: wrap the existing `on_press`/`on_release` in a small state
  machine.
  - **hybrid** (OpenSuperWhisper's model, the recommended default to offer):
    on `Pressed` when idle, start capture and stamp a `press_instant`. On
    `Released`: if held > 300 ms, treat as push-to-talk and run the
    pipeline; if < 300 ms, stay recording (the tap armed a toggle). The
    next `Pressed` stops and processes.
  - **toggle**: same without the 300 ms branch - press starts, press stops.
- Key-repeat trap: while a key is held the OS delivers repeated `Pressed`
  events. The current `is_recording.swap(true)` guard eats them, but in
  toggle/hybrid mode a "second press" must stop the recording - so only
  accept a stop-press after a `Released` has been observed since the
  start-press (one extra `AtomicBool`, e.g. `saw_release`).
- Esc-cancel and the `is_processing` guard are unchanged. The 15-minute
  buffer ceiling matters more in toggle mode (the user can walk away while
  recording): surface a Flow Bar warning as the cap approaches.
- Flow Bar: in toggle/hybrid mode, show a subtle "tap <key> to stop" hint
  in the listening state.

### A2. Free shortcut recorder

Replace the fixed 5-item dropdown in `SettingsPanel` with a capture field:
focus it, press a combo, build the accelerator string from the
`KeyboardEvent`, validate by round-tripping through `set_shortcut`. Change
`state::parse_shortcut` failure handling so the command returns an error to
the UI instead of silently falling back to F8; keep the current dropdown
entries as one-click suggestions.

### A3. Bare-modifier triggers (Right Alt, Right Ctrl, Fn-style keys)

`tauri-plugin-global-shortcut` cannot express "just Right Alt", so this
needs a Windows low-level keyboard hook.

- New `src-tauri/src/hooks.rs` (`#[cfg(windows)]`): a dedicated thread
  installs `SetWindowsHookExW(WH_KEYBOARD_LL, ...)` and runs a `GetMessage`
  pump. Match on VK codes (`VK_RMENU`, `VK_LMENU`, `VK_RCONTROL`,
  `VK_RSHIFT`, ...), dedupe auto-repeat via previous key state, and dispatch
  down/up into the same `on_press`/`on_release` entry points.
- Hook-proc discipline: never block inside the proc (Windows silently
  removes low-level hooks that exceed the `LowLevelHooksTimeout`, ~300 ms
  default). Set atomics / send on a channel and return immediately.
- Setting `modifier_trigger: Option<String>` (`"right_alt"`,
  `"left_ctrl"`, ...). When set it is an **additional** trigger alongside
  the accelerator, not a replacement (simpler than OpenSuperWhisper's
  mutually-exclusive modes, and strictly more useful).

### A4. Mouse-button triggers

Same `hooks.rs` thread adds `WH_MOUSE_LL`: match `WM_MBUTTONDOWN/UP` and
`WM_XBUTTONDOWN/UP` (X1/X2 thumb buttons). **Return 1 from the hook proc
for the bound button** so the click is consumed and does not leak to the
app under the cursor - this is the detail that makes the feature feel
right. Setting `mouse_trigger: Option<String>`
(`"middle" | "x1" | "x2"`).

Settings added in Phase A: `activation_mode`, `modifier_trigger`,
`mouse_trigger`.

## Phase B: Ship Local-by-Default

The local code exists behind feature gates; this phase is packaging and
onboarding.

1. **Build matrix**: release artifact builds with `--features
   local-whisper` (CPU). Keep `local-whisper-cuda` as a second artifact.
   Touch `scripts/release.mjs` + `.github/workflows/release.yml`. Do not
   bundle a model (installer stays small).
2. **Updater caveat**: `tauri-plugin-updater` has one `latest.json` per
   platform, so two variants cannot share a feed. Resolution: the CPU build
   is the updater channel; the CUDA build is a manually-downloaded
   power-user artifact (document this). Revisit only if it becomes painful.
3. **Onboarding fork**: add a step before the Groq-key step - "Cloud (fast
   setup, needs API key) or Private (on-device, download a model)". The
   local branch reuses `LocalModelsPage`'s download machinery (progress
   events already exist) and sets `transcription_backend = "local"`. The
   Groq-key step becomes skippable.
4. **Checksum the catalog**: fill in the `sha256` fields in `models.rs`
   (all `None` today, so downloads are unverified - worth fixing
   regardless).
5. Parakeet second engine: explicitly deferred (see Summary).

## Phase C: File Transcription + Queue

Transcribe dropped/picked audio files. No audio is copied or stored -
files are read in place and history keeps text plus the source path.

- **Decode**: add `symphonia` (pure-Rust demux/decode: wav/mp3/m4a/flac/ogg)
  in a new `src-tauri/src/file_transcribe.rs`; decode, downmix to mono,
  reuse the existing 16 kHz resampler in `audio.rs`.
- **Queue**: `VecDeque<QueueItem>` in `AppState` (Arc-backed like the rest)
  plus one worker `tokio::task` processing serially through the existing
  `transcriber.transcribe()` -> polish -> history insert. Injection is
  skipped entirely for file items.
- **Schema**: migration 7 adds a `source_file` column to `transcripts`.
- **IPC**: new events `queue://progress | done | error`; new commands
  `transcribe_files(paths)`, `cancel_queue_item(id)` (standard four-file
  command adds).
- **UI**: Tauri `onDragDropEvent` on the Hub window + a "Transcribe
  files..." button (`tauri-plugin-dialog` for the picker). A queue card on
  the Dashboard with per-item progress/cancel; finished items appear in
  History like any dictation.
- **v1 limits, stated in the UI**: Groq's 25 MB cap applies per file
  (pre-check like the mic path already does); long files are not chunked
  yet - error clearly instead. Chunking-with-overlap is a follow-up.

## Phase D: Translate-to-English

Setting `translate_to_english: bool`, rendered as a toggle next to the
language chips.

- **Local**: `whisper-rs` `FullParams::set_translate(true)` - one line in
  `run_inference`.
- **Cloud gotcha**: Groq's translation endpoint
  (`/openai/v1/audio/translations`) does **not** support
  `whisper-large-v3-turbo`, only `whisper-large-v3`. `GroqTranscriber` must
  switch both endpoint and model when the flag is on. Verify against
  current Groq docs at implementation time.
- Optional same-pass addition: user-facing `whisper_prompt` setting,
  prepended to the dictionary terms Eve already sends as the Whisper
  prompt.

## Phase E: Polish Batch

Each item is small; land as one batch.

1. **Min-duration guard**: in `pipeline::process`, if the drained buffer is
   under 1 s, hide the bar with a brief "Too short" pill instead of
   uploading. No setting.
2. **Record-start sound**: setting `sound_on_start: bool` (default false).
   Windows: `PlaySoundW` with `SND_ASYNC` on a bundled short WAV - no new
   audio dependencies.
3. **Mic warm-up state**: emit a `session://ready` event from the capture
   thread when the first real frames arrive; the Flow Bar shows "starting
   mic..." until then. This is the honest fix for Bluetooth mics that eat
   the first second of speech.
4. **Search highlighting** in `HistoryPage.tsx`: wrap FTS query terms in
   `<mark>` in result cards. Frontend-only.
5. **CJK autocorrect**: add the `autocorrect` crate (the same library
   OpenSuperWhisper wraps, natively Rust) to `text_processing::finalize`,
   applied when the effective language is zh/ja/ko, behind a default-on
   setting (`cjk_autocorrect: bool`).
6. **Caret-anchored Flow Bar** (last, riskiest): setting
   `bar_position: "fixed" | "near_caret"` (default `"fixed"`). Resolution
   order: `GetGUIThreadInfo` (classic Win32 caret rect) -> UI Automation
   `TextPattern2::GetCaretRange` (modern apps/browsers) -> fall back to the
   fixed position. Clamp to the monitor work area.

## Sequencing

- A, B, D, E are independent of each other; C's history migration should
  land before E4 touches `HistoryPage.tsx` to avoid churn.
- Recommended order: live-baseline test -> A -> B -> C -> D -> E.
