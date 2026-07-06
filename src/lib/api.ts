import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { listen, type UnlistenFn, type EventCallback } from "@tauri-apps/api/event";

// ---------------------------------------------------------------------------
// Shared types (mirror the Rust structs in src-tauri/src/config.rs)
// ---------------------------------------------------------------------------

export type CleanupLevel = "none" | "light" | "medium" | "high";

export type AudioStoragePolicy = "store" | "delete24h" | "never";

/** Which backend runs an AI step: cloud Groq or on-device local model. */
export type ModelBackend = "groq" | "local";

/** Local transcription performance profile (optimization Phase 4). */
export type LocalProfile = "fast" | "balanced" | "accurate";

/** Parity A1: how the main trigger starts/stops recording. */
export type ActivationMode = "hold" | "toggle" | "hybrid";

export interface Settings {
  shortcut: string;
  language: string; // "auto" or an ISO-639-1 code
  cleanupLevel: CleanupLevel;
  injectStrategy: "paste" | "type";
  inputDevice: string; // capture device name ("" = system default)
  copyShortcut: string;
  commandShortcut: string; // Phase 7: Command Mode push-to-talk shortcut
  scratchpadShortcut: string; // Phase 9: opens the floating Scratchpad window
  bubbleScale: number; // Flow Bar size multiplier (1.0 = default)
  bubbleOpacity: number; // Flow Bar opacity (0–1)
  audioStoragePolicy: AudioStoragePolicy; // retention of saved audio (Phase 3)
  audioRetentionHours: number; // window for "delete24h"
  transcriptionBackend: ModelBackend; // local models: speech→text backend
  polishBackend: ModelBackend; // local models: polish backend
  localWhisperModel: string; // catalog id of the selected local Whisper model
  localLlmModel: string; // catalog id of the selected local polish LLM
  localTranscriptionProfile: LocalProfile; // optimization: speed/quality profile
  localWhisperThreads: number | null; // optimization: explicit whisper.cpp threads (null = auto)
  localVadEnabled: boolean; // optimization: trim silence before local inference
  localBeamSearchEnabled: boolean; // optimization: beam search for balanced/accurate local profiles
  localCorrectnessRescue: boolean; // quality-first local mode for difficult clips
  localPrewarmEnabled: boolean; // optimization: prewarm local model on switch/select
  debugTiming: boolean; // Phase 5: print a detailed per-stage latency breakdown per session
  vibeCoding: boolean; // Phase 8: wrap spoken "backtick X backtick" in code editors
  languages: string[]; // Phase 10: enabled languages (["auto"] = auto-detect)
  pausedApps: string[]; // Phase 10: process names where dictation is suppressed
  contextAwareness: boolean; // Phase 10: resolve/store focused-app context
  onboardingComplete: boolean; // Phase 10: first-run flow finished
  launchAtStartup: boolean; // Phase 11: start Eve at OS login
  activationMode: ActivationMode; // Parity A1: hold / toggle / hybrid
  modifierTrigger: string; // Parity A3: bare-modifier trigger id ("" = none)
  mouseTrigger: string; // Parity A4: mouse-button trigger id ("" = none)
  translateToEnglish: boolean; // Parity D: Translate all audio to English
  whisperPrompt: string; // Parity D: Initial prompt passed to the Whisper transcriber
  soundOnStart: boolean; // Parity E2: Play a sound when recording starts
  cjkAutocorrect: boolean; // Parity E5: Automatically correct spacing in CJK languages
  barPosition: "fixed" | "near_caret"; // Parity E6: Flow Bar window position
}

export const DEFAULT_SETTINGS: Settings = {
  shortcut: "F8",
  language: "auto",
  cleanupLevel: "none",
  injectStrategy: "paste",
  inputDevice: "",
  copyShortcut: "CmdOrCtrl+Shift+C",
  commandShortcut: "CmdOrCtrl+Shift+Alt+Space",
  scratchpadShortcut: "CmdOrCtrl+Shift+S",
  bubbleScale: 1.0,
  bubbleOpacity: 1.0,
  audioStoragePolicy: "delete24h",
  audioRetentionHours: 24,
  transcriptionBackend: "groq",
  polishBackend: "groq",
  localWhisperModel: "",
  localLlmModel: "",
  localTranscriptionProfile: "balanced",
  localWhisperThreads: null,
  localVadEnabled: true,
  localBeamSearchEnabled: true,
  localCorrectnessRescue: false,
  localPrewarmEnabled: true,
  debugTiming: false,
  vibeCoding: true,
  languages: ["auto"],
  pausedApps: ["1password.exe", "keepass.exe", "keepassxc.exe", "bitwarden.exe"],
  contextAwareness: true,
  onboardingComplete: false,
  launchAtStartup: false,
  activationMode: "hold",
  modifierTrigger: "",
  mouseTrigger: "",
  translateToEnglish: false,
  whisperPrompt: "",
  soundOnStart: false,
  cjkAutocorrect: true,
  barPosition: "fixed",
};

// ---------------------------------------------------------------------------
// History (Phase 3) — mirrors src-tauri/src/db/queries.rs
// ---------------------------------------------------------------------------

export interface Transcript {
  id: number;
  createdAt: number; // unix epoch ms (UTC)
  rawText: string;
  polishedText: string;
  cleanupLevel: string;
  language: string;
  audioPath: string | null;
  appProcess: string;
  appTitle: string;
  appCategory: string;
  wordCount: number;
  durationMs: number;
  wasPolished: boolean;
  deletedAt: number | null;
  sourceFile: string | null; // Phase C: source audio path for file transcriptions
}

export interface HistoryPage {
  items: Transcript[];
  total: number;
  page: number;
  perPage: number;
}

export interface AppUsage {
  category: string;
  sessions: number;
  words: number;
}

export interface DailyPoint {
  date: string; // YYYY-MM-DD (local)
  words: number;
  sessions: number;
}

export interface Stats {
  totalWords: number;
  totalSessions: number;
  totalMs: number;
  corrections: number; // Phase 8: summed per-session cleanup edits
  appUsage: AppUsage[];
  daily: DailyPoint[];
  since: number;
}

export type StatsRange = "day" | "week" | "month" | "all";

// ---------------------------------------------------------------------------
// Dictionary (Phase 4) — mirrors src-tauri/src/db/dictionary.rs
// ---------------------------------------------------------------------------

export interface DictionaryEntry {
  id: number;
  word: string;
  replacement: string | null; // null = boost-only (no substitution)
  isStarred: boolean;
  source: string; // "user" | "auto" | "import"
  learnedCount: number;
  createdAt: number;
  updatedAt: number;
}

// ---------------------------------------------------------------------------
// Snippets (Phase 5) — mirrors src-tauri/src/db/snippets.rs
// ---------------------------------------------------------------------------

export interface Snippet {
  id: number;
  triggerPhrase: string;
  expansion: string;
  isActive: boolean;
  createdAt: number;
  updatedAt: number;
}

// ---------------------------------------------------------------------------
// Flow Styles (Phase 6) — mirrors src-tauri/src/db/flow_styles.rs
// ---------------------------------------------------------------------------

/** Focused-app categories (mirror context::active_window::AppCategory). */
export type AppCategory = "email" | "workmsg" | "personalmsg" | "code" | "other";

/** Built-in voices a Flow Style can apply. */
export type FlowTone = "casual" | "formal" | "excited" | "very_casual";

export interface FlowStyle {
  id: number;
  name: string;
  appCategory: AppCategory;
  tone: FlowTone;
  systemPrompt: string;
  writingSample: string;
  isActive: boolean;
  createdAt: number;
  updatedAt: number;
}

// ---------------------------------------------------------------------------
// Transforms (Phase 7) — mirrors src-tauri/src/db/transforms.rs
// ---------------------------------------------------------------------------

export interface Transform {
  id: number;
  name: string;
  systemPrompt: string;
  shortcut: string; // optional global accelerator ("" = none)
  autoApply: boolean; // run after every dictation
  appCategory: string; // scope auto-apply to a category ("" = all apps)
  isActive: boolean;
  createdAt: number;
  updatedAt: number;
}

/** Convert a stored audio file path into an asset:// URL the `<audio>` tag can load. */
export const audioSrc = (path: string): string => convertFileSrc(path);

// ---------------------------------------------------------------------------
// Scratchpad (Phase 9) — mirrors src-tauri/src/db/scratchpad.rs
// ---------------------------------------------------------------------------

export interface ScratchpadTab {
  id: number;
  title: string;
  content: string; // editor HTML
  position: number;
  createdAt: number;
  updatedAt: number;
}

// ---------------------------------------------------------------------------
// Local models — mirrors src-tauri/src/models.rs
// ---------------------------------------------------------------------------

export type ModelKind = "whisper" | "llm";

export interface ModelStatus {
  id: string;
  kind: ModelKind;
  name: string;
  sizeBytes: number;
  installed: boolean;
  downloading: boolean;
  active: boolean; // selected in Settings for its kind
}

export interface ModelProgressPayload {
  id: string;
  downloaded: number;
  total: number;
}

export interface ModelStatusPayload {
  id: string;
  message: string | null; // present only on model://error
}

/** Phase 2: readiness of the selected local Whisper model (mirrors WhisperStatus). */
export interface WhisperStatus {
  model: string; // selected catalog id ("" = none)
  loading: boolean; // a cold load is in flight
  ready: boolean; // loaded + cached, ready for instant inference
  lastLoadMs: number | null; // wall-clock cost of the last cold load
  lastTranscribeMs: number | null; // wall-clock cost of the last local inference
  backend: string; // "whisper.cpp CPU" | "whisper.cpp CUDA" | unavailable
}

export interface TranscriptionBenchmark {
  mode: "dictation" | "command" | string;
  model: string;
  profile: LocalProfile | string;
  backend: string;
  clipDurationMs: number;
  transcribeMs: number;
  wordsProduced: number;
  vadTrimmed: boolean;
}

// ---------------------------------------------------------------------------
// Pipeline events (emitted by Rust to the Flow Bar window)
// ---------------------------------------------------------------------------

export const EVT = {
  start: "session://start",
  processing: "session://processing",
  amplitude: "session://amplitude",
  done: "session://done",
  error: "session://error",
  cancel: "session://cancel",
  transcriptRaw: "session://transcript-raw",
  transcriptPolished: "session://transcript-polished",
  copied: "session://copied",
  ready: "session://ready",
  // Phase 1 (optimization): coarse processing-stage label for the Flow Bar.
  stage: "session://stage",
  // Phase 10: auto-pause suppressed recording in a sensitive app.
  paused: "session://paused",
  // Parity A1: capture buffer nearing the 15-minute ceiling (toggle mode).
  limit: "session://limit",
  // Phase 11: tray "Check for updates" → Hub runs the check.
  checkUpdate: "app://check-update",
  // Phase C: file-transcription queue lifecycle (emitted to the Hub window).
  queueProgress: "queue://progress",
  queueDone: "queue://done",
  queueError: "queue://error",
  // Phase 9: dictated text routed into the focused Scratchpad editor.
  scratchpadInsert: "scratchpad://insert",
  // Local-model download lifecycle (emitted to the Hub window).
  modelProgress: "model://progress",
  modelDone: "model://done",
  modelError: "model://error",
} as const;

export interface DonePayload {
  text: string;
}
export interface ErrorPayload {
  message: string;
}
export interface TranscriptPayload {
  text: string;
}
export interface StagePayload {
  label: string; // "Transcribing" | "Polishing" | "Inserting"
}
export interface StartPayload {
  bubbleScale: number;
  bubbleOpacity: number;
  mode: "dictation" | "command"; // Phase 7: Command Mode tints the Flow Bar
  toggleHint: boolean; // Parity A1: toggle/hybrid mode - hint that a tap stops
}

// Phase C: file-transcription queue --------------------------------------------

/** A file accepted into the transcription queue (mirrors QueueItem in Rust). */
export interface QueueItem {
  id: number;
  fileName: string;
}

export interface QueueProgressPayload {
  id: number;
  fileName: string;
  stage: string; // "Queued" | "Decoding" | "Transcribing" | "Polishing"
}

export interface QueueDonePayload {
  id: number;
  fileName: string;
  transcriptId: number;
  text: string;
}

export interface QueueErrorPayload {
  id: number;
  fileName: string;
  message: string;
}

export function on<T>(event: string, cb: EventCallback<T>): Promise<UnlistenFn> {
  return listen<T>(event, cb);
}

/**
 * Phase 10: derive the single Whisper `language` hint from the enabled list.
 * Exactly one specific language pins Whisper to it; anything else (including
 * "auto" or multiple languages) means auto-detect.
 */
export function effectiveLanguage(languages: string[]): string {
  const specific = languages.filter((l) => l !== "auto");
  return specific.length === 1 ? specific[0] : "auto";
}

// ---------------------------------------------------------------------------
// Commands (defined in src-tauri/src/commands.rs)
// ---------------------------------------------------------------------------

/** Mirrors the Rust `PlatformInfo` (commands.rs). `os` is "windows" | "macos" |
 * "linux"; `isWayland` is only ever true on a Linux Wayland session. */
export interface PlatformInfo {
  os: string;
  isWayland: boolean;
}

export const api = {
  getPlatformInfo: () => invoke<PlatformInfo>("get_platform_info"),
  /** macOS Accessibility trust state for the event tap (bare-modifier + mouse
   * triggers). Always resolves `true` off macOS. */
  checkAccessibility: () => invoke<boolean>("check_accessibility"),
  /** Open the macOS Accessibility prompt; resolves to the trust state after. */
  requestAccessibility: () => invoke<boolean>("request_accessibility"),
  getSettings: () => invoke<Settings>("get_settings"),
  updateSettings: (settings: Settings) => invoke<void>("update_settings", { settings }),
  setShortcut: (shortcut: string) => invoke<void>("set_shortcut", { shortcut }),
  setCopyShortcut: (shortcut: string) => invoke<void>("set_copy_shortcut", { shortcut }),
  listInputDevices: () => invoke<string[]>("list_input_devices"),
  storeApiKey: (key: string) => invoke<void>("store_api_key", { key }),
  hasApiKey: () => invoke<boolean>("has_api_key"),
  clearApiKey: () => invoke<void>("clear_api_key"),
  // History (Phase 3)
  getHistory: (page: number, perPage: number, query?: string) =>
    invoke<HistoryPage>("get_history", { page, perPage, query: query ?? null }),
  deleteTranscript: (id: number) => invoke<void>("delete_transcript", { id }),
  recoverTranscript: (id: number) => invoke<void>("recover_transcript", { id }),
  clearHistory: () => invoke<void>("clear_history"),
  getStats: (range: StatsRange) => invoke<Stats>("get_stats", { range }),
  // Dictionary (Phase 4)
  getDictionary: (query?: string) =>
    invoke<DictionaryEntry[]>("get_dictionary", { query: query ?? null }),
  upsertDictionaryEntry: (word: string, replacement: string | null, isStarred: boolean) =>
    invoke<number>("upsert_dictionary_entry", { word, replacement, isStarred }),
  deleteDictionaryEntry: (id: number) => invoke<void>("delete_dictionary_entry", { id }),
  importDictionaryCsv: (csv: string) => invoke<number>("import_dictionary_csv", { csv }),
  exportDictionaryCsv: () => invoke<string>("export_dictionary_csv"),
  // Snippets (Phase 5)
  getSnippets: (query?: string) =>
    invoke<Snippet[]>("get_snippets", { query: query ?? null }),
  upsertSnippet: (triggerPhrase: string, expansion: string, isActive: boolean) =>
    invoke<number>("upsert_snippet", { triggerPhrase, expansion, isActive }),
  deleteSnippet: (id: number) => invoke<void>("delete_snippet", { id }),
  importSnippetsJson: (json: string) => invoke<number>("import_snippets_json", { json }),
  exportSnippetsJson: () => invoke<string>("export_snippets_json"),
  // Flow Styles (Phase 6)
  getFlowStyles: () => invoke<FlowStyle[]>("get_flow_styles"),
  upsertFlowStyle: (
    appCategory: AppCategory,
    tone: FlowTone,
    systemPrompt: string,
    writingSample: string,
    isActive: boolean,
    name = "",
  ) =>
    invoke<number>("upsert_flow_style", {
      name,
      appCategory,
      tone,
      systemPrompt,
      writingSample,
      isActive,
    }),
  deleteFlowStyle: (id: number) => invoke<void>("delete_flow_style", { id }),
  // Command Mode + Transforms (Phase 7)
  setCommandShortcut: (shortcut: string) =>
    invoke<void>("set_command_shortcut", { shortcut }),
  commandModeRewrite: (instruction: string, selectedText: string | null) =>
    invoke<string>("command_mode_rewrite", { selectedText, instruction }),
  getTransforms: () => invoke<Transform[]>("get_transforms"),
  upsertTransform: (t: {
    id: number | null;
    name: string;
    systemPrompt: string;
    shortcut: string;
    autoApply: boolean;
    appCategory: string;
    isActive: boolean;
  }) => invoke<number>("upsert_transform", t),
  deleteTransform: (id: number) => invoke<void>("delete_transform", { id }),
  applyTransform: (id: number, text: string) =>
    invoke<string>("apply_transform", { id, text }),
  // Scratchpad (Phase 9)
  setScratchpadShortcut: (shortcut: string) =>
    invoke<void>("set_scratchpad_shortcut", { shortcut }),
  openScratchpad: () => invoke<void>("open_scratchpad"),
  getScratchpadTabs: () => invoke<ScratchpadTab[]>("get_scratchpad_tabs"),
  createScratchpadTab: (title?: string) =>
    invoke<ScratchpadTab>("create_scratchpad_tab", { title: title ?? null }),
  saveScratchpadTab: (id: number, title: string, content: string) =>
    invoke<void>("save_scratchpad_tab", { id, title, content }),
  deleteScratchpadTab: (id: number) => invoke<void>("delete_scratchpad_tab", { id }),
  // Local models
  listModels: () => invoke<ModelStatus[]>("list_models"),
  downloadModel: (id: string) => invoke<void>("download_model", { id }),
  cancelModelDownload: (id: string) => invoke<void>("cancel_model_download", { id }),
  deleteModel: (id: string) => invoke<void>("delete_model", { id }),
  // Phase 2: prewarm + readiness of the selected local Whisper model.
  prewarmLocalModel: () => invoke<void>("prewarm_local_model"),
  getLocalWhisperStatus: () => invoke<WhisperStatus | null>("get_local_whisper_status"),
  getLocalTranscriptionBenchmark: () =>
    invoke<TranscriptionBenchmark | null>("get_local_transcription_benchmark"),
  // File transcription (Phase C)
  transcribeFiles: (paths: string[]) => invoke<QueueItem[]>("transcribe_files", { paths }),
  cancelQueueItem: (id: number) => invoke<void>("cancel_queue_item", { id }),
  // Startup & auto-update (Phase 11)
  setAutostart: (enabled: boolean) => invoke<void>("set_autostart", { enabled }),
  checkForUpdate: () => invoke<string | null>("check_for_update"),
  installUpdate: () => invoke<boolean>("install_update"),
};
