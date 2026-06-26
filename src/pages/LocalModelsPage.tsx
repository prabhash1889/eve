import { useCallback, useEffect, useState } from "react";
import { Download, Trash2, Check, Mic, Sparkles, Loader2, X, AlertTriangle } from "lucide-react";
import {
  api,
  on,
  EVT,
  type Settings,
  type ModelStatus,
  type ModelBackend,
  type ModelProgressPayload,
  type ModelStatusPayload,
} from "../lib/api";

function fmtBytes(n: number): string {
  if (n >= 1e9) return (n / 1e9).toFixed(1) + " GB";
  if (n >= 1e6) return Math.round(n / 1e6) + " MB";
  return Math.round(n / 1e3) + " KB";
}

type Progress = { downloaded: number; total: number };

/** Return a copy of `map` without the given key. */
function drop<T>(map: Record<string, T>, key: string): Record<string, T> {
  const next = { ...map };
  delete next[key];
  return next;
}

export function LocalModelsPage({
  settings,
  setSettings,
}: {
  settings: Settings;
  setSettings: (s: Settings) => void;
}) {
  const [models, setModels] = useState<ModelStatus[]>([]);
  const [loading, setLoading] = useState(true);
  const [progress, setProgress] = useState<Record<string, Progress>>({});
  const [errors, setErrors] = useState<Record<string, string>>({});

  const load = useCallback(async () => {
    try {
      setModels(await api.listModels());
    } catch {
      setModels([]);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  // Live download lifecycle events from Rust.
  useEffect(() => {
    const subs = [
      on<ModelProgressPayload>(EVT.modelProgress, (e) =>
        setProgress((prev) => ({
          ...prev,
          [e.payload.id]: { downloaded: e.payload.downloaded, total: e.payload.total },
        })),
      ),
      on<ModelStatusPayload>(EVT.modelDone, (e) => {
        clearTransient(e.payload.id);
        load();
      }),
      on<ModelStatusPayload>(EVT.modelError, (e) => {
        setProgress((prev) => drop(prev, e.payload.id));
        if (e.payload.message)
          setErrors((prev) => ({ ...prev, [e.payload.id]: e.payload.message as string }));
        load();
      }),
    ];
    return () => {
      subs.forEach((s) => s.then((un) => un()));
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [load]);

  const clearTransient = (id: string) => {
    setProgress((prev) => drop(prev, id));
    setErrors((prev) => drop(prev, id));
  };

  const persist = async (next: Settings) => {
    setSettings(next);
    await api.updateSettings(next).catch(() => {});
  };

  const setBackend = (key: "transcriptionBackend" | "polishBackend", value: ModelBackend) =>
    persist({ ...settings, [key]: value });

  const download = async (id: string) => {
    clearTransient(id);
    setProgress((prev) => ({ ...prev, [id]: { downloaded: 0, total: 0 } }));
    await api.downloadModel(id).catch((e) => {
      setProgress((prev) => drop(prev, id));
      setErrors((prev) => ({ ...prev, [id]: String(e) }));
    });
  };

  const cancel = (id: string) => api.cancelModelDownload(id).catch(() => {});

  const remove = async (id: string) => {
    await api.deleteModel(id).catch(() => {});
    load();
  };

  const selectModel = async (m: ModelStatus) => {
    const next: Settings =
      m.kind === "whisper"
        ? { ...settings, localWhisperModel: m.id }
        : { ...settings, localLlmModel: m.id };
    await persist(next);
    load();
  };

  const whisper = models.filter((m) => m.kind === "whisper");
  const llm = models.filter((m) => m.kind === "llm");

  // Warn when a local backend is chosen but no downloaded model is active.
  const needsWhisper =
    settings.transcriptionBackend === "local" &&
    !whisper.some((m) => m.active && m.installed);
  const needsLlm =
    settings.polishBackend === "local" && !llm.some((m) => m.active && m.installed);

  return (
    <div>
      <h1 className="font-serif text-3xl">Local models</h1>
      <p className="mt-2 max-w-lg text-sm text-ink-soft">
        Run speech-to-text and AI polish on your own machine — fully offline, no audio or
        text leaves your computer. Downloaded models are stored locally. If a local model
        fails, Eve falls back to Groq when an API key is set.
      </p>

      {/* Backend selectors */}
      <section className="mt-8">
        <h2 className="mb-3 text-sm font-semibold uppercase tracking-wide text-ink-faint">
          Backends
        </h2>
        <div className="space-y-3 rounded-2xl border border-border bg-surface p-5">
          <BackendRow
            label="Speech-to-text"
            value={settings.transcriptionBackend}
            onChange={(v) => setBackend("transcriptionBackend", v)}
            warn={needsWhisper ? "Select a downloaded speech model below." : undefined}
          />
          <BackendRow
            label="AI polish"
            value={settings.polishBackend}
            onChange={(v) => setBackend("polishBackend", v)}
            warn={needsLlm ? "Select a downloaded polish model below." : undefined}
          />
        </div>
      </section>

      {/* Model catalogs */}
      <ModelSection
        title="Speech models"
        icon={<Mic size={16} />}
        models={whisper}
        loading={loading}
        progress={progress}
        errors={errors}
        onDownload={download}
        onCancel={cancel}
        onDelete={remove}
        onSelect={selectModel}
      />
      <ModelSection
        title="Polish models"
        icon={<Sparkles size={16} />}
        models={llm}
        loading={loading}
        progress={progress}
        errors={errors}
        onDownload={download}
        onCancel={cancel}
        onDelete={remove}
        onSelect={selectModel}
      />
    </div>
  );
}

function BackendRow({
  label,
  value,
  onChange,
  warn,
}: {
  label: string;
  value: ModelBackend;
  onChange: (v: ModelBackend) => void;
  warn?: string;
}) {
  const options: { value: ModelBackend; label: string }[] = [
    { value: "groq", label: "Groq (cloud)" },
    { value: "local", label: "Local" },
  ];
  return (
    <div className="flex flex-wrap items-center justify-between gap-3">
      <div>
        <div className="text-sm font-medium text-ink">{label}</div>
        {warn && (
          <div className="mt-0.5 flex items-center gap-1 text-xs text-amber-500">
            <AlertTriangle size={12} />
            {warn}
          </div>
        )}
      </div>
      <div className="flex gap-1 rounded-xl border border-border bg-canvas p-1">
        {options.map((o) => (
          <button
            key={o.value}
            onClick={() => onChange(o.value)}
            className={
              "rounded-lg px-3 py-1.5 text-sm transition-colors " +
              (value === o.value ? "bg-accent-soft text-ink" : "text-ink-faint hover:text-ink")
            }
          >
            {o.label}
          </button>
        ))}
      </div>
    </div>
  );
}

function ModelSection({
  title,
  icon,
  models,
  loading,
  progress,
  errors,
  onDownload,
  onCancel,
  onDelete,
  onSelect,
}: {
  title: string;
  icon: React.ReactNode;
  models: ModelStatus[];
  loading: boolean;
  progress: Record<string, Progress>;
  errors: Record<string, string>;
  onDownload: (id: string) => void;
  onCancel: (id: string) => void;
  onDelete: (id: string) => void;
  onSelect: (m: ModelStatus) => void;
}) {
  return (
    <section className="mt-8">
      <h2 className="mb-3 flex items-center gap-2 text-sm font-semibold uppercase tracking-wide text-ink-faint">
        {icon}
        {title}
      </h2>
      <div className="space-y-3">
        {loading ? (
          <p className="text-sm text-ink-faint">Loading…</p>
        ) : (
          models.map((m) => (
            <ModelCard
              key={m.id}
              model={m}
              progress={progress[m.id]}
              error={errors[m.id]}
              onDownload={() => onDownload(m.id)}
              onCancel={() => onCancel(m.id)}
              onDelete={() => onDelete(m.id)}
              onSelect={() => onSelect(m)}
            />
          ))
        )}
      </div>
    </section>
  );
}

function ModelCard({
  model,
  progress,
  error,
  onDownload,
  onCancel,
  onDelete,
  onSelect,
}: {
  model: ModelStatus;
  progress?: Progress;
  error?: string;
  onDownload: () => void;
  onCancel: () => void;
  onDelete: () => void;
  onSelect: () => void;
}) {
  const downloading = model.downloading || !!progress;
  const pct = progress && progress.total > 0 ? (progress.downloaded / progress.total) * 100 : 0;

  return (
    <div
      className={
        "rounded-2xl border bg-surface p-5 " +
        (model.active && model.installed ? "border-accent" : "border-border")
      }
    >
      <div className="flex items-center justify-between gap-3">
        <div>
          <div className="font-medium text-ink">{model.name}</div>
          <div className="text-xs text-ink-faint">{fmtBytes(model.sizeBytes)}</div>
        </div>

        <div className="flex items-center gap-2">
          {downloading ? (
            <button
              onClick={onCancel}
              className="flex items-center gap-1.5 rounded-lg px-2.5 py-1.5 text-xs text-ink-faint hover:text-ink"
            >
              <X size={14} />
              Cancel
            </button>
          ) : model.installed ? (
            <>
              {model.active ? (
                <span className="flex items-center gap-1.5 rounded-lg px-2.5 py-1.5 text-xs text-accent">
                  <Check size={14} />
                  In use
                </span>
              ) : (
                <button
                  onClick={onSelect}
                  className="rounded-lg border border-border px-2.5 py-1.5 text-xs text-ink-soft hover:border-accent/50"
                >
                  Use
                </button>
              )}
              <button
                onClick={onDelete}
                title="Delete from disk"
                className="flex items-center gap-1.5 rounded-lg px-2.5 py-1.5 text-xs text-ink-faint hover:text-red-500"
              >
                <Trash2 size={14} />
              </button>
            </>
          ) : (
            <button
              onClick={onDownload}
              className="flex items-center gap-1.5 rounded-lg bg-accent-soft px-3 py-1.5 text-xs text-ink hover:bg-accent-soft/70"
            >
              <Download size={14} />
              Download
            </button>
          )}
        </div>
      </div>

      {downloading && (
        <div className="mt-3">
          <div className="h-1.5 w-full overflow-hidden rounded-full bg-canvas">
            <div
              className="h-full rounded-full bg-accent transition-[width]"
              style={{ width: `${pct}%` }}
            />
          </div>
          <div className="mt-1 flex items-center gap-1.5 text-[11px] text-ink-faint">
            <Loader2 size={11} className="animate-spin" />
            {progress && progress.total > 0
              ? `${fmtBytes(progress.downloaded)} / ${fmtBytes(progress.total)} (${Math.round(pct)}%)`
              : "Starting…"}
          </div>
        </div>
      )}

      {error && !downloading && (
        <div className="mt-2 text-xs text-red-500">{error}</div>
      )}
    </div>
  );
}
