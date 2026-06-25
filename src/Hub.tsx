import { useEffect, useState, type ReactNode } from "react";
import {
  Mic,
  Settings as SettingsIcon,
  LayoutDashboard,
  History,
  BookMarked,
  Sparkles,
  KeyRound,
  Check,
  Moon,
  Sun,
} from "lucide-react";
import { api, DEFAULT_SETTINGS, type Settings, type CleanupLevel } from "./lib/api";
import { HistoryPage } from "./pages/HistoryPage";
import { DictionaryPage } from "./pages/DictionaryPage";

const SHORTCUT_CHOICES = ["F8", "F9", "F10", "CmdOrCtrl+Shift+Space", "Alt+Q"];

const LANGUAGES: { code: string; label: string }[] = [
  { code: "auto", label: "Auto-detect" },
  { code: "en", label: "English" },
  { code: "hi", label: "Hindi" },
  { code: "es", label: "Spanish" },
  { code: "fr", label: "French" },
  { code: "de", label: "German" },
  { code: "it", label: "Italian" },
  { code: "pt", label: "Portuguese" },
  { code: "ja", label: "Japanese" },
  { code: "zh", label: "Chinese" },
];

const CLEANUP: { value: CleanupLevel; label: string; hint: string }[] = [
  { value: "none", label: "None", hint: "Raw transcript, no AI edits" },
  { value: "light", label: "Light", hint: "Fix capitalization/punctuation, drop stray fillers" },
  { value: "medium", label: "Medium", hint: "Remove fillers, fix grammar, resolve self-corrections" },
  { value: "high", label: "High", hint: "Rewrite into clean prose; format spoken lists" },
];

const COPY_SHORTCUT_CHOICES = [
  "CmdOrCtrl+Shift+C",
  "CmdOrCtrl+Shift+V",
  "CmdOrCtrl+Alt+C",
  "Alt+C",
];

type Nav = "dashboard" | "settings" | "history" | "dictionary";

export function Hub() {
  const [nav, setNav] = useState<Nav>("dashboard");
  const [settings, setSettings] = useState<Settings>(DEFAULT_SETTINGS);
  const [hasKey, setHasKey] = useState(false);
  const [dark, setDark] = useState(() => document.documentElement.classList.contains("dark"));

  useEffect(() => {
    api.getSettings().then(setSettings).catch(() => {});
    api.hasApiKey().then(setHasKey).catch(() => {});
  }, []);

  const toggleTheme = () => {
    const next = !dark;
    setDark(next);
    document.documentElement.classList.toggle("dark", next);
  };

  return (
    <div className="flex h-full bg-canvas text-ink">
      {/* Sidebar */}
      <aside className="flex w-60 shrink-0 flex-col gap-1 border-r border-border bg-surface/60 p-4">
        <div className="mb-6 flex items-center gap-2 px-2">
          <div className="flex h-9 w-9 items-center justify-center rounded-2xl bg-accent text-surface">
            <Mic size={18} />
          </div>
          <div>
            <div className="font-serif text-lg leading-none">Eve</div>
            <div className="text-xs text-ink-faint">voice dictation</div>
          </div>
        </div>

        <NavItem icon={<LayoutDashboard size={18} />} label="Dashboard" active={nav === "dashboard"} onClick={() => setNav("dashboard")} />
        <NavItem icon={<History size={18} />} label="History" active={nav === "history"} onClick={() => setNav("history")} />
        <NavItem icon={<BookMarked size={18} />} label="Dictionary" active={nav === "dictionary"} onClick={() => setNav("dictionary")} />
        <NavItem icon={<SettingsIcon size={18} />} label="Settings" active={nav === "settings"} onClick={() => setNav("settings")} />

        <div className="mt-4 mb-1 px-3 text-[11px] font-semibold uppercase tracking-wide text-ink-faint">Coming soon</div>
        <NavItem icon={<Sparkles size={18} />} label="Styles" disabled />

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
          ) : nav === "history" ? (
            <HistoryPage />
          ) : nav === "dictionary" ? (
            <DictionaryPage />
          ) : (
            <SettingsPanel
              settings={settings}
              setSettings={setSettings}
              hasKey={hasKey}
              setHasKey={setHasKey}
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

      <div className="mt-8 grid gap-4">
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
    </div>
  );
}

function SettingsPanel({
  settings,
  setSettings,
  hasKey,
  setHasKey,
}: {
  settings: Settings;
  setSettings: (s: Settings) => void;
  hasKey: boolean;
  setHasKey: (b: boolean) => void;
}) {
  const [apiKey, setApiKey] = useState("");
  const [savedFlash, setSavedFlash] = useState(false);

  const persist = async (next: Settings) => {
    setSettings(next);
    await api.updateSettings(next).catch(() => {});
  };

  const saveKey = async () => {
    if (!apiKey.trim()) return;
    await api.storeApiKey(apiKey.trim());
    setApiKey("");
    setHasKey(true);
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
        {hasKey && (
          <button
            onClick={async () => {
              await api.clearApiKey();
              setHasKey(false);
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

      <Section title="Language">
        <Select
          value={settings.language}
          onChange={(v) => persist({ ...settings, language: v })}
          options={LANGUAGES.map((l) => ({ value: l.code, label: l.label }))}
        />
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
