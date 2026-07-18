// Build-time app edition, set via the `VITE_EVE_EDITION` env var at build time.
//
// - "store": the Microsoft Store build. Offline-first (Parakeet bundled, no Groq
//   key), so the UI hides cloud/whisper affordances (Groq key, backend pickers,
//   the Local models catalog) that don't apply.
// - "full" (default): the complete developer / direct-download build.
//
// The matching backend defaults live behind the `store-edition` Cargo feature.
export const EDITION: string = import.meta.env.VITE_EVE_EDITION || "full";

/** True in the trimmed, offline-first Microsoft Store build. */
export const isStore = EDITION === "store";
