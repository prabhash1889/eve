// Build-time app edition, set via the `VITE_EVE_EDITION` env var at build time.
//
// - "store": the Microsoft Store build. Offline-first by default (Parakeet
//   bundled and selected out of the box), so the Local models catalog is hidden
//   (whisper.cpp isn't compiled into that build). Cloud providers ARE
//   available: users can add keys and route speech/polish through them like in
//   any other edition.
// - "full" (default): the complete developer / direct-download build.
//
// The matching backend defaults live behind the `store-edition` Cargo feature.
export const EDITION: string = import.meta.env.VITE_EVE_EDITION || "full";

/** True in the trimmed, offline-first-by-default Microsoft Store build. */
export const isStore = EDITION === "store";
