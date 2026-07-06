// Synchronous, best-effort OS detection for the webview, used only for
// render-time modifier labels (⌘/⌥ vs Ctrl/Alt). For anything authoritative -
// the exact OS string or Wayland detection - use `api.getPlatformInfo()`, which
// the Rust backend answers definitively.

export const isMac =
  typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.userAgent);

export const isLinux =
  typeof navigator !== "undefined" &&
  /Linux/.test(navigator.userAgent) &&
  !/Android/.test(navigator.userAgent) &&
  !isMac;

/**
 * Example value shown for the auto-pause app list, per OS: Windows matches the
 * executable name, macOS the app bundle id, Linux the `/proc/<pid>/comm` name.
 */
export function pausedAppExample(): string {
  if (isMac) return "com.apple.mail";
  if (isLinux) return "keepassxc";
  return "1password.exe";
}

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
