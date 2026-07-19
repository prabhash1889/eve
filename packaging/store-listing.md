# Microsoft Store listing - Eve (dictation-only edition)

Copy for the Partner Center submission. Accurate to the **Store build**, which is
offline/on-device dictation only (no cloud, no AI-polish/Command Mode/Styles).
Note: the marketing site describes the full edition - keep the Store listing to
what the Store build actually does, or reviewers will flag the mismatch.

---

## Short description (one line)

Hold a key, speak, and Eve types it into any app - fully offline, on-device, no account.

## Description (long)

Eve is a system-wide voice dictation app for Windows. Hold your push-to-talk
hotkey, speak, and release - Eve transcribes your voice on-device and types the
text straight into whatever app you're using: your editor, browser, chat, email,
anywhere.

Everything runs locally. The speech model (NVIDIA Parakeet) is built in, so there
is nothing to download, no API key, no account, and no internet required. Your
audio never leaves your computer.

Features:
- Push-to-talk dictation into any Windows application
- Fully offline, on-device transcription - no cloud, no sign-in
- Automatic spacing, punctuation, and capitalization
- Local, searchable dictation history stored only on your PC
- Floating scratchpad and a copy-last-transcript shortcut

English speech recognition.

## Search terms
voice dictation, speech to text, offline dictation, voice typing, transcription,
push to talk, on-device, private

## Category
Productivity

---

## Privacy policy (host this at a public URL, e.g. your site /eve/privacy)

  **Eve Privacy Policy**

  Eve is an offline, on-device voice dictation application. We designed it so your
  data stays on your computer.

  **What Eve accesses**
  - **Microphone:** Eve records audio only while you hold the push-to-talk hotkey.
    Audio is transcribed to text locally on your device.
  - **Keyboard/mouse:** Eve uses a system-wide input hook solely to detect your
    configured push-to-talk trigger, and it types the transcribed text into the
    app you're using. Eve does not log, store, or transmit your keystrokes.

  **What Eve stores**
  - Your dictation transcripts and settings are stored locally on your PC (in a
    local database in your user profile). You can clear history at any time.
  - Audio clips are stored locally only if you enable that in settings, and are
    pruned per your chosen retention setting.

  **What Eve does NOT do**
  - Eve does not send your audio, transcripts, or any personal data to us or any
    third party. Transcription runs entirely on your device.
  - Eve has no account, no sign-in, and no analytics/telemetry.

  **Contact**
  [your support email / contact link]

  *Last updated: [date]*

---

## runFullTrust justification (paste into Partner Center when prompted)

Eve is a full-trust desktop dictation utility. It requires the runFullTrust
capability for two OS integrations that are not possible in an AppContainer:

1. **System-wide push-to-talk trigger.** Eve installs a low-level keyboard/mouse
   hook so the user can start/stop recording with a global hotkey from any
   application. The hook only detects the user's configured trigger key - it does
   not record, store, or transmit keystroke content.

2. **Inserting the transcription into the focused app.** After transcribing, Eve
   uses input injection (SendInput) to type/paste the resulting text into
   whatever application the user is working in.

Both are core to a dictation tool and require full desktop (Win32) execution. No
keystroke content is captured or sent anywhere; all speech processing is on-device.

---

## Pre-submission checklist
- [ ] Reserve app name in Partner Center -> get Identity Name / Publisher / Publisher display name
- [ ] Repack: `MSIX_IDENTITY_NAME=... MSIX_PUBLISHER=CN=... MSIX_PUBLISHER_DISPLAY="..." npm run build:msix`
- [ ] Host the privacy policy at a public URL; put that URL in the submission
- [ ] Paste the runFullTrust justification
- [ ] Upload the (unsigned) .msix; add screenshots, description, category, age rating
- [ ] Submit and respond to certification
