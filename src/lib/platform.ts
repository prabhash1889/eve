// Synchronous, best-effort OS detection for the webview, used only for
// render-time modifier labels (⌘/⌥ vs Ctrl/Alt). For anything authoritative -
// the exact OS string or Wayland detection - use `api.getPlatformInfo()`, which
// the Rust backend answers definitively.

export const isMac =
  typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.userAgent);

/**
 * Rewrite an accelerator string for display on the current OS: on macOS show the
 * Cmd/Option glyphs instead of the "Super"/"Alt" tokens the backend parses.
 * Purely cosmetic - the stored accelerator is unchanged.
 */
export function labelAccelerator(accelerator: string): string {
  if (!isMac) return accelerator;
  return accelerator
    .split("+")
    .map((tok) => {
      switch (tok) {
        case "Super":
        case "Cmd":
        case "Command":
        case "CmdOrCtrl":
          return "⌘";
        case "Alt":
        case "Option":
          return "⌥";
        case "Shift":
          return "⇧";
        case "Ctrl":
        case "Control":
          return "⌃";
        default:
          return tok;
      }
    })
    .join(isMac ? "" : "+");
}
