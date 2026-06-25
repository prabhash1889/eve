import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn, type EventCallback } from "@tauri-apps/api/event";

// ---------------------------------------------------------------------------
// Shared types (mirror the Rust structs in src-tauri/src/config.rs)
// ---------------------------------------------------------------------------

export type CleanupLevel = "none" | "light" | "medium" | "high";

export interface Settings {
  shortcut: string;
  language: string; // "auto" or an ISO-639-1 code
  cleanupLevel: CleanupLevel;
  injectStrategy: "paste" | "type";
  copyShortcut: string;
  bubbleScale: number; // Flow Bar size multiplier (1.0 = default)
  bubbleOpacity: number; // Flow Bar opacity (0–1)
}

export const DEFAULT_SETTINGS: Settings = {
  shortcut: "F8",
  language: "auto",
  cleanupLevel: "none",
  injectStrategy: "paste",
  copyShortcut: "CmdOrCtrl+Shift+C",
  bubbleScale: 1.0,
  bubbleOpacity: 1.0,
};

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
};
