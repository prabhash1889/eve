# Frontend (React / TypeScript)

## Purpose

The two Tauri webview UIs: the **Hub** (Dashboard + Settings) and the **Flow Bar**
(floating dictation widget). Owns rendering and settings *input* only — all OS work
(hotkey, audio, Groq, injection) lives in the Rust backend (`../src-tauri/src/`).

## Entry Points

- `main.tsx` → `Hub.tsx` — Hub window (`../index.html`, window label `main`). Sends commands.
- `flow-bar.tsx` — Flow Bar widget (`../flowbar.html`, label `flowbar`). Frameless,
  transparent, always-on-top, non-focusable; **event-driven only — it sends no commands**.
- `lib/api.ts` — the single IPC surface: `Settings` type, `EVT` event-name map,
  `api.*` command wrappers, `on()` event subscriber.
- `styles/globals.css` — Tailwind v4 design tokens + light/dark (there is no `tailwind.config.js`).

## Contracts & Invariants

- **All IPC goes through `lib/api.ts`.** Don't call `@tauri-apps/api` `invoke`/`listen` directly elsewhere.
- `lib/api.ts` mirrors Rust by hand and MUST stay in sync:
  - `EVT` ↔ `../src-tauri/src/events.rs` consts (`session://*`).
  - `Settings` ↔ `../src-tauri/src/config.rs` (Rust `snake_case` serialized as `camelCase`).
  - `api.*` ↔ commands in `../src-tauri/src/commands.rs`.
- The Flow Bar renders state purely from `session://*` events; it never invokes a command.
- `tsconfig` is strict with `noUnusedLocals`/`noUnusedParameters` — unused symbols fail `npm run build`.

## Patterns

- **New Flow Bar state**: add the event const in both `events.rs` and `EVT`, emit it from
  Rust (`pipeline.rs`/`hotkey.rs`), handle it in `flow-bar.tsx`.
- **New window**: new `.html` at repo root + new rollup input in `../vite.config.ts` +
  window entry in `../src-tauri/tauri.conf.json`.

## Anti-patterns

- No raw `invoke`/`listen` outside `lib/api.ts`.
- Don't add `tailwind.config.js` — Tailwind v4 is wired via the Vite plugin + CSS tokens.
- Don't make the Flow Bar focusable or let it call commands.

## Related Context

- Backend / IPC source of truth: `../src-tauri/src/AGENTS.md`
- Project overview + full sync table: `../CLAUDE.md`
