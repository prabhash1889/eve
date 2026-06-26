import type { CleanupLevel } from "./api";

// Shared UI option lists used by both the Hub Settings panel and the
// first-run Onboarding flow (Phase 10).

export const SHORTCUT_CHOICES = ["F8", "F9", "F10", "CmdOrCtrl+Shift+Space", "Alt+Q"];

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
