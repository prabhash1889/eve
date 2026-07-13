import { useCallback, useEffect, useState } from "react";
import { Download, Trash2, Check, Mic, Sparkles, Loader2, X, AlertTriangle } from "lucide-react";
import {
  api,
  on,
  EVT,
  type Settings,
  type ModelStatus,
  type ModelBackend,
  type LocalProfile,
  type ModelProgressPayload,
  type ModelStatusPayload,
  type TranscriptionBenchmark,
  type WhisperStatus,
} from "../lib/api";

function fmtBytes(n: number): string {
  if (n >= 1e9) return (n / 1e9).toFixed(1) + " GB";
  if (n >= 1e6) return Math.round(n / 1e6) + " MB";
  return Math.round(n / 1e3) + " KB";
}

/** Performance profiles → recommended speech model ids + a short blurb. */
const PROFILES: { value: LocalProfile; label: string; blurb: string; recommends: string[] }[] = [
  {
    value: "fast",
    label: "Fast",
    blurb: "Lowest latency. Aggressive silence trimming, smallest models.",
    recommends: ["whisper-tiny.en", "whisper-base.en"],
  },
  {
    value: "balanced",
    label: "Balanced",
    blurb: "A good speed/quality tradeoff for everyday dictation.",
    recommends: ["whisper-small.en", "parakeet-tdt-0.6b-v2"],
  },
  {
    value: "accurate",
    label: "Accurate",
    blurb: "Highest quality. Gentler trimming, largest model.",
    recommends: ["whisper-large-v3-turbo", "parakeet-tdt-0.6b-v2"],
  },
];

function recommendedModelIds(profile: LocalProfile, language: string): string[] {
  if (language !== "en" && profile !== "fast") return ["whisper-large-v3-turbo"];
  return PROFILES.find((p) => p.value === profile)?.recommends ?? ["whisper-small.en"];
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
  // Phase 2: readiness of the selected local Whisper model (loaded / loading).
  const [whisperStatus, setWhisperStatus] = useState<WhisperStatus | null>(null);
  const [benchmark, setBenchmark] = useState<TranscriptionBenchmark | null>(null);

  const load = useCallback(async () => {
    try {
      setModels(await api.listModels());
    } catch {
      setModels([]);
    } finally {
      setLoading(false);
    }
  }, []);

  const refreshStatus = useCallback(async () => {
    setWhisperStatus(await api.getLocalWhisperStatus().catch(() => null));
    setBenchmark(await api.getLocalTranscriptionBenchmark().catch(() => null));
  }, []);

  useEffect(() => {
    load();
    refreshStatus();
  }, [load, refreshStatus]);

  // While a model is loading, poll readiness so the badge flips to "Ready".
  useEffect(() => {
    if (!whisperStatus?.loading) return;
    const t = window.setInterval(refreshStatus, 500);
    return () => window.clearInterval(t);
  }, [whisperStatus?.loading, refreshStatus]);

  // Prewarm the selected local model (best-effort), then refresh readiness.
  // Honors the prewarm-enabled setting so a user who opted out isn't surprised
  // by a cold load happening on switch.
  const prewarm = useCallback(async () => {
    if (!settings.localPrewarmEnabled) return;
    await api.prewarmLocalModel().catch(() => {});
    refreshStatus();
  }, [refreshStatus, settings.localPrewarmEnabled]);

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
      subs.forEach((s) => s.then((un) => un()).catch(() => {}));
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

  const setBackend = async (
    key: "transcriptionBackend" | "polishBackend",
    value: ModelBackend,
  ) => {
    await persist({ ...settings, [key]: value });
    // Switching speech-to-text to local → prewarm the selected model now so the
    // first dictation isn't slowed by a cold load.
    if (key === "transcriptionBackend" && value === "local") prewarm();
  };

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
    // Selecting a speech model while local STT is active → prewarm the new pick.
    if (m.kind === "whisper" && settings.transcriptionBackend === "local") prewarm();
  };

  const whisper = models.filter((m) => m.kind === "whisper");
  const llm = models.filter((m) => m.kind === "llm");

  // Warn when a local backend is chosen but no downloaded model is active.
  const needsWhisper =
    settings.transcriptionBackend === "local" &&
    !whisper.some((m) => m.active && m.installed);
  const needsLlm =
    settings.polishBackend === "local" && !llm.some((m) => m.active && m.installed);

  const activeProfile =
    PROFILES.find((p) => p.value === settings.localTranscriptionProfile) ?? PROFILES[1];
  const recommended = new Set(
    recommendedModelIds(settings.localTranscriptionProfile, settings.language),
  );
  // Nudge when the selected speech model isn't one the active profile suggests.
  const offProfile =
    settings.transcriptionBackend === "local" &&
    !!settings.localWhisperModel &&
    !recommended.has(settings.localWhisperModel);

  // Loud warning for the worst-case combo: the large model on a CPU build, where
  // it runs ~12–25s per clip. (The backend label reads "whisper.cpp CUDA" on a
  // GPU build, where the large model is fast and this doesn't apply.)
  const onCpuBuild = !!whisperStatus?.backend?.includes("CPU");
  const heavyOnCpu =
    settings.transcriptionBackend === "local" &&
    onCpuBuild &&
    settings.localWhisperModel === "whisper-large-v3-turbo";

  const setProfile = (value: LocalProfile) => persist({ ...settings, localTranscriptionProfile: value });

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

      {/* Performance profile + tuning (optimization Phase 4) */}
      {settings.transcriptionBackend === "local" && (
        <section className="mt-8">
          <h2 className="mb-3 text-sm font-semibold uppercase tracking-wide text-ink-faint">
            Performance
          </h2>
          <div className="space-y-4 rounded-2xl border border-border bg-surface p-5">
            {heavyOnCpu && (
              <div className="flex items-start gap-2 rounded-xl border border-amber-500/40 bg-amber-500/10 p-3 text-xs text-amber-600 dark:text-amber-400">
                <AlertTriangle size={14} className="mt-0.5 shrink-0" />
                <span>
                  Large v3 Turbo runs on the CPU in this build (~12–25s per clip).
                  For fast dictation, switch to Whisper Small (English) below, or
                  rebuild with CUDA for GPU acceleration.
                </span>
              </div>
            )}
            <div>
              <div className="mb-2 text-sm font-medium text-ink">Profile</div>
              <div className="flex flex-wrap gap-1 rounded-xl border border-border bg-canvas p-1">
                {PROFILES.map((p) => (
                  <button
                    key={p.value}
                    onClick={() => setProfile(p.value)}
                    className={
                      "rounded-lg px-3 py-1.5 text-sm transition-colors " +
                      (settings.localTranscriptionProfile === p.value
                        ? "bg-accent-soft text-ink"
                        : "text-ink-faint hover:text-ink")
                    }
                  >
                    {p.label}
                  </button>
                ))}
              </div>
              <p className="mt-2 text-xs text-ink-faint">{activeProfile.blurb}</p>
              {offProfile && (
                <div className="mt-1 flex items-center gap-1 text-xs text-amber-500">
                  <AlertTriangle size={12} />
                  Your selected model isn't recommended for this profile.
                </div>
              )}
            </div>

            <ToggleRow
              label="Trim silence before transcribing"
              hint="Voice-activity detection cuts leading/trailing silence to speed up local inference."
              checked={settings.localVadEnabled}
              onChange={(v) => persist({ ...settings, localVadEnabled: v })}
            />
            <ToggleRow
              label="Beam search for quality"
              hint="Off = greedy decoding (fastest, the default). On adds beam search to the balanced profile. Accurate always uses it; fast always stays greedy."
              checked={settings.localBeamSearchEnabled}
              onChange={(v) => persist({ ...settings, localBeamSearchEnabled: v })}
            />
            <ToggleRow
              label="Correctness rescue"
              hint="Gentler trimming, quieter normalization, beam search, and large-v3-turbo when downloaded."
              checked={settings.localCorrectnessRescue}
              onChange={(v) => persist({ ...settings, localCorrectnessRescue: v })}
            />
            <ToggleRow
              label="Prewarm model on switch"
              hint="Load the selected model into memory when you switch to local, so the first dictation isn't slowed by a cold load."
              checked={settings.localPrewarmEnabled}
              onChange={(v) => persist({ ...settings, localPrewarmEnabled: v })}
            />

            <StatusPanel
              backend={settings.transcriptionBackend}
              model={settings.localWhisperModel}
              status={whisperStatus}
              benchmark={benchmark}
            />
          </div>
        </section>
      )}

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
        recommended={recommended}
        status={
          settings.transcriptionBackend === "local" ? (
            <WhisperReadiness status={whisperStatus} />
          ) : undefined
        }
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

function WhisperReadiness({ status }: { status: WhisperStatus | null }) {
  if (!status || !status.model) return null;
  if (status.loading)
    return (
      <span className="flex items-center gap-1.5 text-xs text-ink-faint">
        <Loader2 size={12} className="animate-spin" />
        Loading model…
      </span>
    );
  if (status.ready)
    return (
      <span className="flex items-center gap-1.5 text-xs text-accent">
        <Check size={12} />
        Model ready
        {status.lastLoadMs != null && (
          <span className="text-ink-faint">· loaded in {(status.lastLoadMs / 1000).toFixed(1)}s</span>
        )}
      </span>
    );
  return <span className="text-xs text-ink-faint">Model loads on first use</span>;
}

function ToggleRow({
  label,
  hint,
  checked,
  onChange,
}: {
  label: string;
  hint: string;
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <div className="flex items-start justify-between gap-3">
      <div className="max-w-md">
        <div className="text-sm font-medium text-ink">{label}</div>
        <div className="mt-0.5 text-xs text-ink-faint">{hint}</div>
      </div>
      <button
        role="switch"
        aria-checked={checked}
        onClick={() => onChange(!checked)}
        className={
          "relative mt-0.5 h-6 w-11 shrink-0 rounded-full transition-colors " +
          (checked ? "bg-accent" : "bg-canvas border border-border")
        }
      >
        <span
          className={
            "absolute left-0.5 top-0.5 h-5 w-5 rounded-full bg-white shadow transition-transform " +
            (checked ? "translate-x-5" : "translate-x-0")
          }
        />
      </button>
    </div>
  );
}

/** Small panel: backend, selected model, readiness, and last local timing. */
function StatusPanel({
  backend,
  model,
  status,
  benchmark,
}: {
  backend: ModelBackend;
  model: string;
  status: WhisperStatus | null;
  benchmark: TranscriptionBenchmark | null;
}) {
  const readiness = !model
    ? "No model selected"
    : status?.loading
      ? "Loading…"
      : status?.ready
        ? "Ready"
        : "Loads on first use";
  return (
    <div className="grid grid-cols-2 gap-x-6 gap-y-1.5 rounded-xl border border-border bg-canvas p-4 text-xs sm:grid-cols-[auto_minmax(0,1fr)_auto_auto]">
      <StatusCell label="Backend" value={backend === "local" ? (status?.backend ?? "Local") : "Groq"} />
      <StatusCell label="Model" value={model || "—"} />
      <StatusCell label="Readiness" value={readiness} />
      <StatusCell
        label="Last transcribe"
        value={
          status?.lastTranscribeMs != null
            ? `${(status.lastTranscribeMs / 1000).toFixed(1)}s`
            : "—"
        }
      />
      {benchmark && (
        <>
          <StatusCell label="Last mode" value={benchmark.mode} />
          <StatusCell label="Last clip" value={`${(benchmark.clipDurationMs / 1000).toFixed(1)}s`} />
          <StatusCell label="Words" value={String(benchmark.wordsProduced)} />
          <StatusCell label="VAD trimmed" value={benchmark.vadTrimmed ? "Yes" : "No"} />
        </>
      )}
    </div>
  );
}

function StatusCell({ label, value }: { label: string; value: string }) {
  return (
    <div className="min-w-0">
      <div className="text-ink-faint">{label}</div>
      <div className="truncate font-medium text-ink" title={value}>
        {value}
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
  status,
  recommended,
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
  status?: React.ReactNode;
  recommended?: Set<string>;
}) {
  return (
    <section className="mt-8">
      <h2 className="mb-3 flex items-center gap-2 text-sm font-semibold uppercase tracking-wide text-ink-faint">
        {icon}
        {title}
        {status && <span className="ml-2 normal-case tracking-normal">{status}</span>}
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
              recommended={!!recommended?.has(m.id)}
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
  recommended,
}: {
  model: ModelStatus;
  progress?: Progress;
  error?: string;
  onDownload: () => void;
  onCancel: () => void;
  onDelete: () => void;
  onSelect: () => void;
  recommended?: boolean;
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
          <div className="flex items-center gap-2">
            <span className="font-medium text-ink">{model.name}</span>
            {recommended && (
              <span className="rounded-full bg-accent-soft px-2 py-0.5 text-[10px] font-medium uppercase tracking-wide text-accent">
                Recommended
              </span>
            )}
          </div>
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
