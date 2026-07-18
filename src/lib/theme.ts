// Theme model: a flat list of named palettes. Each theme sets the full `--c-*`
// variable set in globals.css via `[data-theme]`; the `.dark` class is toggled
// alongside so Tailwind `dark:` utilities and the default palettes still work.
// The choice persists in localStorage and syncs across Eve's windows (Hub,
// Flow Bar, Scratchpad) via the `storage` event.

export type ThemeId =
  | "system"
  | "paper"
  | "ink"
  | "slate"
  | "midnight"
  | "nord"
  | "rose"
  | "sky";

// `swatch` = [canvas, accent] preview colors for the picker; keep in sync with
// the palettes in globals.css.
export const THEMES: { id: ThemeId; label: string; dark: boolean; swatch: [string, string] }[] = [
  { id: "system", label: "System", dark: false, swatch: ["#f6f4ef", "#3f9b6e"] },
  { id: "paper", label: "Paper", dark: false, swatch: ["#f6f4ef", "#3f9b6e"] },
  { id: "ink", label: "Ink", dark: true, swatch: ["#16150f", "#5cc591"] },
  { id: "slate", label: "Slate", dark: true, swatch: ["#0b0e14", "#6ea8fe"] },
  { id: "midnight", label: "Midnight", dark: true, swatch: ["#0f0f1a", "#8b7cff"] },
  { id: "nord", label: "Nord", dark: true, swatch: ["#2e3440", "#88c0d0"] },
  { id: "rose", label: "Rosé", dark: false, swatch: ["#fbf3f2", "#c4547e"] },
  { id: "sky", label: "Sky", dark: false, swatch: ["#f2f6fb", "#2f7fd6"] },
];

const KEY = "eve-theme";

function systemDark(): boolean {
  return window.matchMedia("(prefers-color-scheme: dark)").matches;
}

export function loadTheme(): ThemeId {
  const saved = localStorage.getItem(KEY) as ThemeId | null;
  return saved && THEMES.some((t) => t.id === saved) ? saved : "system";
}

export function applyTheme(id: ThemeId): void {
  const el = document.documentElement;
  if (id === "system") {
    delete el.dataset.theme;
    el.classList.toggle("dark", systemDark());
    return;
  }
  const t = THEMES.find((x) => x.id === id);
  el.dataset.theme = id;
  el.classList.toggle("dark", t ? t.dark : systemDark());
}

export function setTheme(id: ThemeId): void {
  localStorage.setItem(KEY, id);
  applyTheme(id);
}

// Apply the saved theme and keep this window in sync when another Eve window
// changes it. Call once at window startup, before React renders.
export function initTheme(): void {
  applyTheme(loadTheme());
  window.addEventListener("storage", (e) => {
    if (e.key === KEY) applyTheme(loadTheme());
  });
}
