import { useEffect, useRef, useState } from "react";
import { Keyboard } from "lucide-react";

/**
 * Parity A2: free shortcut recorder. Click to arm, press any key combo, and it
 * is validated by round-tripping through the backend (`set_shortcut` rejects
 * accelerators the global-shortcut plugin can't parse or register). Esc
 * cancels the capture.
 */
export function ShortcutCapture({
  value,
  suggestions,
  onCommit,
}: {
  value: string;
  suggestions: string[];
  /** Persist the accelerator; must reject (throw) when it's unsupported. */
  onCommit: (accelerator: string) => Promise<void>;
}) {
  const [armed, setArmed] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const buttonRef = useRef<HTMLButtonElement | null>(null);

  const commit = async (accelerator: string) => {
    setError(null);
    try {
      await onCommit(accelerator);
    } catch (e) {
      setError(typeof e === "string" ? e : "That shortcut isn't supported.");
    }
  };

  useEffect(() => {
    if (!armed) return;
    const onKeyDown = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") {
        setArmed(false);
        return;
      }
      const accelerator = acceleratorFromEvent(e);
      if (!accelerator) return; // bare modifier - keep listening for the key
      setArmed(false);
      void commit(accelerator);
    };
    const onBlur = () => setArmed(false);
    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("blur", onBlur);
    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("blur", onBlur);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [armed]);

  return (
    <div>
      <button
        ref={buttonRef}
        onClick={() => {
          setError(null);
          setArmed((a) => !a);
        }}
        className={
          "flex w-full items-center justify-between gap-3 rounded-xl border px-3 py-2 text-left text-sm transition-colors " +
          (armed
            ? "border-accent bg-accent/5 text-ink"
            : "border-border bg-surface text-ink hover:border-accent/50")
        }
      >
        <span className="flex items-center gap-2">
          <Keyboard size={15} className="shrink-0 text-ink-faint" />
          {armed ? (
            <span className="text-ink-soft">Press a key combination… (Esc to cancel)</span>
          ) : (
            <kbd className="font-medium">{value}</kbd>
          )}
        </span>
        {!armed && <span className="text-xs text-ink-faint">Click to change</span>}
      </button>
      {error && <p className="mt-2 text-xs text-danger">{error}</p>}
      <div className="mt-2 flex flex-wrap gap-1.5">
        {suggestions.map((s) => (
          <button
            key={s}
            onClick={() => void commit(s)}
            className={
              "rounded-md border px-2 py-0.5 text-xs transition-colors " +
              (s === value
                ? "border-accent/60 bg-accent/10 text-ink"
                : "border-border text-ink-soft hover:bg-surface-2")
            }
          >
            {s}
          </button>
        ))}
      </div>
    </div>
  );
}

/**
 * Build a global-shortcut accelerator string from a keydown event, or null when
 * only modifiers are down. Letters/digits go as single characters; everything
 * else uses the W3C `event.code` name, which matches the `keyboard_types::Code`
 * variants the Rust-side parser accepts. Unsupported keys are rejected by the
 * backend validation either way.
 */
function acceleratorFromEvent(e: KeyboardEvent): string | null {
  const mods: string[] = [];
  if (e.ctrlKey) mods.push("Ctrl");
  if (e.altKey) mods.push("Alt");
  if (e.shiftKey) mods.push("Shift");
  if (e.metaKey) mods.push("Super");

  const code = e.code;
  let key: string | null = null;
  if (/^F\d{1,2}$/.test(code)) key = code;
  else if (/^Key[A-Z]$/.test(code)) key = code.slice(3);
  else if (/^Digit\d$/.test(code)) key = code.slice(5);
  else if (/^(Space|Comma|Period|Slash|Semicolon|Quote|Backquote|Minus|Equal|Backslash|BracketLeft|BracketRight|Home|End|PageUp|PageDown|Insert|Delete|ArrowUp|ArrowDown|ArrowLeft|ArrowRight|Enter|Tab|CapsLock|NumLock|ScrollLock|Pause|PrintScreen)$/.test(code))
    key = code;
  else if (/^Numpad/.test(code)) key = code;

  if (!key) return null;
  return [...mods, key].join("+");
}
