# Eve — Fix Plan #1 (Failure-Mode Remediation)

Derived from the full failure-mode audit (build + clippy passed clean; these are
runtime risks). Ordered by impact. Each item lists the file(s), the fix, and how
to verify.

**Status:** Phase 1 (1.1, 1.2) and Phase 2 (2.1–2.6) are **done** (this commit).
Phases 3 and 4 are not started. Verification run: `npm run build` clean; `cargo
check` + `cargo clippy` clean on default features (clippy warning count unchanged
at the pre-existing 23). The `local-models`-gated paths (1.2, 2.3) compile-check
locally requires CMake/Ninja for the native whisper.cpp/llama.cpp build scripts;
the edits there mirror the existing cache/spawn_blocking patterns.

Verification gates for every change:
- Frontend: `npm run build` (tsc + vite, must stay clean)
- Rust: `cargo check` then `cargo clippy` (0 errors; keep warnings from rising)
- Manual: run app, paste Groq key, hold hotkey in Notepad, speak, release.

---

## Phase 1 — Critical (stops the "app froze / Flow Bar stuck forever" class) — ✅ DONE

### 1.1 Add HTTP timeouts to every outbound request — ✅ DONE
- **Files:** `src-tauri/src/llm.rs` (`chat_with`), `src-tauri/src/transcription.rs`
  (`GroqTranscriber::transcribe`), `src-tauri/src/models.rs` (`download_to_file`).
- **Fix:**
  - Build a single shared `reqwest::Client` with
    `.timeout(...)` + `.connect_timeout(...)` (e.g. 60s overall / 10s connect for
    Groq; for downloads use a long-but-finite per-request read timeout, or wrap the
    stream poll in `tokio::time::timeout` so a dead connection can't hang forever).
  - Stop constructing `reqwest::Client::new()` per call in `chat_with` — store one
    `Client` on `AppState` (or a `OnceCell`) and reuse it. Fixes connection-pool
    churn / TIME_WAIT buildup too.
- **Verify:** point the base URL at a non-responsive host (or pull the network mid
  call); the pipeline must surface an error and the Flow Bar must clear, not hang.
- **Implemented:** added `llm::groq_client()` (shared `OnceLock<Client>`, 10s
  connect / 60s overall) used by `chat_with`; `GroqTranscriber::new` builds a
  client with 10s connect / 120s overall; the download client uses a 30s connect
  + 60s per-read timeout (`read_timeout`) with no overall cap (multi-GB downloads).

### 1.2 Move blocking model load off the async runtime — ✅ DONE
- **Files:** `src-tauri/src/transcription.rs` (`LocalTranscriber::transcribe`),
  `src-tauri/src/polish.rs` (`LocalPolisher::polish`).
- **Fix:** Wrap the heavy `WhisperContext::new_with_params(...)` /
  `LlamaModel::load_from_file(...)` loads in `tauri::async_runtime::spawn_blocking`.
  Do not hold the `parking_lot` cache mutex across the load — load first, then take
  the lock only to insert the `Arc` into the cache (double-check the cache after
  acquiring to avoid a duplicate concurrent load).
- **Verify:** trigger a cold load of a large local model; other async work (UI
  responsiveness, a second event) must not freeze during the load.
- **Implemented:** both `LocalTranscriber::transcribe` and `LocalPolisher::polish`
  now take the cache lock only briefly for the fast path, run the heavy load in
  `spawn_blocking` with no lock held, then re-check the cache before inserting
  (preferring an entry a concurrent load may have installed).

---

## Phase 2 — High (correctness / wrong output / data loss) — ✅ DONE

### 2.1 Guard against concurrent pipelines — ✅ DONE
- **Files:** `src-tauri/src/hotkey.rs` (`on_press`/`on_release`),
  `src-tauri/src/pipeline.rs` (`process`), `src-tauri/src/state.rs`.
- **Fix:** Add an `AtomicBool`/flag like `is_processing` on `AppState`. `on_press`
  refuses to start a new recording while a pipeline is in flight (or queues it);
  clear the flag in a guard at the end of `process` (including error/cancel paths).
- **Verify:** rapid press-release-press; exactly one injection, correct order.
- **Implemented:** added `is_processing: AtomicBool` to `AppState`; `on_press`
  returns early while it's set, `on_release` sets it, and `process` clears it via
  a `ProcessingGuard` drop guard covering every return path.

### 2.2 Don't paste into a stale / wrong window — ✅ DONE
- **File:** `src-tauri/src/injection.rs` (`restore_focus`, `inject_paste`,
  `send_ctrl_v`).
- **Fix:** Check the `SetForegroundWindow` return value; if it fails (or
  `IsWindow(hwnd)` is false / hwnd == 0), abort the paste and emit an error event
  rather than firing Ctrl+V into whatever now has focus.
- **Verify:** start dictation, close the target window before release; text must
  not land in an unrelated app — user gets an error instead.
- **Implemented:** `restore_focus` now returns `bool` (false if hwnd == 0, not
  `IsWindow`, or `SetForegroundWindow` fails); `inject_paste` and
  `capture_selection` abort before touching the clipboard when it fails. The
  pipeline surfaces the injection error via `window_mgmt::fail` and records
  `last_transcript` before injecting so the copy-last shortcut still works.

### 2.3 Fix local-LLM zero-output bug — ✅ DONE
- **File:** `src-tauri/src/polish.rs` (`generate`).
- **Fix:** Make the cap relative to the prompt length:
  `let max_new = n_cur + MAX_NEW_TOKENS;` (or clamp generated-token count, not the
  absolute position). Ensure long prompts still produce output.
- **Verify:** feed a >2000-token transcript through the local polisher; non-empty
  result, no silent fallback to raw.
- **Implemented:** `generate` now bounds *new* tokens with
  `let max_new = (n_cur + MAX_NEW_TOKENS).min(4096)` (MAX_NEW_TOKENS = 512), so a
  long prompt no longer makes the generation loop exit immediately.

### 2.4 Replace panicking WAV writer — ✅ DONE
- **File:** `src-tauri/src/audio.rs` (`encode_wav`, ~line 190).
- **Fix:** Replace `.expect("create wav writer")` with `?`/proper error propagation
  so the pipeline returns a real error instead of a caught panic.
- **Verify:** still encodes normally; error path returns `Err`, not a task abort.
- **Implemented:** `encode_wav` returns `anyhow::Result<Vec<u8>>` (`?` on writer
  create / `write_sample` / `finalize`); both callers (`pipeline.rs`,
  `command_mode.rs`) match `Ok(Ok(w))` and fail gracefully otherwise.

### 2.5 Stop swallowing API-key / settings save failures (frontend) — ✅ DONE
- **Files:** `src/components/onboarding/Onboarding.tsx` (`ApiKeyStep.save`,
  `finish`), `src/Hub.tsx` (`SettingsPanel.saveKey`, `clearApiKey`).
- **Fix:** Surface failures. On `storeApiKey`/`updateSettings` rejection, show an
  error and do **not** set the "Saved ✓" / `hasKey: true` / `onboardingComplete`
  state. Only advance on success.
- **Verify:** simulate a rejecting `storeApiKey`; UI shows an error and does not
  claim success; onboarding does not silently re-appear next launch.
- **Implemented:** `ApiKeyStep` and `SettingsPanel` now `try/catch` the store/
  clear calls, render an error line, and only set the saved/`hasKey` state on
  success; onboarding `finish` shows `finishError` and skips `onComplete` if
  `updateSettings` rejects.

### 2.6 Scratchpad: flush debounced save before switch/close — ✅ DONE
- **File:** `src/scratchpad.tsx` (`switchTo` ~131, `closeTab` ~142, debounce
  `persist` ~78).
- **Fix:** Before changing `activeId` or deleting a tab, flush the pending save
  (clear the timer **and** immediately call `saveScratchpadTab` with current
  content). For `closeTab`, cancel the timer so it can't fire on a deleted tab.
- **Verify:** type, switch tab within 500ms, return — text is preserved; type then
  close within 500ms — no orphaned write / no error.
- **Implemented:** a `pending` ref tracks the debounced save; `flushPending`
  writes it immediately + clears the timer, `cancelPending` drops it. `switchTo`
  flushes before changing `activeId`; `closeTab` cancels when the pending save
  targets the closing tab, otherwise flushes.

---

## Phase 3 — Medium

### 3.1 Bound the audio buffer + accurate over-length error
- **Files:** `src-tauri/src/audio.rs` (`ingest_*`), `src-tauri/src/pipeline.rs`,
  error mapping (`friendly_error`).
- **Fix:** Cap recording length (e.g. stop/flush near Groq's 25 MB ≈ ~13 min limit)
  or detect the over-size case and emit a clear "recording too long" message
  instead of "check your connection".

### 3.2 Roll back clipboard on failure
- **File:** `src-tauri/src/injection.rs` (`inject_paste`).
- **Fix:** Use a `scopeguard`/defer so the prior clipboard is restored even on an
  early return or panic between write and restore.

### 3.3 Populate + verify model checksums
- **File:** `src-tauri/src/models.rs` (catalog, verify path ~305).
- **Fix:** Fill `sha256` for each catalog entry; the verification path already
  exists. Reject/redownload on mismatch.

### 3.4 Distinguish keychain "missing" vs "unavailable"
- **File:** `src-tauri/src/secrets.rs` (`has_api_key`, `get_api_key`).
- **Fix:** Only treat the genuine "not found" case as "no key"; surface real OS
  keychain errors instead of mapping everything to `false`.

### 3.5 Frontend optimistic-update rollbacks
- **Files:** `src/Hub.tsx` (`setAutostart` ~633, `clearApiKey` ~393),
  `src/HistoryPage.tsx` (`onDelete` ~43).
- **Fix:** On API rejection, revert the optimistic state (or only apply state after
  the call resolves).

### 3.6 Scratchpad dictation arriving before editor is ready
- **File:** `src/scratchpad.tsx` (`scratchpadInsert` listener ~121).
- **Fix:** Buffer the incoming text (queue) and flush once `editorRef.current` is
  set, instead of silently dropping.

### 3.7 Capture-selection timing
- **File:** `src-tauri/src/injection.rs` (`capture_selection`, ~65).
- **Fix:** Replace the fixed 120ms wait with a short poll loop (re-read clipboard
  until it changes or a max timeout), so heavy apps don't silently fall back.

---

## Phase 4 — Low (polish / hardening)

- `src-tauri/src/config.rs` `save`: don't `unwrap_or_default()` the serialized JSON
  — on serialize error, keep the existing file rather than writing `""`.
- `src-tauri/src/state.rs` `parse_shortcut` / `"Escape".expect()`: handle the parse
  failure gracefully instead of panicking at startup.
- `src/flow-bar.tsx`: add an auto-dismiss/timeout so the bar can't stay stuck in
  "processing" forever if the backend dies; change initial state from `"listening"`
  to an `"idle"` that renders nothing.
- `src/flow-bar.tsx` / `src/LocalModelsPage.tsx`: `.catch()` listener-cleanup
  promises (match the Scratchpad pattern) and avoid `setState` after unmount.
- `src-tauri/src/injection.rs`: consider `SendInput` (with scan codes) over the
  deprecated `keybd_event` for paste/copy, for compatibility with security software.
- Clippy warnings (23): `map_or`→`is_none_or` (`text_processing.rs`),
  deref `Copy` `Shortcut` instead of `.clone()` (`lib.rs:110-118`). Cosmetic.

---

## Suggested execution order
1. Phase 1 (1.1, 1.2) — kills the freeze/hang class. Highest ROI.
2. 2.1, 2.2 — most visible correctness bugs.
3. 2.5, 2.6 — user-facing data-loss / false-success.
4. 2.3, 2.4 — local-model + audio robustness.
5. Phase 3, then Phase 4 as cleanup.

Each phase should land as its own commit with the verification gates run.
