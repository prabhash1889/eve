import { useEffect, useState, type ReactNode } from "react";
import {
  Settings as SettingsIcon,
  LayoutDashboard,
  BookMarked,
  Zap,
  Sparkles,
  KeyRound,
  Check,
  Palette,
  Type,
  Gauge,
  AudioLines,
  Cpu,
  Wand2,
  BarChart3,
  Code2,
  NotebookPen,
  ShieldCheck,
  Languages,
  Mic,
  Plus,
  X,
  RefreshCw,
  Power,
  Database,
} from "lucide-react";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { api, DEFAULT_SETTINGS, effectiveLanguage, on, EVT, type Settings, type CleanupLevel, type Stats } from "./lib/api";
import {
  ACTIVATION_MODES,
  CLEANUP,
  MODIFIER_TRIGGERS,
  MOUSE_TRIGGERS,
  PROVIDERS,
  SHORTCUT_CHOICES,
  STT_PROVIDERS,
} from "./lib/options";
import { pausedAppExample } from "./lib/platform";
import { THEMES, loadTheme, setTheme, type ThemeId } from "./lib/theme";
import { ShortcutCapture } from "./components/ShortcutCapture";
import { FileQueue } from "./components/FileQueue";
import { Onboarding, LanguageMultiSelect } from "./components/onboarding/Onboarding";
import { HistoryPage } from "./pages/HistoryPage";
import { DictionaryPage } from "./pages/DictionaryPage";
import { SnippetsPage } from "./pages/SnippetsPage";
import { StylesPage } from "./pages/StylesPage";
import { TransformsPage } from "./pages/TransformsPage";
import { LocalModelsPage } from "./pages/LocalModelsPage";
import { InsightsPage } from "./pages/InsightsPage";
import { isStore } from "./lib/edition";

const COPY_SHORTCUT_CHOICES = [
  "CmdOrCtrl+Shift+C",
  "CmdOrCtrl+Shift+V",
  "CmdOrCtrl+Alt+C",
  "Alt+C",
];

const COMMAND_SHORTCUT_CHOICES = [
  "CmdOrCtrl+Shift+Alt+Space",
  "CmdOrCtrl+Shift+Space",
  "CmdOrCtrl+Alt+Space",
  "Alt+Space",
];

const SCRATCHPAD_SHORTCUT_CHOICES = [
  "CmdOrCtrl+Shift+S",
  "CmdOrCtrl+Shift+N",
  "CmdOrCtrl+Alt+S",
  "Alt+S",
];

const UNDO_SHORTCUT_CHOICES = [
  "CmdOrCtrl+Shift+Alt+Z",
  "CmdOrCtrl+Shift+Z",
  "CmdOrCtrl+Alt+Z",
  "Alt+Z",
];

type Nav =
  | "dashboard"
  | "insights"
  | "settings"
  | "dictionary"
  | "snippets"
  | "styles"
  | "transforms"
  | "models";

export function Hub() {
  const [nav, setNav] = useState<Nav>("dashboard");
  const [settings, setSettings] = useState<Settings>(DEFAULT_SETTINGS);
  const [hasKey, setHasKey] = useState(false);
  const [loaded, setLoaded] = useState(false);
  // Bumped when the tray "Check for updates" item fires, so the Settings panel
  // can auto-run the check.
  const [updateNonce, setUpdateNonce] = useState(0);
  const [theme, setThemeState] = useState<ThemeId>(() => loadTheme());

  useEffect(() => {
    let cancelled = false;
    // The Rust `AppState` is `.manage()`d partway through the setup hook, after
    // DB open + audio pruning. In a release build the webview can call
    // `get_settings` before that runs, so the invoke rejects. Retry until the
    // backend is ready, and only flip `loaded` (which arms the first-run
    // onboarding gate) on a *successful* load — never mark loaded on failure, or
    // `settings` stays at DEFAULT_SETTINGS (onboardingComplete: false) and the
    // onboarding reappears on every launch that loses this race.
    const loadSettings = (attempt = 0) => {
      api
        .getSettings()
        .then((s) => {
          if (cancelled) return;
          setSettings(s);
          setLoaded(true);
        })
        .catch(() => {
          if (!cancelled && attempt < 50) {
            setTimeout(() => loadSettings(attempt + 1), 100);
          }
        });
    };
    loadSettings();
    api.hasApiKey().then(setHasKey).catch(() => {});
    return () => {
      cancelled = true;
    };
  }, []);

  // Phase 11: the tray "Check for updates" item routes here.
  useEffect(() => {
    const unlisten = on(EVT.checkUpdate, () => {
      setNav("settings");
      setUpdateNonce((n) => n + 1);
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const pickTheme = (id: ThemeId) => {
    setThemeState(id);
    setTheme(id);
  };

  // Phase 10: gate the app behind first-run onboarding.
  if (loaded && !settings.onboardingComplete) {
    return (
      <div className="h-full bg-canvas text-ink">
        <Onboarding
          settings={settings}
          onComplete={(next) => {
            setSettings(next);
            api.hasApiKey().then(setHasKey).catch(() => {});
          }}
        />
      </div>
    );
  }

  return (
    <div className="flex h-full bg-canvas text-ink">
      {/* Sidebar */}
      <aside className="flex w-60 shrink-0 flex-col gap-1 border-r border-border bg-surface/60 p-4">
        <div className="mb-6 flex items-center gap-2 px-2">
          <img
            src="/image-src/logo/logo.png"
            alt="Eve logo"
            className="h-9 w-9 rounded-2xl object-cover"
          />
          <div>
            <div className="font-serif text-lg leading-none">Eve</div>
            <div className="text-xs text-ink-faint">voice dictation</div>
          </div>
        </div>

        <NavItem icon={<LayoutDashboard size={18} />} label="Home" active={nav === "dashboard"} onClick={() => setNav("dashboard")} />
        <NavItem icon={<BarChart3 size={18} />} label="Insights" active={nav === "insights"} onClick={() => setNav("insights")} />
        <NavItem icon={<BookMarked size={18} />} label="Dictionary" active={nav === "dictionary"} onClick={() => setNav("dictionary")} />
        <NavItem icon={<Zap size={18} />} label="Snippets" active={nav === "snippets"} onClick={() => setNav("snippets")} />
        {/* Styles + Transforms are LLM-polish features; the offline Store build
            has no polish model, so they are hidden there. */}
        {!isStore && (
          <>
            <NavItem icon={<Sparkles size={18} />} label="Styles" active={nav === "styles"} onClick={() => setNav("styles")} />
            <NavItem icon={<Wand2 size={18} />} label="Transforms" active={nav === "transforms"} onClick={() => setNav("transforms")} />
          </>
        )}
        <NavItem icon={<NotebookPen size={18} />} label="Scratchpad" onClick={() => api.openScratchpad().catch(() => {})} />
        {/* Store edition runs offline on the bundled Parakeet model - there is
            no cloud/whisper choice to configure, so the catalog is hidden. */}
        {!isStore && (
          <NavItem icon={<Cpu size={18} />} label="Local models" active={nav === "models"} onClick={() => setNav("models")} />
        )}
        <NavItem icon={<SettingsIcon size={18} />} label="Settings" active={nav === "settings"} onClick={() => setNav("settings")} />

        <div className="mt-auto">
          <ThemePicker value={theme} onChange={pickTheme} />
        </div>
      </aside>

      {/* Content */}
      <main className="flex-1 overflow-y-auto">
        <div className="mx-auto max-w-2xl px-10 py-10">
          {nav === "dashboard" ? (
            <Dashboard settings={settings} hasKey={hasKey} onConfigure={() => setNav("settings")} />
          ) : nav === "insights" ? (
            <InsightsPage />
          ) : nav === "dictionary" ? (
            <DictionaryPage />
          ) : nav === "snippets" ? (
            <SnippetsPage />
          ) : nav === "styles" ? (
            <StylesPage />
          ) : nav === "transforms" ? (
            <TransformsPage />
          ) : nav === "models" ? (
            <LocalModelsPage settings={settings} setSettings={setSettings} />
          ) : (
            <SettingsPanel
              settings={settings}
              setSettings={setSettings}
              updateNonce={updateNonce}
            />
          )}
        </div>
      </main>
    </div>
  );
}

function NavItem({
  icon,
  label,
  active,
  disabled,
  onClick,
}: {
  icon: ReactNode;
  label: string;
  active?: boolean;
  disabled?: boolean;
  onClick?: () => void;
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      className={
        "flex items-center gap-3 rounded-xl px-3 py-2 text-sm transition-colors " +
        (active
          ? "bg-accent-soft text-ink font-medium"
          : disabled
            ? "cursor-not-allowed text-ink-faint/60"
            : "text-ink-soft hover:bg-surface-2")
      }
    >
      {icon}
      {label}
    </button>
  );
}

function ThemePicker({ value, onChange }: { value: ThemeId; onChange: (id: ThemeId) => void }) {
  const [open, setOpen] = useState(false);
  const current = THEMES.find((t) => t.id === value) ?? THEMES[0];

  // Close when clicking outside the picker.
  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (!(e.target as HTMLElement).closest("[data-theme-picker]")) setOpen(false);
    };
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  }, [open]);

  return (
    <div className="relative" data-theme-picker>
      {open && (
        <div className="absolute bottom-full left-0 mb-2 w-full rounded-xl border border-border bg-surface p-1 shadow-lg">
          {THEMES.map((t) => (
            <button
              key={t.id}
              onClick={() => {
                onChange(t.id);
                setOpen(false);
              }}
              className={
                "flex w-full items-center gap-2.5 rounded-lg px-2.5 py-2 text-sm " +
                (t.id === value ? "bg-surface-2 text-ink font-medium" : "text-ink-soft hover:bg-surface-2")
              }
            >
              <span
                className="h-4 w-4 shrink-0 rounded-full border border-border"
                style={{ background: `linear-gradient(135deg, ${t.swatch[0]} 50%, ${t.swatch[1]} 50%)` }}
              />
              <span className="flex-1 text-left">{t.label}</span>
              {t.id === value && <Check size={14} className="text-accent" />}
            </button>
          ))}
        </div>
      )}
      <button
        onClick={() => setOpen((o) => !o)}
        className="flex w-full items-center gap-2 rounded-xl px-3 py-2 text-sm text-ink-soft hover:bg-surface-2"
      >
        <Palette size={16} />
        <span className="flex-1 text-left">Theme</span>
        <span className="text-ink-faint">{current.label}</span>
      </button>
    </div>
  );
}

function Dashboard({
  settings,
  hasKey,
  onConfigure,
}: {
  settings: Settings;
  hasKey: boolean;
  onConfigure: () => void;
}) {
  // Bumped when a queued file finishes so the embedded History list reloads.
  const [historyReload, setHistoryReload] = useState(0);
  return (
    <div>
      <h1 className="font-serif text-3xl">Welcome to Eve</h1>
      <p className="mt-2 max-w-md text-ink-soft">
        Hold your hotkey anywhere, speak, and release. Eve transcribes your voice and types it
        into whatever app you're using.
      </p>

      <TodayStats />

      <div className="mt-6 grid gap-4">
        <div className="rounded-2xl border border-border bg-surface p-5">
          <div className="text-sm text-ink-faint">Push-to-talk hotkey</div>
          <div className="mt-1 flex items-center gap-3">
            <kbd className="rounded-lg border border-border bg-surface-2 px-3 py-1.5 font-mono text-lg">
              {settings.shortcut}
            </kbd>
            <span className="text-ink-soft">— hold, speak, release.</span>
          </div>
        </div>

        {!isStore && (
          <div className="rounded-2xl border border-border bg-surface p-5">
            <div className="flex items-center justify-between">
              <div>
                <div className="text-sm text-ink-faint">Groq API key</div>
                <div className="mt-1 flex items-center gap-2">
                  {hasKey ? (
                    <>
                      <Check size={18} className="text-accent" />
                      <span>Configured</span>
                    </>
                  ) : (
                    <span className="text-danger">Not set — required to transcribe</span>
                  )}
                </div>
              </div>
              {!hasKey && (
                <button
                  onClick={onConfigure}
                  className="rounded-xl bg-accent px-4 py-2 text-sm font-medium text-white hover:opacity-90"
                >
                  Add key
                </button>
              )}
            </div>
          </div>
        )}
      </div>

      <div className="mt-6">
        <FileQueue hasKey={hasKey} onItemDone={() => setHistoryReload((n) => n + 1)} />
      </div>

      <div className="mt-10">
        <HistoryPage reloadSignal={historyReload} />
      </div>
    </div>
  );
}

/** Today's dictation stats: words spoken and average words-per-minute. */
function TodayStats() {
  const [stats, setStats] = useState<Stats | null>(null);

  useEffect(() => {
    api.getStats("day").then(setStats).catch(() => {});
  }, []);

  // WPM over actual dictation time (sum of recording durations), not wall-clock.
  const minutes = stats ? stats.totalMs / 60000 : 0;
  const wpm = stats && minutes > 0 ? Math.round(stats.totalWords / minutes) : null;

  return (
    <div className="mt-8 grid grid-cols-3 gap-4">
      <StatCard
        icon={<Type size={16} />}
        label="Words today"
        value={stats ? stats.totalWords.toLocaleString() : "—"}
      />
      <StatCard
        icon={<Gauge size={16} />}
        label="Words / min"
        value={wpm != null ? String(wpm) : "—"}
      />
      <StatCard
        icon={<AudioLines size={16} />}
        label="Dictations today"
        value={stats ? String(stats.totalSessions) : "—"}
      />
    </div>
  );
}

function StatCard({ icon, label, value }: { icon: ReactNode; label: string; value: string }) {
  return (
    <div className="rounded-2xl border border-border bg-surface p-4">
      <div className="flex items-center gap-1.5 text-xs text-ink-faint">
        {icon}
        {label}
      </div>
      <div className="mt-2 font-serif text-3xl tabular-nums">{value}</div>
    </div>
  );
}

function SettingsPanel({
  settings,
  setSettings,
  updateNonce,
}: {
  settings: Settings;
  setSettings: (s: Settings) => void;
  updateNonce: number;
}) {
  const [inputDevices, setInputDevices] = useState<string[]>([]);
  // Phase 4 (Linux/Wayland): the compositor owns global shortcuts via the XDG
  // portal, which can't express bare-modifier/mouse triggers or an Esc-cancel
  // bind - so those affordances are hidden, and privacy pause can't match a
  // (Wayland-hidden) foreground app.
  const [isWayland, setIsWayland] = useState(false);

  // Enumerate capture devices for the microphone picker (best-effort).
  useEffect(() => {
    api.listInputDevices().then(setInputDevices).catch(() => {});
    api.getPlatformInfo().then((p) => setIsWayland(p.isWayland)).catch(() => {});
  }, []);

  const persist = async (next: Settings) => {
    setSettings(next);
    await api.updateSettings(next).catch(() => {});
  };

  return (
    <div>
      <h1 className="font-serif text-3xl">Settings</h1>

      {!isStore && <ProvidersSection settings={settings} persist={persist} />}

      <Section title="Record trigger">
        <ShortcutCapture
          value={settings.shortcut}
          suggestions={SHORTCUT_CHOICES}
          onCommit={async (v) => {
            await api.setShortcut(v); // throws on unsupported/duplicate keys
            setSettings({ ...settings, shortcut: v });
          }}
        />

        <div className="mt-4">
          <Select
            value={settings.activationMode}
            onChange={(v) =>
              persist({ ...settings, activationMode: v as Settings["activationMode"] })
            }
            options={ACTIVATION_MODES.map((m) => ({ value: m.value, label: m.label }))}
          />
          <p className="mt-2 text-xs text-ink-faint">
            {ACTIVATION_MODES.find((m) => m.value === settings.activationMode)?.hint}
          </p>
        </div>

        {settings.activationMode !== "hold" && (
          <div className="mt-4">
            <Select
              value={String(settings.autoStopSilenceSecs)}
              onChange={(v) => persist({ ...settings, autoStopSilenceSecs: Number(v) })}
              options={[
                { value: "0", label: "Auto-stop: off" },
                { value: "1", label: "Auto-stop after 1s silence" },
                { value: "2", label: "Auto-stop after 2s silence" },
                { value: "3", label: "Auto-stop after 3s silence" },
                { value: "5", label: "Auto-stop after 5s silence" },
              ]}
            />
            <p className="mt-2 text-xs text-ink-faint">
              Hands-free: end the recording automatically when you've been quiet for this long.
            </p>
          </div>
        )}

        {isWayland ? (
          <p className="mt-4 text-xs text-ink-faint">
            On Wayland, the desktop's global-shortcut portal owns your triggers: bare-modifier
            and mouse-button triggers aren't available, and Esc can't cancel a recording (toggle
            the record trigger again to stop). The portal may prompt you to confirm the binding on
            first use.
          </p>
        ) : (
          <>
            <div className="mt-4 grid grid-cols-2 gap-3">
              <div>
                <div className="mb-1.5 text-xs font-medium text-ink-soft">Modifier key trigger</div>
                <Select
                  value={settings.modifierTrigger}
                  onChange={(v) => persist({ ...settings, modifierTrigger: v })}
                  options={MODIFIER_TRIGGERS}
                />
              </div>
              <div>
                <div className="mb-1.5 text-xs font-medium text-ink-soft">Mouse button trigger</div>
                <Select
                  value={settings.mouseTrigger}
                  onChange={(v) => persist({ ...settings, mouseTrigger: v })}
                  options={MOUSE_TRIGGERS}
                />
              </div>
            </div>
            <p className="mt-2 text-xs text-ink-faint">
              Extra triggers that work alongside the hotkey. A bound mouse button's normal click is
              disabled while assigned.
            </p>
          </>
        )}
      </Section>

      <Section title="Microphone" icon={<Mic size={16} />}>
        <Select
          value={settings.inputDevice}
          onChange={(v) => persist({ ...settings, inputDevice: v })}
          options={[
            { value: "", label: "System default" },
            // Keep the saved device selectable even if it's currently unplugged.
            ...(settings.inputDevice && !inputDevices.includes(settings.inputDevice)
              ? [{ value: settings.inputDevice, label: `${settings.inputDevice} (disconnected)` }]
              : []),
            ...inputDevices.map((d) => ({ value: d, label: d })),
          ]}
        />
        <label className="flex items-center justify-between gap-4 mt-3 py-1.5 cursor-pointer">
          <span className="text-sm text-ink-soft">Play sound when recording starts</span>
          <input
            type="checkbox"
            checked={settings.soundOnStart}
            onChange={(e) => persist({ ...settings, soundOnStart: e.target.checked })}
            className="size-4 shrink-0 accent-accent"
          />
        </label>
        <p className="mt-2 text-xs text-ink-faint">
          Which microphone to record from. “System default” follows your Windows input device.
          The choice applies to your next dictation; if the selected mic is unavailable, Eve
          falls back to the default.
        </p>
      </Section>

      <Section title="Languages" icon={<Languages size={16} />}>
        <LanguageMultiSelect
          value={settings.languages}
          onChange={(languages) =>
            persist({ ...settings, languages, language: effectiveLanguage(languages) })
          }
        />
        <label className="flex items-center justify-between gap-4 mt-3 py-1.5 cursor-pointer">
          <span className="text-sm text-ink-soft">Translate audio to English</span>
          <input
            type="checkbox"
            checked={settings.translateToEnglish}
            onChange={(e) => persist({ ...settings, translateToEnglish: e.target.checked })}
            className="size-4 shrink-0 accent-accent"
          />
        </label>
        <label className="flex items-center justify-between gap-4 mt-1.5 py-1.5 cursor-pointer">
          <span className="text-sm text-ink-soft">CJK Autocorrect spacing</span>
          <input
            type="checkbox"
            checked={settings.cjkAutocorrect}
            onChange={(e) => persist({ ...settings, cjkAutocorrect: e.target.checked })}
            className="size-4 shrink-0 accent-accent"
          />
        </label>
        <p className="mt-2 text-xs text-ink-faint">
          Pick one language to lock transcription to it, or several (or Auto-detect) to let
          Eve detect per dictation. CJK Autocorrect formats spacing between Chinese/Japanese/Korean
          characters and English letters.
        </p>
      </Section>

      <Section title="Whisper Prompt" icon={<Sparkles size={16} />}>
        <input
          type="text"
          value={settings.whisperPrompt}
          onChange={(e) => persist({ ...settings, whisperPrompt: e.target.value })}
          placeholder="e.g. Transcribe exactly as spoken, with punctuation."
          className="w-full rounded-xl border border-border bg-surface px-3 py-2 outline-none focus:border-accent text-sm"
        />
        <p className="mt-2 text-xs text-ink-faint">
          Instruction or context to guide the Whisper model. Prepended to your dictionary terms.
        </p>
      </Section>

      {/* Cleanup level is LLM polish; the offline Store build has no polish
          model, so it stays on deterministic cleanup and the selector is hidden. */}
      {!isStore && (
        <Section title="Cleanup level">
          <Select
            value={settings.cleanupLevel}
            onChange={(v) => persist({ ...settings, cleanupLevel: v as CleanupLevel })}
            options={CLEANUP.map((c) => ({ value: c.value, label: c.label }))}
          />
          <p className="mt-2 text-xs text-ink-faint">
            {CLEANUP.find((c) => c.value === settings.cleanupLevel)?.hint}
            {settings.cleanupLevel !== "none" &&
              " · uses your cloud LLM provider (needs its API key)"}
          </p>
        </Section>
      )}

      <Section title="Copy last transcript" icon={<Sparkles size={16} />}>
        <Select
          value={settings.copyShortcut}
          onChange={async (v) => {
            const next = { ...settings, copyShortcut: v };
            setSettings(next);
            await api.setCopyShortcut(v).catch(() => {});
          }}
          options={COPY_SHORTCUT_CHOICES.map((s) => ({ value: s, label: s }))}
        />
        <p className="mt-2 text-xs text-ink-faint">
          Press this anytime to copy your most recent transcript to the clipboard.
        </p>
      </Section>

      {/* Command Mode rewrites/generates via an LLM; hidden in the offline
          Store build, which has no polish model. */}
      {!isStore && (
        <Section title="Command Mode" icon={<Wand2 size={16} />}>
          <Select
            value={settings.commandShortcut}
            onChange={async (v) => {
              const next = { ...settings, commandShortcut: v };
              setSettings(next);
              await api.setCommandShortcut(v).catch(() => {});
            }}
            options={COMMAND_SHORTCUT_CHOICES.map((s) => ({ value: s, label: s }))}
          />
          <p className="mt-2 text-xs text-ink-faint">
            Hold this and speak an instruction. With text selected, Eve rewrites it; with
            nothing selected, it generates text at your cursor. Uses your cloud LLM
            provider (needs its API key).
          </p>
        </Section>
      )}

      <Section title="Scratchpad" icon={<NotebookPen size={16} />}>
        <Select
          value={settings.scratchpadShortcut}
          onChange={async (v) => {
            const next = { ...settings, scratchpadShortcut: v };
            setSettings(next);
            await api.setScratchpadShortcut(v).catch(() => {});
          }}
          options={SCRATCHPAD_SHORTCUT_CHOICES.map((s) => ({ value: s, label: s }))}
        />
        <p className="mt-2 text-xs text-ink-faint">
          Opens the floating multi-tab notepad. While it's focused, dictation lands
          in the editor at your cursor instead of pasting into another app.
        </p>
        <button
          onClick={() => api.openScratchpad().catch(() => {})}
          className="mt-3 rounded-xl bg-accent px-4 py-2 text-sm font-medium text-white hover:opacity-90"
        >
          Open Scratchpad
        </button>
      </Section>

      <Section title="Undo last injection" icon={<Sparkles size={16} />}>
        <Select
          value={settings.undoShortcut}
          onChange={async (v) => {
            const next = { ...settings, undoShortcut: v };
            setSettings(next);
            await api.setUndoShortcut(v).catch(() => {});
          }}
          options={UNDO_SHORTCUT_CHOICES.map((s) => ({ value: s, label: s }))}
        />
        <p className="mt-2 text-xs text-ink-faint">
          Press this to delete what Eve just pasted (re-focuses the target and sends one
          Backspace per character). Best-effort: if you typed or moved the cursor since the paste,
          the Backspaces land wherever the caret is now.
        </p>
      </Section>

      <Section title="Vibe-coding" icon={<Code2 size={16} />}>
        <label className="flex cursor-pointer items-center justify-between gap-3">
          <span className="text-sm text-ink-soft">
            In code editors, wrap spoken{" "}
            <span className="font-medium text-ink">“backtick X backtick”</span> in literal
            backticks before injecting.
          </span>
          <input
            type="checkbox"
            checked={settings.vibeCoding}
            onChange={(e) => persist({ ...settings, vibeCoding: e.target.checked })}
            className="size-4 shrink-0 accent-accent"
          />
        </label>
        <p className="mt-2 text-xs text-ink-faint">
          Only applies when the focused app is detected as a code editor (VS Code, Cursor…).
        </p>
      </Section>

      <Section title="Flow Bar appearance">
        <div className="mb-4">
          <div className="mb-1.5 text-xs font-medium text-ink-soft">Positioning</div>
          <Select
            value={settings.barPosition}
            onChange={(v) => persist({ ...settings, barPosition: v as Settings["barPosition"] })}
            options={[
              { value: "fixed", label: "Fixed (Bottom center)" },
              { value: "near_caret", label: "Near Caret (Anchored)" },
            ]}
          />
        </div>
        <Range
          label="Size"
          min={0.7}
          max={1.4}
          step={0.05}
          value={settings.bubbleScale}
          format={(v) => `${Math.round(v * 100)}%`}
          onChange={(v) => persist({ ...settings, bubbleScale: v })}
        />
        <div className="mt-4">
          <Range
            label="Opacity"
            min={0.4}
            max={1}
            step={0.05}
            value={settings.bubbleOpacity}
            format={(v) => `${Math.round(v * 100)}%`}
            onChange={(v) => persist({ ...settings, bubbleOpacity: v })}
          />
        </div>
        <p className="mt-3 text-xs text-ink-faint">
          Takes effect the next time the Flow Bar appears.
        </p>
      </Section>

      <Section title="Privacy" icon={<ShieldCheck size={16} />}>
        <label className="flex cursor-pointer items-center justify-between gap-3">
          <span className="text-sm text-ink-soft">
            <span className="font-medium text-ink">Context awareness</span> — detect the
            focused app to adapt tone (Styles) and label History.
          </span>
          <input
            type="checkbox"
            checked={settings.contextAwareness}
            onChange={(e) => persist({ ...settings, contextAwareness: e.target.checked })}
            className="size-4 shrink-0 accent-accent"
          />
        </label>
        <p className="mt-2 text-xs text-ink-faint">
          When off, Eve won't read window titles or app names. Flow Styles stop adapting and
          History rows are saved without app info.
        </p>

        <div className="mt-5 border-t border-border pt-4">
          <div className="text-sm font-medium text-ink">Auto-pause apps</div>
          <p className="mb-3 mt-1 text-xs text-ink-faint">
            Dictation is suppressed when one of these apps is focused. Use the app's process name
            (e.g. <span className="font-mono">{pausedAppExample()}</span>).
          </p>
          {isWayland && (
            <p className="mb-3 -mt-1 text-xs text-danger">
              On Wayland, Eve can't see which app is focused, so auto-pause can't match. Keep
              sensitive apps closed while dictating.
            </p>
          )}
          <PausedAppsEditor
            apps={settings.pausedApps}
            onChange={(pausedApps) => persist({ ...settings, pausedApps })}
          />
        </div>
      </Section>

      <Section title="Startup & updates" icon={<Power size={16} />}>
        <label className="flex cursor-pointer items-center justify-between gap-3">
          <span className="text-sm text-ink-soft">
            <span className="font-medium text-ink">Launch at startup</span> — start Eve
            automatically when you sign in to Windows.
          </span>
          <input
            type="checkbox"
            checked={settings.launchAtStartup}
            onChange={async (e) => {
              const enabled = e.target.checked;
              const prev = settings.launchAtStartup;
              setSettings({ ...settings, launchAtStartup: enabled });
              try {
                await api.setAutostart(enabled);
              } catch {
                // Revert the toggle if the OS autostart change didn't take.
                setSettings({ ...settings, launchAtStartup: prev });
              }
            }}
            className="size-4 shrink-0 accent-accent"
          />
        </label>

        <label className="mt-4 flex cursor-pointer items-center justify-between gap-3">
          <span className="text-sm text-ink-soft">
            <span className="font-medium text-ink">Debug timing</span> — print a
            detailed per-stage latency breakdown to the console for each dictation.
          </span>
          <input
            type="checkbox"
            checked={settings.debugTiming}
            onChange={(e) => persist({ ...settings, debugTiming: e.target.checked })}
            className="size-4 shrink-0 accent-accent"
          />
        </label>

        <div className="mt-5 border-t border-border pt-4">
          <UpdateChecker nonce={updateNonce} />
        </div>
      </Section>

      <BackupSection />
    </div>
  );
}

/** 4.6: full backup bundle export/import (settings + dictionary + snippets +
 * styles + transforms, optionally history; secrets never included). */
function BackupSection() {
  const [includeHistory, setIncludeHistory] = useState(true);
  const [busy, setBusy] = useState(false);

  const onExport = async () => {
    const path = await saveDialog({
      defaultPath: `eve-backup-${new Date().toISOString().slice(0, 10)}.json`,
      filters: [{ name: "JSON", extensions: ["json"] }],
    });
    if (!path) return;
    setBusy(true);
    try {
      await api.exportBackup(path, includeHistory);
      alert("Backup saved.");
    } catch {
      alert("Couldn't write the backup file.");
    } finally {
      setBusy(false);
    }
  };

  const onImport = async () => {
    const path = await openDialog({
      multiple: false,
      filters: [{ name: "JSON", extensions: ["json"] }],
    });
    if (!path || typeof path !== "string") return;
    if (
      !confirm(
        "Restore this backup? Settings will be replaced and data rows merged into what's already here.",
      )
    )
      return;
    setBusy(true);
    try {
      const s = await api.importBackup(path);
      alert(
        `Restored: ${s.dictionary} dictionary, ${s.snippets} snippets, ${s.flowStyles} styles, ${s.transforms} transforms` +
          (s.transcripts ? `, ${s.transcripts} transcripts` : "") +
          ".",
      );
    } catch {
      alert("That file isn't a valid Eve backup.");
    } finally {
      setBusy(false);
    }
  };

  return (
    <Section title="Backup & restore" icon={<Database size={16} />}>
      <label className="flex cursor-pointer items-center justify-between gap-3">
        <span className="text-sm text-ink-soft">Include history transcripts</span>
        <input
          type="checkbox"
          checked={includeHistory}
          onChange={(e) => setIncludeHistory(e.target.checked)}
          className="size-4 shrink-0 accent-accent"
        />
      </label>
      <p className="mt-2 text-xs text-ink-faint">
        A single JSON file with your settings (shortcuts, providers,
        privacy list), dictionary, snippets, Flow Styles, transforms, and optionally your
        history. API keys are never included - they stay in the OS keychain.
      </p>
      <div className="mt-3 flex gap-2">
        <button
          onClick={onExport}
          disabled={busy}
          className="rounded-xl bg-accent px-4 py-2 text-sm font-medium text-white hover:opacity-90 disabled:opacity-50"
        >
          Export backup
        </button>
        <button
          onClick={onImport}
          disabled={busy}
          className="rounded-xl border border-border px-4 py-2 text-sm text-ink-soft hover:bg-surface-2 disabled:opacity-50"
        >
          Import backup…
        </button>
      </div>
    </Section>
  );
}

/** Editable list of process names that suppress dictation (Phase 10). */
function PausedAppsEditor({
  apps,
  onChange,
}: {
  apps: string[];
  onChange: (apps: string[]) => void;
}) {
  const [draft, setDraft] = useState("");
  const add = () => {
    const name = draft.trim().toLowerCase();
    if (!name || apps.some((a) => a.toLowerCase() === name)) {
      setDraft("");
      return;
    }
    onChange([...apps, name]);
    setDraft("");
  };
  return (
    <div>
      <div className="flex flex-wrap gap-2">
        {apps.length === 0 && (
          <span className="text-xs text-ink-faint">No apps paused.</span>
        )}
        {apps.map((a) => (
          <span
            key={a}
            className="flex items-center gap-1.5 rounded-full border border-border bg-surface-2 px-3 py-1 text-sm"
          >
            <span className="font-mono text-xs">{a}</span>
            <button
              onClick={() => onChange(apps.filter((x) => x !== a))}
              className="text-ink-faint hover:text-danger"
              title="Remove"
            >
              <X size={13} />
            </button>
          </span>
        ))}
      </div>
      <div className="mt-3 flex gap-2">
        <input
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && add()}
          placeholder={pausedAppExample()}
          className="flex-1 rounded-lg border border-border bg-surface px-3 py-2 text-sm outline-none focus:border-accent"
        />
        <button
          onClick={add}
          disabled={!draft.trim()}
          className="flex items-center gap-1 rounded-lg bg-accent px-3 py-2 text-sm font-medium text-white hover:opacity-90 disabled:opacity-40"
        >
          <Plus size={14} /> Add
        </button>
      </div>
    </div>
  );
}

/** Check the GitHub Releases feed and offer to install (Phase 11). */
function UpdateChecker({ nonce }: { nonce: number }) {
  type UState =
    | { kind: "idle" }
    | { kind: "checking" }
    | { kind: "available"; version: string }
    | { kind: "current" }
    | { kind: "installing" }
    | { kind: "error"; message: string };
  const [state, setState] = useState<UState>({ kind: "idle" });

  const check = async () => {
    setState({ kind: "checking" });
    try {
      const version = await api.checkForUpdate();
      setState(version ? { kind: "available", version } : { kind: "current" });
    } catch (e) {
      setState({ kind: "error", message: String(e) });
    }
  };

  // Re-run when the tray item fires (nonce changes); skip the initial mount.
  useEffect(() => {
    if (nonce > 0) check();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [nonce]);

  const install = async () => {
    setState({ kind: "installing" });
    try {
      const ok = await api.installUpdate();
      if (!ok) setState({ kind: "current" });
      // On success the app relaunches, so no further state update runs.
    } catch (e) {
      setState({ kind: "error", message: String(e) });
    }
  };

  return (
    <div>
      <div className="flex items-center justify-between gap-3">
        <div className="text-sm text-ink-soft">
          {state.kind === "checking" && "Checking for updates…"}
          {state.kind === "current" && "You're on the latest version."}
          {state.kind === "available" && (
            <span className="text-ink">Version {state.version} is available.</span>
          )}
          {state.kind === "installing" && "Downloading & installing…"}
          {state.kind === "error" && (
            <span className="text-danger">Couldn't check — {state.message}</span>
          )}
          {state.kind === "idle" && "Check for a new version of Eve."}
        </div>
        {state.kind === "available" ? (
          <button
            onClick={install}
            className="shrink-0 rounded-xl bg-accent px-4 py-2 text-sm font-medium text-white hover:opacity-90"
          >
            Install & restart
          </button>
        ) : (
          <button
            onClick={check}
            disabled={state.kind === "checking" || state.kind === "installing"}
            className="flex shrink-0 items-center gap-1.5 rounded-xl border border-border px-4 py-2 text-sm text-ink-soft hover:bg-surface-2 disabled:opacity-50"
          >
            <RefreshCw size={14} className={state.kind === "checking" ? "animate-spin" : ""} />
            Check now
          </button>
        )}
      </div>
    </div>
  );
}

/**
 * Phase 2 providers A: per-provider key management plus the cloud LLM routing
 * choices for polish / Command Mode / Transforms. Keys go straight to the OS
 * keychain via `secrets.rs`; provider/model changes hot-swap on the next call.
 */
function ProvidersSection({
  settings,
  persist,
}: {
  settings: Settings;
  persist: (s: Settings) => void;
}) {
  const [keyStatus, setKeyStatus] = useState<Record<string, boolean>>({});
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [flash, setFlash] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  // Phase 3: effective speech provider. An empty `transcriptionProvider` is a
  // legacy install - resolve from the old `transcriptionBackend` field exactly
  // like the Rust router does.
  const speechProvider =
    settings.transcriptionProvider ||
    (settings.transcriptionBackend === "local" ? "local" : "groq");

  // Load each provider's configured state (best-effort).
  useEffect(() => {
    PROVIDERS.forEach((p) => {
      api
        .hasProviderKey(p.id)
        .then((has) => setKeyStatus((s) => ({ ...s, [p.id]: has })))
        .catch(() => {});
    });
  }, []);

  const saveKey = async (id: string) => {
    const key = (drafts[id] ?? "").trim();
    if (!key) return;
    try {
      await api.storeProviderKey(id, key);
    } catch {
      setError("Couldn't save the key. Please try again.");
      return;
    }
    setDrafts((d) => ({ ...d, [id]: "" }));
    setKeyStatus((s) => ({ ...s, [id]: true }));
    setError(null);
    setFlash(id);
    setTimeout(() => setFlash((f) => (f === id ? null : f)), 1500);
  };

  const removeKey = async (id: string) => {
    try {
      await api.clearProviderKey(id);
      setKeyStatus((s) => ({ ...s, [id]: false }));
      setError(null);
    } catch {
      setError("Couldn't remove the key. Please try again.");
    }
  };

  return (
    <Section title="Providers" icon={<KeyRound size={16} />}>
      <p className="mb-4 text-sm text-ink-soft">
        Pick which cloud providers transcribe your speech and clean up your dictations
        (plus Command Mode and Transforms). Keys are stored securely in the Windows
        Credential Manager — never written to disk in plain text.
      </p>

      <div className="grid grid-cols-2 gap-3">
        <div>
          <div className="mb-1.5 text-xs font-medium text-ink-soft">Polish provider</div>
          <Select
            value={settings.polishProvider}
            onChange={(v) => persist({ ...settings, polishProvider: v })}
            options={PROVIDERS.map((p) => ({ value: p.id, label: p.label }))}
          />
        </div>
        <div>
          <div className="mb-1.5 text-xs font-medium text-ink-soft">Fallback provider</div>
          <Select
            value={settings.fallbackPolishProvider}
            onChange={(v) => persist({ ...settings, fallbackPolishProvider: v })}
            options={[
              { value: "", label: "None" },
              ...PROVIDERS.filter((p) => p.id !== settings.polishProvider).map((p) => ({
                value: p.id,
                label: p.label,
              })),
            ]}
          />
        </div>
      </div>
      <input
        type="text"
        value={settings.polishCloudModel}
        onChange={(e) => persist({ ...settings, polishCloudModel: e.target.value })}
        placeholder="Model — leave empty for the provider default"
        className="mt-3 w-full rounded-xl border border-border bg-surface px-3 py-2 outline-none focus:border-accent text-sm font-mono"
      />
      <p className="mt-2 text-xs text-ink-faint">
        On failure Eve tries the fallback provider (if its key is configured), then your local
        polish model. A rejected key is always reported - it never silently falls back.
      </p>

      {/* Phase 3 providers B: cloud speech-to-text routing */}
      <div className="mt-5 border-t border-border pt-4">
        <div className="grid grid-cols-2 gap-3">
          <div>
            <div className="mb-1.5 text-xs font-medium text-ink-soft">Speech provider</div>
            <Select
              value={speechProvider}
              onChange={(v) =>
                persist({ ...settings, transcriptionProvider: v })
              }
              options={[
                ...STT_PROVIDERS.map((p) => ({ value: p.id, label: p.label })),
                { value: "local", label: "Local model" },
              ]}
            />
          </div>
          <div>
            <div className="mb-1.5 text-xs font-medium text-ink-soft">Fallback provider</div>
            <Select
              value={settings.fallbackTranscriptionProvider}
              onChange={(v) =>
                persist({ ...settings, fallbackTranscriptionProvider: v })
              }
              options={[
                { value: "", label: "None" },
                ...STT_PROVIDERS.filter(
                  (p) => speechProvider === "local" || p.id !== speechProvider,
                ).map((p) => ({ value: p.id, label: p.label })),
              ]}
            />
          </div>
        </div>
        <input
          type="text"
          value={settings.transcriptionCloudModel}
          onChange={(e) => persist({ ...settings, transcriptionCloudModel: e.target.value })}
          placeholder={
            speechProvider === "local"
              ? "Local model is picked on the Models page"
              : "Model — leave empty for the provider default"
          }
          disabled={speechProvider === "local"}
          className="mt-3 w-full rounded-xl border border-border bg-surface px-3 py-2 outline-none focus:border-accent text-sm font-mono disabled:opacity-50"
        />
        <p className="mt-2 text-xs text-ink-faint">
          {speechProvider === "local"
            ? "Dictation runs on-device; on failure Eve falls back to your cloud speech provider when its key is set."
            : "On rate limits or connection errors Eve switches to the fallback provider (if its key is configured). Deepgram doesn't support dictionary hints or translate-to-English."}
        </p>
      </div>

      <div className="mt-5 space-y-2 border-t border-border pt-4">
        {PROVIDERS.map((p) => (
          <div key={p.id} className="flex items-center gap-2">
            <span className="w-24 shrink-0 text-sm text-ink">{p.label}</span>
            {keyStatus[p.id] ? (
              <>
                <span className="flex flex-1 items-center gap-1.5 text-sm text-accent">
                  <Check size={15} /> Configured
                </span>
                <button
                  onClick={() => removeKey(p.id)}
                  className="text-xs text-ink-faint underline hover:text-danger"
                >
                  Remove
                </button>
              </>
            ) : (
              <>
                <input
                  type="password"
                  value={drafts[p.id] ?? ""}
                  onChange={(e) => setDrafts((d) => ({ ...d, [p.id]: e.target.value }))}
                  onKeyDown={(e) => e.key === "Enter" && saveKey(p.id)}
                  placeholder={p.placeholder}
                  className="flex-1 rounded-lg border border-border bg-surface px-3 py-1.5 outline-none focus:border-accent text-sm"
                />
                <button
                  onClick={() => saveKey(p.id)}
                  disabled={!(drafts[p.id] ?? "").trim()}
                  className={
                    "shrink-0 rounded-lg px-3 py-1.5 text-sm font-medium " +
                    (flash === p.id
                      ? "bg-accent-soft text-accent"
                      : "bg-accent text-white hover:opacity-90 disabled:opacity-40")
                  }
                >
                  {flash === p.id ? "Saved ✓" : "Save"}
                </button>
              </>
            )}
          </div>
        ))}
      </div>
      {error && <p className="mt-3 text-xs text-danger">{error}</p>}
    </Section>
  );
}

function Section({ title, icon, children }: { title: string; icon?: ReactNode; children: ReactNode }) {
  return (
    <section className="mt-8">
      <h2 className="mb-3 flex items-center gap-2 text-sm font-semibold uppercase tracking-wide text-ink-faint">
        {icon}
        {title}
      </h2>
      <div className="rounded-2xl border border-border bg-surface p-5">{children}</div>
    </section>
  );
}

function Select({
  value,
  onChange,
  options,
}: {
  value: string;
  onChange: (v: string) => void;
  options: { value: string; label: string }[];
}) {
  return (
    <select
      value={value}
      onChange={(e) => onChange(e.target.value)}
      className="w-full rounded-xl border border-border bg-surface px-3 py-2 outline-none focus:border-accent"
    >
      {options.map((o) => (
        <option key={o.value} value={o.value}>
          {o.label}
        </option>
      ))}
    </select>
  );
}

function Range({
  label,
  min,
  max,
  step,
  value,
  format,
  onChange,
}: {
  label: string;
  min: number;
  max: number;
  step: number;
  value: number;
  format: (v: number) => string;
  onChange: (v: number) => void;
}) {
  return (
    <label className="block">
      <div className="mb-1.5 flex items-center justify-between text-sm">
        <span className="text-ink-soft">{label}</span>
        <span className="font-mono text-xs text-ink-faint">{format(value)}</span>
      </div>
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        onChange={(e) => onChange(Number(e.target.value))}
        className="w-full accent-accent"
      />
    </label>
  );
}
