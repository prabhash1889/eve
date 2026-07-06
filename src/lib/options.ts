import type { ActivationMode, CleanupLevel } from "./api";

// Shared UI option lists used by both the Hub Settings panel and the
// first-run Onboarding flow (Phase 10).

export const SHORTCUT_CHOICES = ["F8", "F9", "F10", "CmdOrCtrl+Shift+Space", "Alt+Q"];

// Parity A1: how the record trigger starts/stops a dictation.
export const ACTIVATION_MODES: { value: ActivationMode; label: string; hint: string }[] = [
  {
    value: "hold",
    label: "Hold to talk",
    hint: "Hold the trigger while speaking; release to insert.",
  },
  {
    value: "hybrid",
    label: "Hybrid",
    hint: "Quick tap records hands-free (tap again to stop); holding still works like push-to-talk.",
  },
  {
    value: "toggle",
    label: "Toggle",
    hint: "Press once to start recording, press again to stop.",
  },
];

// Parity A3: bare modifier keys usable as an extra record trigger.
export const MODIFIER_TRIGGERS: { value: string; label: string }[] = [
  { value: "", label: "None" },
  { value: "right_alt", label: "Right Alt" },
  { value: "left_alt", label: "Left Alt" },
  { value: "right_ctrl", label: "Right Ctrl" },
  { value: "left_ctrl", label: "Left Ctrl" },
  { value: "right_shift", label: "Right Shift" },
  { value: "left_shift", label: "Left Shift" },
];

// Parity A4: mouse buttons usable as an extra record trigger. The bound
// button's normal click is consumed while assigned.
export const MOUSE_TRIGGERS: { value: string; label: string }[] = [
  { value: "", label: "None" },
  { value: "middle", label: "Middle button" },
  { value: "x1", label: "Back button (X1)" },
  { value: "x2", label: "Forward button (X2)" },
];

export const LANGUAGES: { code: string; label: string }[] = [
  { code: "auto", label: "Auto-detect" },
  { code: "en", label: "English" },
  { code: "hi", label: "Hindi" },
  { code: "es", label: "Spanish" },
  { code: "fr", label: "French" },
  { code: "de", label: "German" },
  { code: "it", label: "Italian" },
  { code: "pt", label: "Portuguese" },
  { code: "ja", label: "Japanese" },
  { code: "zh", label: "Chinese" },
];

export const CLEANUP: { value: CleanupLevel; label: string; hint: string }[] = [
  { value: "none", label: "None", hint: "Raw transcript, no AI edits" },
  { value: "light", label: "Light", hint: "Fix capitalization/punctuation, drop stray fillers" },
  { value: "medium", label: "Medium", hint: "Remove fillers, fix grammar, resolve self-corrections" },
  { value: "high", label: "High", hint: "Rewrite into clean prose; format spoken lists" },
];
