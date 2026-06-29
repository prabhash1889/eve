import { useEffect, useState, type ReactNode } from "react";
import {
  Settings as SettingsIcon,
  LayoutDashboard,
  History,
  BookMarked,
  Zap,
  Sparkles,
  KeyRound,
  Check,
  Moon,
  Sun,
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
} from "lucide-react";
import { api, DEFAULT_SETTINGS, effectiveLanguage, on, EVT, type Settings, type CleanupLevel, type Stats } from "./lib/api";
import { CLEANUP, SHORTCUT_CHOICES } from "./lib/options";
import { Onboarding, LanguageMultiSelect } from "./components/onboarding/Onboarding";
import { HistoryPage } from "./pages/HistoryPage";
import { DictionaryPage } from "./pages/DictionaryPage";
import { SnippetsPage } from "./pages/SnippetsPage";
import { StylesPage } from "./pages/StylesPage";
import { TransformsPage } from "./pages/TransformsPage";
import { LocalModelsPage } from "./pages/LocalModelsPage";
import { InsightsPage } from "./pages/InsightsPage";

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
  const [dark, setDark] = useState(() => document.documentElement.classList.contains("dark"));

  useEffect(() => {
    api
      .getSettings()
      .then(setSettings)
      .catch(() => {})
      .finally(() => setLoaded(true));
    api.hasApiKey().then(setHasKey).catch(() => {});
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

  const toggleTheme = () => {
    const next = !dark;
    setDark(next);
    document.documentElement.classList.toggle("dark", next);
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
        <NavItem icon={<Sparkles size={18} />} label="Styles" active={nav === "styles"} onClick={() => setNav("styles")} />
        <NavItem icon={<Wand2 size={18} />} label="Transforms" active={nav === "transforms"} onClick={() => setNav("transforms")} />
        <NavItem icon={<NotebookPen size={18} />} label="Scratchpad" onClick={() => api.openScratchpad().catch(() => {})} />
        <NavItem icon={<Cpu size={18} />} label="Local models" active={nav === "models"} onClick={() => setNav("models")} />
        <NavItem icon={<SettingsIcon size={18} />} label="Settings" active={nav === "settings"} onClick={() => setNav("settings")} />

        <div className="mt-auto">
          <button
            onClick={toggleTheme}
            className="flex w-full items-center gap-2 rounded-xl px-3 py-2 text-sm text-ink-soft hover:bg-surface-2"
          >
            {dark ? <Sun size={16} /> : <Moon size={16} />}
            {dark ? "Light mode" : "Dark mode"}
          </button>
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
              hasKey={hasKey}
              setHasKey={setHasKey}
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

function Dashboard({
  settings,
  hasKey,
  onConfigure,
}: {
  settings: Settings;
  hasKey: boolean;
  onConfigure: () => void;
}) {
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
      </div>

      <div className="mt-10">
        <HistoryPage />
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
  hasKey,
  setHasKey,
  updateNonce,
}: {
  settings: Settings;
  setSettings: (s: Settings) => void;
  hasKey: boolean;
  setHasKey: (b: boolean) => void;
  updateNonce: number;
}) {
  const [apiKey, setApiKey] = useState("");
  const [savedFlash, setSavedFlash] = useState(false);
  const [keyError, setKeyError] = useState<string | null>(null);
  const [inputDevices, setInputDevices] = useState<string[]>([]);

  // Enumerate capture devices for the microphone picker (best-effort).
  useEffect(() => {
    api.listInputDevices().then(setInputDevices).catch(() => {});
  }, []);

  const persist = async (next: Settings) => {
    setSettings(next);
    await api.updateSettings(next).catch(() => {});
  };

  const saveKey = async () => {
    if (!apiKey.trim()) return;
    try {
      await api.storeApiKey(apiKey.trim());
    } catch {
      // Surface the failure rather than flashing "Saved ✓" on a rejected store.
      setKeyError("Couldn't save the key. Please try again.");
      return;
    }
    setApiKey("");
    setHasKey(true);
    setKeyError(null);
    setSavedFlash(true);
    setTimeout(() => setSavedFlash(false), 1500);
  };

  return (
    <div>
      <h1 className="font-serif text-3xl">Settings</h1>

      <Section title="Groq API key" icon={<KeyRound size={16} />}>
        <p className="mb-3 text-sm text-ink-soft">
          Stored securely in the Windows Credential Manager — never written to disk in plain text.
        </p>
        <div className="flex gap-2">
          <input
            type="password"
            value={apiKey}
            onChange={(e) => setApiKey(e.target.value)}
            placeholder={hasKey ? "•••••••••••• (configured)" : "gsk_..."}
            className="flex-1 rounded-xl border border-border bg-surface px-3 py-2 outline-none focus:border-accent"
          />
          <button
            onClick={saveKey}
            className="rounded-xl bg-accent px-4 py-2 text-sm font-medium text-white hover:opacity-90"
          >
            {savedFlash ? "Saved ✓" : "Save"}
          </button>
        </div>
        {keyError && <p className="mt-2 text-xs text-danger">{keyError}</p>}
        {hasKey && (
          <button
            onClick={async () => {
              try {
                await api.clearApiKey();
                setHasKey(false);
                setKeyError(null);
              } catch {
                setKeyError("Couldn't remove the key. Please try again.");
              }
            }}
            className="mt-2 text-xs text-ink-faint underline hover:text-danger"
          >
            Remove key
          </button>
        )}
      </Section>

      <Section title="Push-to-talk hotkey">
        <Select
          value={settings.shortcut}
          onChange={async (v) => {
            const next = { ...settings, shortcut: v };
            setSettings(next);
            await api.setShortcut(v).catch(() => {});
          }}
          options={SHORTCUT_CHOICES.map((s) => ({ value: s, label: s }))}
        />
        <p className="mt-2 text-xs text-ink-faint">Hold this key to record; release to transcribe.</p>
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
        <p className="mt-3 text-xs text-ink-faint">
          Pick one language to lock transcription to it, or several (or Auto-detect) to let
          Eve detect per dictation.
        </p>
      </Section>

      <Section title="Cleanup level">
        <Select
          value={settings.cleanupLevel}
          onChange={(v) => persist({ ...settings, cleanupLevel: v as CleanupLevel })}
          options={CLEANUP.map((c) => ({ value: c.value, label: c.label }))}
        />
        <p className="mt-2 text-xs text-ink-faint">
          {CLEANUP.find((c) => c.value === settings.cleanupLevel)?.hint}
          {settings.cleanupLevel !== "none" && " · uses Groq Llama (needs your API key)"}
        </p>
      </Section>

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
          nothing selected, it generates text at your cursor. Uses Groq Llama (needs your
          API key).
        </p>
      </Section>

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

      <Section title="Audio storage" icon={<History size={16} />}>
        <Select
          value={settings.audioStoragePolicy}
          onChange={(v) =>
            persist({ ...settings, audioStoragePolicy: v as Settings["audioStoragePolicy"] })
          }
          options={[
            { value: "store", label: "Keep recordings" },
            { value: "delete24h", label: "Auto-delete after a while" },
            { value: "never", label: "Don't save audio" },
          ]}
        />
        {settings.audioStoragePolicy === "delete24h" && (
          <div className="mt-4">
            <Range
              label="Keep for"
              min={1}
              max={168}
              step={1}
              value={settings.audioRetentionHours}
              format={(v) => `${v} h`}
              onChange={(v) => persist({ ...settings, audioRetentionHours: Math.round(v) })}
            />
          </div>
        )}
        <p className="mt-3 text-xs text-ink-faint">
          Recordings let you replay a dictation from History. They're pruned on launch when
          auto-delete is on. Transcript text is always kept.
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
            Dictation is suppressed when one of these apps is focused. Use the executable name
            (e.g. <span className="font-mono">1password.exe</span>).
          </p>
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
              setSettings({ ...settings, launchAtStartup: enabled });
              await api.setAutostart(enabled).catch(() => {});
            }}
            className="size-4 shrink-0 accent-accent"
          />
        </label>

        <div className="mt-5 border-t border-border pt-4">
          <UpdateChecker nonce={updateNonce} />
        </div>
      </Section>
    </div>
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
          placeholder="app.exe"
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
