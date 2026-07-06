# Eve Cross-Platform Plan: macOS + Linux Full Parity (without breaking Windows)

## Context

Eve is currently Windows-only in practice: the OS-integration layer (foreground-window
capture, SendInput paste, low-level keyboard/mouse hooks, PlaySoundW, Windows Credential
Manager) exists only behind `#[cfg(windows)]`. Everything else (Tauri 2, cpal audio,
Groq HTTP, the React frontend, tray, autostart, the global-shortcut plugin) is already
cross-platform, and the focus handle is already abstracted as `isize` through
state/pipeline (`state.rs` `foreground_hwnd: Arc<AtomicIsize>`).

Goal: full-parity macOS and Linux support - hotkey (including bare-modifier and
mouse-button triggers), paste injection into the focused app, selection capture /
command mode / transforms, sounds, secure key storage, privacy pause, scratchpad
routing - added as NEW platform backends behind the existing cfg seams. The Windows
build must never regress. Linux supports both X11 and Wayland via runtime detection.

Status: Phase 0 complete (compile-everywhere seam + CI gate). Phase 1 code
complete (macOS core-dictation backend). Phase 2 code complete (macOS parity -
CGEventTap triggers, selection capture, Accessibility permissions UX, focused-app
context). Phase 3 code complete (Linux/X11 - EWMH focus capture/restore, XI2
bare-modifier + GrabButton mouse triggers, `_NET_WM_PID`/`_NET_WM_NAME` context,
X11 paste/selection); all behind the cfg seams, Windows verified green locally,
macOS + Linux compile/E2E finalization pending on CI + target hardware. Phases
4-5 not started. Phases are ordered and each one is independently shippable.

---

## Core design decisions

### 1. Code organization: hybrid - cfg siblings in place, new code in `platform/`

Windows code does NOT move. Existing `#[cfg(windows)]` blocks stay byte-identical
(pattern already proven in `injection.rs` and `command_mode.rs::on_transform`). New
`#[cfg(target_os = "macos")]` / `"linux"` siblings are thin one-line delegations into:

```
src-tauri/src/platform/
├── mod.rs              // cfg re-exports; Frontmost struct; frontmost() facade
├── macos/
│   ├── focus.rs        // frontmost pid capture; activate-by-pid restore
│   ├── input.rs        // CGEventTap: bare-modifier + mouse triggers, self-injection filter
│   ├── keys.rs         // enigo Cmd+V / Cmd+C combos
│   ├── context.rs      // NSWorkspace name/bundle-id + AX focused-window title
│   └── permissions.rs  // AXIsProcessTrusted(+WithOptions prompt)
└── linux/
    ├── mod.rs          // Session::{X11, Wayland} runtime detection (OnceLock)
    ├── x11.rs          // EWMH focus capture/restore, XI2 raw triggers, XGrabButton
    ├── wayland.rs      // ashpd GlobalShortcuts portal; virtual-keyboard injection
    └── context.rs      // _NET_WM_PID -> /proc/<pid>/comm, _NET_WM_NAME (X11 only)
```

`hooks.rs` stays where it is as the de-facto Windows input module.

**The ONE sanctioned Windows refactor (Phase 0, mechanical):** `hotkey.rs::on_press`
lines 104-141 wrap platform-neutral business logic (privacy-pause check,
context-awareness gating, scratchpad routing) inside `#[cfg(windows)]` around just two
Win32 calls (`GetForegroundWindow`, `active_window::resolve`). Extract into a
platform-neutral `capture_focus_and_gate(app, st) -> bool` that calls
`platform::frontmost(app) -> Frontmost { handle: isize, ctx: AppContext,
is_scratchpad: bool }`; the Windows impl of `frontmost` is a verbatim ~12-line lift.
Same extraction for `command_mode.rs::on_press` (lines 41-46) and `on_transform`
(lines 264-267).

**The `isize` handle keeps its name** (`foreground_hwnd`); only its per-OS meaning is
documented: Windows = HWND, macOS = frontmost app pid, Linux/X11 = X window id,
Linux/Wayland = always 0. All plumbing (AtomicIsize, pipeline, `injection::inject`)
unchanged.

### 2. macOS approach

- **Focus**: app-level pid via `NSWorkspace.frontmostApplication` (objc2-app-kit).
  Restore: `NSRunningApplication::runningApplicationWithProcessIdentifier(pid)
  .activate()`, then poll frontmost == pid up to ~500 ms; return `false` -> bail
  before touching the clipboard (same abort contract as Windows `restore_focus`).
- **Paste/copy**: enigo key combos (Meta+V / Meta+C) - already a dependency, uses
  CGEvent underneath, and the same code covers Linux/X11. Direct CGEvent only as
  fallback if combo timing proves unreliable in Phase 1 testing.
- **Bare-modifier + mouse triggers**: CGEventTap (`core-graphics`) on a dedicated
  CFRunLoop thread; `flagsChanged` for modifier up/down, `otherMouseDown/Up` for
  mouse; consume mouse clicks by returning None from an active tap (needs
  Accessibility). Dispatch through the same channel-to-dispatcher pattern as
  `hooks.rs` into `hotkey::on_main_pressed/released`. Re-enable the tap on
  `tapDisabledByTimeout`.
- **Self-injection filter**: process-global `INJECTING: AtomicBool` in `injection.rs`
  set around send-combo calls; taps/listeners drop trigger events while set. Windows
  keeps its existing `LLKHF_INJECTED` check untouched.
- **Permissions**: `NSMicrophoneUsageDescription` via new `src-tauri/Info.plist`
  (Tauri merges it). New commands `check_accessibility` / `request_accessibility`
  wrapping `AXIsProcessTrusted(WithOptions)` (small extern "C" block, no extra
  crate); onboarding step + Settings banner while untrusted. Verify on hardware
  whether newer macOS also wants Input Monitoring.
- **Context**: bundle id + localizedName; focused-window title via AX (best-effort).
  `active_window.rs::classify` gains ADDITIVE macOS bundle-id lists; the Windows
  `.exe` lists stay byte-identical. `default_paused_apps()` extended additively
  (equality matching makes extra entries inert on Windows).
- **Keychain**: keyring `apple-native` feature.

### 3. Linux approach

- **Runtime detection** (`platform/linux/mod.rs`): Wayland iff `WAYLAND_DISPLAY` is
  set (tie-break with `XDG_SESSION_TYPE`); else X11 iff `DISPLAY`. One binary serves
  both.
- **X11** (`x11rb 0.13`, features `xtest, xinput` - pure Rust): capture
  `_NET_ACTIVE_WINDOW`; context via `_NET_WM_PID` -> `/proc/<pid>/comm` +
  `_NET_WM_NAME`; restore via EWMH `_NET_ACTIVE_WINDOW` ClientMessage (NOT
  `XSetInputFocus`, which bypasses the WM) with verify-by-reread. Bare-modifier
  triggers via XI2 raw key events (no grab, no permissions); mouse trigger via
  `XGrabButton` (a grab inherently consumes the click, matching the Windows
  `LRESULT(1)` behavior). Paste = shared enigo Ctrl+V.
- **Wayland**:
  - Global shortcuts: the tauri global-shortcut plugin is a no-op on Wayland. Use
    the `org.freedesktop.portal.GlobalShortcuts` XDG portal via `ashpd` (~0.12,
    tokio feature) - it delivers Activated/Deactivated pairs (exactly push-to-talk's
    press/release), works on KDE 5.25+ and GNOME 45+. Bind
    main/copy/command/scratchpad/transform accelerators through one portal session,
    dispatch into the SAME handler entry points. `lib.rs` wraps existing
    registration in a runtime `if !use_portal` branch (`use_portal` is const false
    off-Linux, so Windows compiles identically).
  - Injection: enigo `wayland` feature (zwp_virtual_keyboard_v1; KDE + wlroots).
    GNOME lacks the protocol: degrade with a one-time Flow Bar error + docs link
    (ydotool opt-in); libei/RemoteDesktop portal is a documented follow-up, not
    parity-critical.
  - Focus capture/restore of foreign windows: impossible on Wayland. `frontmost()`
    returns handle 0 + `AppContext::unknown()`; restore is skipped (focus is still
    on the target app at release time). Documented degradations: privacy pause
    cannot match (info note in Settings), Flow Styles use the default style,
    history app attribution is blank.
  - Bare-modifier/mouse triggers: not expressible via the portal - hide those
    pickers in the UI on Wayland. Esc-cancel does not map to the portal bind-once
    model - document (cancel via toggling the main trigger).
- **Keyring**: `sync-secret-service` feature (GNOME Keyring / KWallet).

### 4. Sound

`#[cfg(not(windows))] play_start_sound` in `sound.rs` generates the same 880 Hz
decaying sine as f32 samples (~12 duplicated lines - deliberate, so the Windows
`generate_start_sound`/`PlaySoundW` path is not edited) and plays via a cpal default
output stream on a short-lived thread. No rodio (cpal is already compiled in). If
output setup latency is audible, pre-build the stream in a `OnceLock` - still no new
crate.

### 5. Frontend / IPC

Add `tauri-plugin-os` + a new `get_platform_info` command (returns platform +
`isWayland`, since JS cannot see the session type) exported from `src/lib/api.ts`.

- `src/Hub.tsx` (~lines 475-490): hide modifier/mouse trigger pickers on Wayland;
  relabel "Alt" -> "Option" on macOS.
- `src/components/ShortcutCapture.tsx:112`: handle `e.metaKey` -> "Cmd"; show
  Cmd/Option glyphs on macOS; verify captured strings still parse via
  `Shortcut::from_str`.
- Per-OS strings: `Hub.tsx:416` (Credential Manager -> Keychain / system keyring),
  `:522`, `:717`, `:816` (paused-apps examples: `.exe` vs bundle id / comm name),
  `:730`; `Onboarding.tsx:547`, `:690` (mic permission help); new macOS
  Accessibility onboarding step (Phase 2); Wayland portal note (Phase 4).
- **IPC contract**: no new `Settings` fields needed (existing fields get per-OS
  semantics), so `config.rs` <-> `api.ts` stays in sync by default. New commands
  (`get_platform_info`, `check_accessibility`, `request_accessibility`) follow the
  4-edit rule: `commands.rs` + `generate_handler!` + `api.ts` +
  `capabilities/default.json`. Off-macOS stubs return true.

### 6. Windows regression guardrails (no test suite exists)

1. **New `.github/workflows/ci.yml` (Phase 0), on every push + PR**: matrix
   {windows-latest, macos-latest, ubuntu-22.04}: `cargo check --all-targets`,
   `cargo clippy -- -D warnings`, `cargo test` (the classify unit tests),
   `npm ci && npm run build`. Today NOTHING gates pushes - this is the
   highest-value guardrail.
2. **Byte-identical policy**: per phase, `git diff <phase-start> --
   src-tauri/src/hooks.rs src-tauri/src/sound.rs` must be empty; no changes inside
   `#[cfg(windows)]` items in `injection.rs` / `window_mgmt.rs` /
   `active_window.rs` (only cfg-widening of `ClipboardGuard` + additive lists).
   Only sanctioned moves: the three Phase 0 extractions.
3. **Manual Windows smoke checklist** per phase: hold-F8 into Notepad (paste), type
   strategy, toggle/hybrid modes, bare Right-Alt trigger, middle-mouse trigger
   consumes click, Esc cancel, command-mode rewrite, transform shortcut, scratchpad
   routing, privacy pause, copy-last, start sound, caret bar, settings persist, API
   key survives restart.
4. **Dependency freeze**: all new crates in `[target.'cfg(target_os = ...)']`
   tables; verify `cargo tree --target x86_64-pc-windows-msvc` diff shows only the
   keyring feature split.

---

## Phase 0 - Compile everywhere + CI gate (no behavior change on any OS)

**Status: DONE.** Windows verified locally (cargo check --all-targets, cargo
clippy -D warnings, cargo test [26 pass], npm run build - all green); the
`#[cfg(windows)]` OS-integration internals are unchanged, only the three
sanctioned seam extractions and the ClipboardGuard cfg-widening. macOS/Linux
compilation is gated on the new CI workflow. What shipped:

- `src-tauri/src/platform/mod.rs` (new): `Frontmost { handle, ctx, is_scratchpad }`
  + `frontmost(app)`. Windows impl is the verbatim `GetForegroundWindow` +
  `active_window::resolve` lift; the non-Windows stub returns handle 0 / unknown
  ctx and answers `is_scratchpad` via the Scratchpad window's `is_focused()`.
- `hotkey.rs`: the Win32 block in `on_press` extracted into the platform-neutral
  `capture_focus_and_gate(app, st) -> bool` (privacy-pause gate + context store +
  Scratchpad routing); the `GetForegroundWindow` import is gone.
- `command_mode.rs`: `on_press` and `on_transform` now call `platform::frontmost`;
  the `GetForegroundWindow` import is gone.
- `injection.rs`: `ClipboardGuard` cfg widened to all platforms (dead-code allow
  off Windows until the mac/Linux paste backends use it).
- `Cargo.toml`: `keyring` split into per-target tables (windows-native /
  apple-native / sync-secret-service); `secrets.rs` unchanged.
- `lib.rs`: `mod platform;`.
- `.github/workflows/ci.yml` (new): push + PR, matrix {windows-latest,
  macos-latest, ubuntu-22.04}, running cargo check/clippy/test + npm build on the
  default (non-native-inference) feature set.
- `scripts/release.mjs`: bundle targets now derived from `process.platform`
  (win32 -> msi,nsis; darwin -> dmg,macos; linux -> deb,rpm,appimage).

### Rust

- `Cargo.toml`: split `keyring` (line 91) into three target tables: windows
  `["windows-native"]` (identical), macos `["apple-native"]`, linux
  `["sync-secret-service"]`. `secrets.rs` unchanged.
- New `src-tauri/src/platform/mod.rs`: `Frontmost` struct + `frontmost(app)`.
  Windows impl = verbatim lift from `hotkey.rs`; mac/linux stubs (handle 0, unknown
  ctx, is_scratchpad via `is_focused()`).
- `hotkey.rs::on_press`: replace the cfg block (lines 104-141) with the
  platform-neutral `capture_focus_and_gate`. Same for `command_mode.rs::on_press` /
  `on_transform`; drop the now-unused `GetForegroundWindow` imports.
- `window_mgmt.rs`: add `#[cfg(not(windows))] scratchpad_hwnd -> None` (or fold
  into `frontmost`).
- `injection.rs`: widen `ClipboardGuard` cfg to all platforms.
- `lib.rs`: `mod platform;`.

### Build / CI

- New `.github/workflows/ci.yml` (ubuntu deps: libwebkit2gtk-4.1-dev libgtk-3-dev
  libayatana-appindicator3-dev librsvg2-dev libasound2-dev libxkbcommon-dev
  libdbus-1-dev patchelf).
- `scripts/release.mjs:85`: replace the hardcoded `["msi","nsis"]` with a
  `process.platform` map: win32 -> msi,nsis; darwin -> dmg,macos; linux ->
  deb,rpm,appimage.

### Verify

CI green on all three runners (this is the phase gate); full Windows smoke
checklist; diff audit. On Mac/Linux hardware: app launches, Hub renders, settings
persist, Groq key stores/retrieves (Keychain / Secret Service), file transcription
works - i.e. everything not gated on the platform seams.

### Risks

Clippy `-D warnings` may flag pre-existing warnings on the new cfg paths - fix here
while the surface is small. sync-secret-service needs libdbus (listed in CI deps,
present on desktop distros).

---

## Phase 1 - macOS core dictation (hotkey -> record -> transcribe -> paste -> sound)

**Status: code complete.** Windows verified locally (`cargo check --all-targets`,
`cargo clippy --all-targets -D warnings`, `npm run build` - all green); the
`#[cfg(windows)]` OS-integration internals (`hooks.rs`, `sound.rs`'s Windows tone
path, `injection.rs`'s Windows `SendInput` path) are untouched. The macOS code is
gated behind `#[cfg(target_os = "macos")]` and finalizes on CI (macos-latest) +
Mac hardware per the plan's verification model - in particular the exact
objc2-app-kit method safe/unsafe split and the `NSApplicationActivationOptions`
const name (hedged with function-level `#[allow(unused_unsafe)]`). What shipped:

- `platform/macos/focus.rs` (new): `frontmost_pid()` (NSWorkspace) +
  `restore_focus(pid)` (NSRunningApplication `activateWithOptions:` + poll-until-
  frontmost, ~500 ms cap - same abort contract as Windows `restore_focus`).
- `platform/macos/keys.rs` (new): enigo Cmd+V wrapped in the `INJECTING` guard.
- `platform/macos/mod.rs` (new) + `platform/mod.rs`: macOS `frontmost` arm (handle
  = frontmost pid, ctx unknown until Phase 2 AX context), the Linux/other stub
  split out to `#[cfg(not(any(windows, target_os = "macos")))]`, shared
  `scratchpad_is_focused` helper, and `is_wayland()` (false off Linux).
- `injection.rs`: `#[cfg(not(windows))]` `injecting` module (process-global flag +
  RAII `Guard` - the non-Windows analogue of Windows' injected-event flag; used by
  macOS keys now, Linux listeners later); real `#[cfg(target_os = "macos")]`
  `inject_paste` (restore -> guard -> write -> PRE 90 ms -> Cmd+V -> SETTLE 120 ms);
  the pre-existing non-Windows `inject_paste` fallback narrowed to non-macOS.
- `sound.rs`: `#[cfg(not(windows))]` `play_start_sound` synthesizes the same
  880 Hz decaying-sine chirp as f32 samples and plays it through a cpal default
  output stream on a short-lived thread (F32/I16/U16 formats). No new crate; the
  Windows `PlaySoundW` path is byte-identical.
- New `src-tauri/Info.plist` with `NSMicrophoneUsageDescription` (Tauri merges it);
  `icons/icon.icns` already present.
- `commands.rs` + `lib.rs`: new `get_platform_info` command (returns
  `{ os, isWayland }`); registered in `generate_handler!`.
- `Cargo.toml`: macOS deps `objc2 0.6`, `objc2-foundation 0.3`, `objc2-app-kit 0.3`
  (features `NSWorkspace`, `NSRunningApplication`) in the existing macOS target
  table.
- Frontend: `api.ts` `PlatformInfo` type + `getPlatformInfo` wrapper; new
  `src/lib/platform.ts` (`isMac`, `labelAccelerator` -> ⌘/⌥/⇧/⌃ glyphs);
  `ShortcutCapture.tsx` emits `Cmd` for the Meta key on macOS and renders the
  glyphs. Deviation from the plan: `get_platform_info` (custom command) supersedes
  `tauri-plugin-os`, so the plugin was not added - one fewer capability/permission
  surface for the same result. Broad per-OS string relabels across `Hub.tsx` /
  `Onboarding.tsx` are deferred to the Mac-hardware pass (they're cosmetic and, per
  the pixel-perfection bar, want a real macOS render to verify; the Accessibility
  onboarding step is Phase 2 regardless).

### Verify (Mac hardware)

F8 hold-to-dictate pastes into TextEdit/Safari/VS Code; type strategy;
toggle/hybrid; clipboard restored after paste; start sound; scratchpad routing;
Keychain prompt behavior on first key read. CI green x3; Windows smoke re-run;
hooks.rs/sound.rs diff empty.

### Risks

enigo Cmd+V timing vs activation - mitigated by the poll-until-frontmost gate before
pasting; fallback to direct CGEvent posting isolated inside `keys.rs`. `activate()`
deprecation churn across macOS 14/15 - pin behavior on the actual hardware.

---

## Phase 2 - macOS parity (triggers, selection/command mode, permissions UX, context)

**Status: code complete.** Windows verified locally (`cargo check --all-targets`,
`cargo clippy --all-targets -D warnings`, `cargo test` [27 pass, incl. the new
macOS bundle-id classify test], `npm run build` - all green); `hooks.rs` and
`sound.rs` diffs are empty and no `#[cfg(windows)]` internals changed. The macOS
code is gated behind `#[cfg(target_os = "macos")]` and finalizes on CI
(macos-latest) + Mac hardware per the plan's verification model - in particular
the core-graphics CGEventTap callback ABI, the mouse-consume mechanism (re-typing
the event to `Null`), and the AX focused-window-title FFI. What shipped:

- `platform/macos/input.rs` (new; core-graphics 0.24, core-foundation 0.10):
  CGEventTap on a dedicated CFRunLoop thread. `update_triggers(&Settings)` mirrors
  the `hooks.rs` atomics (modifier keycode + device-flag mask for left/right
  disambiguation; mouse button number). Lean callback -> channel -> dispatcher
  thread running `hotkey::on_main_pressed/released`; middle/X1/X2 consumed, bare
  modifiers passed through; `injection::injecting` self-injection filter; re-enable
  on `TapDisabledByTimeout`/`ByUserInput` via a leaked, thread-owned tap pointer.
  The tap thread waits for Accessibility trust before creating the tap.
- `platform/macos/permissions.rs` (new): `is_trusted` / `prompt_trust` over a
  small `AXIsProcessTrusted(WithOptions)` extern block (no extra crate).
  `check_accessibility` / `request_accessibility` commands (registered in
  `generate_handler!`; return `true` off macOS).
- `platform/macos/context.rs` (new): `resolve(pid)` -> bundle id (NSRunningApplication)
  + best-effort AX focused-window title, mapped through `classify`. Wired into
  `platform::frontmost` (macOS arm now resolves real context instead of unknown).
- `injection.rs`: macOS `capture_selection` (restore -> clear -> Cmd+C -> poll
  <=~600 ms); the not-any(windows,macos) stub narrowed to return `None`.
  `keys.rs` gained `copy()` (Cmd+C under the INJECTING guard).
- `active_window.rs`: additive macOS bundle-id lists in `classify` (EMAIL /
  WORK_MSG / PERSONAL_MSG / CODE / BROWSERS) - lowercased, inert on Windows - plus
  a `classifies_macos_bundle_ids` unit test.
- `lib.rs` setup + `commands.rs::update_settings`: `#[cfg(target_os = "macos")]`
  siblings next to the Windows hooks blocks (`input::update_triggers` + `init`).
- `Cargo.toml`: `core-graphics 0.24` + `core-foundation 0.10` in the macOS target
  table.
- Frontend: `api.ts` `checkAccessibility` / `requestAccessibility` wrappers; a
  macOS-only Accessibility onboarding step (polls trust, opens the prompt) appended
  after Cleanup so existing step indices are unchanged; the mic-denied help string
  is now OS-aware. Deviation from the plan: the persistent Settings/Hub untrusted
  banner and the remaining per-OS Hub relabels are deferred to the Mac-hardware
  pass (they want a real macOS render per the pixel-perfection bar; the onboarding
  step already covers the untrusted case at first run).

### Verify (Mac hardware)

Right-Option bare trigger with key-up; middle-mouse trigger consumed; no
self-retrigger when pasting with Left-Cmd bound; command mode rewrites a selection
in Notes; transforms; privacy pause by bundle id; Flow Styles detect VS Code.
Windows smoke; CI x3.

### Risks

Active event taps get disabled by the OS if the callback stalls - keep it as lean
as the `hooks.rs` procs (atomics + channel push, re-enable on
`tapDisabledByTimeout`). Input Monitoring possibly required on newer macOS - test
early in the phase, add to the permissions UX if needed.

---

## Phase 3 - Linux X11

**Status: code complete.** Windows verified locally (`cargo check --all-targets`,
`cargo clippy --all-targets -D warnings`, `cargo test` [28 pass, incl. the new
Linux comm-name classify test], `npm run build` - all green); `hooks.rs` and
`sound.rs` diffs are empty and no `#[cfg(windows)]` internals changed. The Linux
code is gated behind `#[cfg(target_os = "linux")]` / `session() == X11` and
finalizes on CI (ubuntu-22.04) + Linux hardware per the plan's verification model.
The x11rb 0.13 request/event ABI was checked against the crate source (0.13.2):
`xinput::EventMask.mask` is `Vec<XIEventMask>` (not raw u32s), `grab_button` /
`send_event` take an `EventMask` (not a u16), `ButtonIndex: From<u8>` (so thumb
buttons 8/9 grab cleanly), and `Event::XinputRawKeyPress`/`RawKeyReleaseEvent`
carry the keycode in `.detail`. What shipped:

- `platform/linux/mod.rs` (new): `Session::{X11, Wayland, Unknown}` runtime
  detection (Wayland iff `WAYLAND_DISPLAY`/`XDG_SESSION_TYPE=wayland`, else X11 iff
  `DISPLAY`), cached in a `OnceLock`. `platform::is_wayland()` now routes through
  it.
- `platform/linux/x11.rs` (new; x11rb 0.13, features xinput + xtest): EWMH focus
  capture (`_NET_ACTIVE_WINDOW`) + restore (root `_NET_ACTIVE_WINDOW` client
  message, source indication 2, verify-by-reread ~500 ms - same abort contract as
  the other backends). Triggers on a dedicated thread with its own connection:
  bare-modifier via XI2 **raw** key events (observed on the root without a grab,
  so the key still reaches the app; keycode -> keysym via the cached keyboard
  mapping, with ISO_Level3_Shift accepted for Right Alt), mouse via a passive
  `GrabButton` (which inherently consumes the click). Same channel-to-dispatcher
  pattern as `hooks.rs`; `injection::injecting` drops our own synthetic Ctrl+V/C.
  A short poll loop reconciles the button grab when settings change (grabs are
  connection-scoped).
- `platform/linux/context.rs` (new): `resolve(pid, title)` -> `/proc/<pid>/comm`
  process name + `classify`; no X11 dependency (x11.rs feeds it the raw signals).
- `platform/linux/keys.rs` (new): enigo Ctrl+V / Ctrl+C under the `INJECTING`
  guard (the Linux sibling of `macos::keys`).
- `platform/mod.rs`: linux `frontmost` arm (X11 = real window id + context;
  Wayland = handle 0 / unknown); the fallback stub narrowed to
  `not(any(windows, macos, linux))`.
- `injection.rs`: `#[cfg(target_os = "linux")]` `inject_paste` / `capture_selection`
  (X11 real via EWMH restore + Ctrl+V/C; Wayland falls back to typing / returns
  `None` until Phase 4); the catch-all stubs narrowed to exclude linux.
- `active_window.rs`: additive Linux comm-name lists in `classify` (EMAIL /
  WORK_MSG / PERSONAL_MSG / CODE / BROWSERS, truncated to 15 chars where relevant,
  inert on Windows/macOS) + a `classifies_linux_comm_names` unit test.
  `default_paused_apps()` extended additively with Linux password-manager comm
  names.
- `lib.rs` setup + `commands.rs::update_settings`: `#[cfg(target_os = "linux")]`
  siblings next to the Windows/macOS trigger blocks (init only on X11; update
  publishes to the atomics, inert on Wayland).
- `Cargo.toml`: `x11rb 0.13` (features `xinput`, `xtest`) in the linux target
  table.
- Frontend: `platform.ts` gains `isLinux` + `pausedAppExample()` (Windows exe /
  macOS bundle id / Linux comm name); Hub's auto-pause example text + input
  placeholder are now OS-aware.

### Verify (Linux box, X11 session)

Dictation into gedit/Firefox/VS Code with paste + clipboard restore; focus restore
after alt-tab during processing; bare-modifier + middle-mouse (click consumed);
command mode + transforms; privacy pause on keepassxc; Secret Service key across
reboot. Clippy 0 warnings x3; Windows + macOS smoke.

### Risks

WM variance in EWMH activation (test GNOME-Xorg + KDE-Xorg, XFCE if handy).
Left/right modifier distinction needs keycode<->keysym resolution via the keyboard
mapping at trigger-config time.

---

## Phase 4 - Linux Wayland

- `platform/linux/wayland.rs` (ashpd ~0.12, tokio): GlobalShortcuts portal session;
  translate accelerator strings to portal trigger descriptions;
  Activated/Deactivated -> existing handler entry points. `lib.rs` `use_portal`
  branch; Wayland branches in `commands.rs::swap_global_shortcut` +
  `command_mode::register_transform_shortcuts`.
- Injection: enigo `wayland` feature where the protocol exists; graceful error +
  docs link on GNOME (ydotool opt-in); libei follow-up documented.
- UI degradations wired: hide trigger pickers, privacy-pause info note, Esc-cancel
  note in shortcut help.

### Verify (same Linux box, Wayland session)

Portal permission dialog on first run (KDE, GNOME if available); hold-to-dictate
via portal Activated/Deactivated; paste lands in the focused app (KDE); type
fallback messaging on GNOME; the SAME binary detects both session types correctly;
X11 behavior unregressed (re-run Phase 3 checks).

### Risks

Portal Deactivated semantics differ per desktop - hybrid/toggle modes do not depend
on release; document hold-mode caveats per desktop. Accelerator -> portal-trigger
mapping is lossy - keep a small translation table and surface bind failures in
Settings.

---

## Phase 5 - Packaging, release matrix, distribution

- `.github/workflows/release.yml` -> matrix: windows-latest (unchanged, incl. the
  libclang step, `--bundles nsis,msi,updater`); macos-latest
  (universal-apple-darwin, `--bundles app,dmg,updater`; whisper.cpp builds with
  Metal via Xcode CLT - no libclang step); ubuntu-22.04 (ci.yml system deps,
  `--bundles deb,rpm,appimage,updater`). tauri-action merges all platforms into
  one draft release + a single multi-platform latest.json (the existing updater
  endpoint keeps working). Keep `--features local-whisper` on all three.
- macOS signing/notarization: optional follow-up (APPLE_* secrets in tauri-action);
  until then document right-click-Open / `xattr -d com.apple.quarantine`. Linux:
  note that the tauri updater only updates AppImage installs.
- Docs: per-platform permission matrix (mic, Accessibility, portal) + known
  Wayland degradations.

### Verify

Tag a prerelease; all three jobs upload; install + auto-update E2E on each OS
(dummy version bump); `release.mjs` collects per-OS artifacts into
`build/<version>/` correctly.

---

## Crate additions (all in per-target tables; pin exact versions at implementation)

| Target | Crate | Features | Purpose |
|---|---|---|---|
| macos | objc2 0.6 / objc2-foundation / objc2-app-kit 0.3 | NSWorkspace, NSRunningApplication | focus, context |
| macos | core-graphics 0.24, core-foundation 0.10 | - | CGEventTap triggers |
| macos | keyring 3 | apple-native | key storage |
| linux | x11rb 0.13 | xtest, xinput | focus/EWMH, triggers, grabs |
| linux | ashpd ~0.12 | tokio | Wayland GlobalShortcuts portal |
| linux | keyring 3 | sync-secret-service | key storage |
| all | enigo 0.3 (existing) | + wayland on linux | injection |
| all | tauri-plugin-os | - | frontend platform detection |

## Critical files

- `src-tauri/src/hotkey.rs` - the Phase 0 seam extraction (the one sanctioned
  Windows refactor)
- `src-tauri/src/injection.rs` - paste/selection seams for every platform
- `src-tauri/src/lib.rs` - trigger init, portal-vs-plugin branch, new commands
- `src-tauri/src/platform/**` - all new platform code
- `src-tauri/Cargo.toml` - keyring target tables + platform crates
- `.github/workflows/ci.yml` (new) + `release.yml` - the regression gate
- `scripts/release.mjs` - platform-aware artifact collection
- `src/lib/api.ts`, `src/Hub.tsx`, `src/components/ShortcutCapture.tsx`,
  `src/Onboarding.tsx` - frontend platform awareness

## Verification summary

Every phase gates on: CI green on all three runners (cargo check + clippy
`-D warnings` + cargo test + `npm run build`), the manual Windows smoke checklist,
a diff audit proving `#[cfg(windows)]` internals are untouched, and hands-on E2E
dictation on the target platform hardware for that phase.
