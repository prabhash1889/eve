import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { listen, type UnlistenFn, type EventCallback } from "@tauri-apps/api/event";

// ---------------------------------------------------------------------------
// Shared types (mirror the Rust structs in src-tauri/src/config.rs)
// ---------------------------------------------------------------------------

export type CleanupLevel = "none" | "light" | "medium" | "high";

export type AudioStoragePolicy = "store" | "delete24h" | "never";

export interface Settings {
  shortcut: string;
  language: string; // "auto" or an ISO-639-1 code
  cleanupLevel: CleanupLevel;
  injectStrategy: "paste" | "type";
  copyShortcut: string;
  bubbleScale: number; // Flow Bar size multiplier (1.0 = default)
  bubbleOpacity: number; // Flow Bar opacity (0–1)
  audioStoragePolicy: AudioStoragePolicy; // retention of saved audio (Phase 3)
  audioRetentionHours: number; // window for "delete24h"
}

export const DEFAULT_SETTINGS: Settings = {
  shortcut: "F8",
  language: "auto",
  cleanupLevel: "none",
  injectStrategy: "paste",
  copyShortcut: "CmdOrCtrl+Shift+C",
  bubbleScale: 1.0,
  bubbleOpacity: 1.0,
  audioStoragePolicy: "delete24h",
  audioRetentionHours: 24,
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
}

export interface HistoryPage {
  items: Transcript[];
  total: number;
  page: number;
  perPage: number;
}

export interface Stats {
  totalWords: number;
  totalSessions: number;
  totalMs: number;
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

/** Convert a stored audio file path into an asset:// URL the `<audio>` tag can load. */
export const audioSrc = (path: string): string => convertFileSrc(path);

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
export interface StartPayload {
  bubbleScale: number;
  bubbleOpacity: number;
}

export function on<T>(event: string, cb: EventCallback<T>): Promise<UnlistenFn> {
  return listen<T>(event, cb);
}

// ---------------------------------------------------------------------------
// Commands (defined in src-tauri/src/commands.rs)
// ---------------------------------------------------------------------------

export const api = {
  getSettings: () => invoke<Settings>("get_settings"),
  updateSettings: (settings: Settings) => invoke<void>("update_settings", { settings }),
  setShortcut: (shortcut: string) => invoke<void>("set_shortcut", { shortcut }),
  setCopyShortcut: (shortcut: string) => invoke<void>("set_copy_shortcut", { shortcut }),
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
};
