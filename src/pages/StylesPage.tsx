import { useCallback, useEffect, useState } from "react";
import {
  Power,
  Mail,
  MessageSquare,
  MessagesSquare,
  Code2,
  AppWindow,
  Plus,
  Trash2,
  X,
} from "lucide-react";
import { api, type AppCategory, type FlowTone, type FlowStyle } from "../lib/api";

// The grid axes: app categories (rows) × tones (columns).
const CATEGORIES: { value: AppCategory; label: string; icon: typeof Mail; hint: string }[] = [
  { value: "email", label: "Email", icon: Mail, hint: "Outlook, Gmail, Proton…" },
  { value: "workmsg", label: "Work chat", icon: MessageSquare, hint: "Slack, Teams" },
  { value: "personalmsg", label: "Personal chat", icon: MessagesSquare, hint: "WhatsApp, Telegram, Discord" },
  { value: "code", label: "Code", icon: Code2, hint: "VS Code, Cursor, terminals" },
  { value: "other", label: "Everything else", icon: AppWindow, hint: "Default for any other app" },
];

const TONES: { value: FlowTone; label: string; preview: string }[] = [
  { value: "formal", label: "Formal", preview: "I wanted to follow up regarding the proposal." },
  { value: "casual", label: "Casual", preview: "Just following up on the proposal." },
  { value: "very_casual", label: "Very casual", preview: "hey, any word on the proposal?" },
  { value: "excited", label: "Excited", preview: "Can't wait to hear what you think about the proposal!" },
];

const DEFAULT_TONE: FlowTone = "casual";

export function StylesPage() {
  const [styles, setStyles] = useState<FlowStyle[]>([]);
  const [loading, setLoading] = useState(true);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      setStyles(await api.getFlowStyles());
    } catch {
      setStyles([]);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  return (
    <div>
      <h1 className="font-serif text-3xl">Flow Styles</h1>
      <p className="mt-2 max-w-lg text-sm text-ink-soft">
        Eve adapts how it polishes your words to the app you're dictating into. Pick a
        tone per app type, and optionally paste a writing sample so Eve mimics your
        voice. You can also add exact-app styles that win over the category default
        whenever that specific app is focused. Styles only apply when a cleanup level
        above <span className="font-medium text-ink">None</span> is set.
      </p>

      <div className="mt-6 space-y-3">
        {loading ? (
          <p className="text-sm text-ink-faint">Loading…</p>
        ) : (
          CATEGORIES.map((cat) => {
            const forCat = styles.filter((s) => s.appCategory === cat.value);
            const def = forCat.find((s) => s.appProcess === "");
            const overrides = forCat
              .filter((s) => s.appProcess !== "")
              .sort((a, b) => a.appProcess.localeCompare(b.appProcess));
            return (
              <StyleCard
                key={cat.value}
                category={cat}
                style={def}
                overrides={overrides}
                onChanged={load}
              />
            );
          })
        )}
      </div>
    </div>
  );
}

function StyleCard({
  category,
  style,
  overrides,
  onChanged,
}: {
  category: (typeof CATEGORIES)[number];
  style: FlowStyle | undefined;
  overrides: FlowStyle[];
  onChanged: () => void;
}) {
  const Icon = category.icon;
  // Whether the category default (or any override) currently applies.
  const anyActive =
    (style?.isActive ?? true) || overrides.some((o) => o.isActive);
  const [adding, setAdding] = useState(false);
  const [newProcess, setNewProcess] = useState("");

  const addOverride = async () => {
    const proc = newProcess.trim().toLowerCase();
    if (!proc) return;
    await api
      .upsertFlowStyle(
        category.value,
        DEFAULT_TONE,
        "",
        "",
        true,
        proc,
        proc,
      )
      .catch(() => {});
    setNewProcess("");
    setAdding(false);
    onChanged();
  };

  return (
    <div className={"rounded-2xl border border-border bg-surface p-5 " + (anyActive ? "" : "opacity-60")}>
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <div className="flex h-9 w-9 items-center justify-center rounded-xl bg-accent-soft text-ink">
            <Icon size={18} />
          </div>
          <div>
            <div className="font-medium text-ink">{category.label}</div>
            <div className="text-xs text-ink-faint">{category.hint}</div>
          </div>
        </div>
        <button
          onClick={() => setAdding((v) => !v)}
          title="Add a style scoped to one exact app"
          className="flex items-center gap-1 rounded-lg px-2.5 py-1.5 text-xs text-ink-faint hover:text-accent"
        >
          {adding ? <X size={14} /> : <Plus size={14} />} Exact app
        </button>
      </div>

      {adding && (
        <div className="mt-3 flex items-center gap-2 rounded-xl border border-border bg-surface-2 px-3 py-2">
          <input
            autoFocus
            value={newProcess}
            onChange={(e) => setNewProcess(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && addOverride()}
            placeholder="Process name, e.g. slack.exe"
            className="flex-1 bg-transparent text-sm outline-none placeholder:text-ink-faint"
          />
          <button
            onClick={addOverride}
            disabled={!newProcess.trim()}
            className="rounded-lg bg-accent px-3 py-1.5 text-xs font-medium text-white enabled:hover:opacity-90 disabled:opacity-40"
          >
            Add
          </button>
        </div>
      )}

      <StyleFields
        appCategory={category.value}
        appProcess=""
        fallbackLabel={category.label}
        style={style}
        onChanged={onChanged}
      />

      {overrides.length > 0 && (
        <div className="mt-4 space-y-3">
          <div className="text-xs font-medium uppercase tracking-wide text-ink-faint">
            Exact-app styles
          </div>
          {overrides.map((o) => (
            <OverrideBlock key={o.id} style={o} onChanged={onChanged} />
          ))}
        </div>
      )}
    </div>
  );
}

/** One exact-app profile: named block + delete + the same editable fields. */
function OverrideBlock({ style, onChanged }: { style: FlowStyle; onChanged: () => void }) {
  const onDelete = async () => {
    if (!confirm(`Delete the exact-app style for "${style.appProcess}"?`)) return;
    await api.deleteFlowStyle(style.id).catch(() => {});
    onChanged();
  };

  return (
    <div className="rounded-xl border border-border bg-surface-2 p-4">
      <div className="mb-3 flex items-center justify-between gap-2">
        <span className="rounded-md bg-accent-soft px-2 py-0.5 font-mono text-xs text-ink">
          {style.appProcess}
        </span>
        <button
          onClick={onDelete}
          title="Delete this exact-app style"
          className="rounded-lg p-1.5 text-ink-faint hover:bg-surface hover:text-danger"
        >
          <Trash2 size={14} />
        </button>
      </div>
      <StyleFields
        appCategory={style.appCategory as AppCategory}
        appProcess={style.appProcess}
        fallbackLabel={style.appProcess}
        style={style}
        onChanged={onChanged}
      />
    </div>
  );
}

/**
 * The tone grid + sample/instructions editors for one style row, identified by
 * (category, process). Saves through upsert on every interaction; when no row
 * exists yet (undefined style), the first change creates it.
 */
function StyleFields({
  appCategory,
  appProcess,
  fallbackLabel,
  style,
  onChanged,
}: {
  appCategory: AppCategory;
  appProcess: string;
  fallbackLabel: string;
  style: FlowStyle | undefined;
  onChanged: () => void;
}) {
  const tone = style?.tone ?? DEFAULT_TONE;
  const isActive = style?.isActive ?? true;
  const [sample, setSample] = useState(style?.writingSample ?? "");
  const [custom, setCustom] = useState(style?.systemPrompt ?? "");

  // Keep local text in sync when the row is (re)loaded from disk.
  useEffect(() => {
    setSample(style?.writingSample ?? "");
    setCustom(style?.systemPrompt ?? "");
  }, [style?.writingSample, style?.systemPrompt]);

  const save = async (next: {
    tone?: FlowTone;
    isActive?: boolean;
    writingSample?: string;
    systemPrompt?: string;
  }) => {
    await api
      .upsertFlowStyle(
        appCategory,
        next.tone ?? tone,
        next.systemPrompt ?? custom,
        next.writingSample ?? sample,
        next.isActive ?? isActive,
        style?.name || fallbackLabel,
        appProcess,
      )
      .catch(() => {});
    onChanged();
  };

  return (
    <div>
      <div className="mt-3 flex items-center justify-end">
        <button
          onClick={() => save({ isActive: !isActive })}
          title={isActive ? "Disable styling here" : "Enable"}
          className={
            "flex items-center gap-1.5 rounded-lg px-2.5 py-1.5 text-xs " +
            (isActive ? "text-accent" : "text-ink-faint hover:text-ink")
          }
        >
          <Power size={14} />
          {isActive ? "On" : "Off"}
        </button>
      </div>

      <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
        {TONES.map((t) => (
          <button
            key={t.value}
            onClick={() => save({ tone: t.value })}
            title={t.preview}
            className={
              "rounded-xl border px-3 py-2 text-left transition-colors " +
              (tone === t.value
                ? "border-accent bg-accent-soft"
                : "border-border bg-surface hover:border-accent/50")
            }
          >
            <div className="text-sm font-medium text-ink">{t.label}</div>
            <div className="mt-0.5 line-clamp-2 text-[11px] leading-snug text-ink-faint">
              “{t.preview}”
            </div>
          </button>
        ))}
      </div>

      <div className="mt-4 grid gap-3">
        <label>
          <span className="mb-1 block text-xs text-ink-faint">Writing sample (optional)</span>
          <textarea
            value={sample}
            onChange={(e) => setSample(e.target.value)}
            onBlur={() => sample !== (style?.writingSample ?? "") && save({ writingSample: sample })}
            placeholder="Paste a few sentences in your own voice; Eve will imitate them."
            rows={2}
            className="w-full resize-y rounded-lg border border-border bg-surface px-3 py-2 text-sm outline-none focus:border-accent"
          />
        </label>
        <label>
          <span className="mb-1 block text-xs text-ink-faint">Extra instructions (optional)</span>
          <textarea
            value={custom}
            onChange={(e) => setCustom(e.target.value)}
            onBlur={() => custom !== (style?.systemPrompt ?? "") && save({ systemPrompt: custom })}
            placeholder="e.g. Always sign off with “Thanks, Sam.”"
            rows={2}
            className="w-full resize-y rounded-lg border border-border bg-surface px-3 py-2 text-sm outline-none focus:border-accent"
          />
        </label>
      </div>
    </div>
  );
}
