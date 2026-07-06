import { useEffect, useRef, useState, type ReactNode } from "react";
import {
  ArrowRight,
  ArrowLeft,
  Check,
  Cloud,
  Download,
  KeyRound,
  Keyboard,
  Languages,
  Loader2,
  Lock,
  Mic,
  Sparkles,
  ShieldCheck,
  X,
} from "lucide-react";
import {
  api,
  on,
  EVT,
  effectiveLanguage,
  type CleanupLevel,
  type ModelProgressPayload,
  type ModelStatus,
  type ModelStatusPayload,
  type Settings,
} from "../../lib/api";
import { LANGUAGES, CLEANUP, SHORTCUT_CHOICES } from "../../lib/options";

/** Parity Phase B: which transcription path onboarding sets up. */
type SetupMode = "cloud" | "private";

/**
 * Phase 10 first-run onboarding. Shown over the Hub while
 * `settings.onboardingComplete` is false. Forks on Cloud (Groq key) vs Private
 * (download a local Whisper model), then walks through hotkey, languages, a
 * live mic test, and cleanup level, and persists everything via `onComplete`.
 */
export function Onboarding({
  settings,
  onComplete,
}: {
  settings: Settings;
  onComplete: (next: Settings) => void;
}) {
  const [step, setStep] = useState(0);
  const [draft, setDraft] = useState<Settings>(settings);
  const [mode, setMode] = useState<SetupMode>("cloud");
  const [apiKey, setApiKey] = useState("");
  const [hasKey, setHasKey] = useState(false);
  const [finishError, setFinishError] = useState<string | null>(null);

  useEffect(() => {
    api.hasApiKey().then(setHasKey).catch(() => {});
    // Mark onboarding as seen the moment it first appears, so first-run is
    // truly once-only: even if the user closes the window before reaching the
    // final step, it won't reappear on the next launch. `finish()` re-saves the
    // full draft when they complete the flow normally.
    api.updateSettings({ ...settings, onboardingComplete: true }).catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Step 2 forks on the chosen mode; both branches have the same length so the
  // current index stays valid when the user flips the choice and navigates.
  const steps =
    mode === "private"
      ? ["Welcome", "Transcription", "Local model", "Hotkey", "Languages", "Mic check", "Cleanup"]
      : ["Welcome", "Transcription", "API key", "Hotkey", "Languages", "Mic check", "Cleanup"];
  const last = steps.length - 1;

  const next = () => setStep((s) => Math.min(last, s + 1));
  const back = () => setStep((s) => Math.max(0, s - 1));

  const pickMode = (m: SetupMode) => {
    setMode(m);
    setDraft((d) => ({ ...d, transcriptionBackend: m === "private" ? "local" : "groq" }));
  };

  const finish = async () => {
    const final: Settings = {
      ...draft,
      language: effectiveLanguage(draft.languages),
      onboardingComplete: true,
    };
    try {
      await api.setShortcut(final.shortcut);
    } catch {
      /* keep going even if the accelerator is taken */
    }
    try {
      await api.updateSettings(final);
    } catch {
      // Don't claim onboarding finished if we couldn't persist the choices.
      setFinishError("Couldn't save your settings. Please try again.");
      return;
    }
    // Private mode: warm the downloaded model so the first dictation isn't
    // slowed by a cold load. Best-effort, mirrors LocalModelsPage.
    if (
      final.transcriptionBackend === "local" &&
      final.localWhisperModel &&
      final.localPrewarmEnabled
    ) {
      api.prewarmLocalModel().catch(() => {});
    }
    onComplete(final);
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-canvas/95 backdrop-blur">
      <div className="flex max-h-[88vh] w-full max-w-lg flex-col overflow-hidden rounded-3xl border border-border bg-surface shadow-2xl">
        {/* Progress dots */}
        <div className="flex items-center gap-1.5 px-8 pt-6">
          {steps.map((_, i) => (
            <span
              key={i}
              className={
                "h-1.5 flex-1 rounded-full transition-colors " +
                (i <= step ? "bg-accent" : "bg-surface-2")
              }
            />
          ))}
        </div>

        <div className="flex-1 overflow-y-auto px-8 py-7">
          {step === 0 && <Welcome />}
          {step === 1 && <ModeStep mode={mode} onPick={pickMode} />}
          {step === 2 && mode === "cloud" && (
            <ApiKeyStep
              hasKey={hasKey}
              apiKey={apiKey}
              setApiKey={setApiKey}
              onSaved={() => setHasKey(true)}
            />
          )}
          {step === 2 && mode === "private" && (
            <LocalModelStep
              selected={draft.localWhisperModel}
              onSelect={(localWhisperModel) =>
                setDraft((d) => ({ ...d, localWhisperModel }))
              }
            />
          )}
          {step === 3 && (
            <HotkeyStep
              value={draft.shortcut}
              onChange={(shortcut) => setDraft((d) => ({ ...d, shortcut }))}
            />
          )}
          {step === 4 && (
            <LanguageStep
              value={draft.languages}
              onChange={(languages) => setDraft((d) => ({ ...d, languages }))}
            />
          )}
          {step === 5 && <MicCheckStep />}
          {step === 6 && (
            <CleanupStep
              value={draft.cleanupLevel}
              hasKey={hasKey}
              onChange={(cleanupLevel) => setDraft((d) => ({ ...d, cleanupLevel }))}
            />
          )}
        </div>

        {/* Footer nav */}
        {finishError && (
          <p className="border-t border-border px-8 pt-3 text-xs text-danger">{finishError}</p>
        )}
        <div className="flex items-center justify-between border-t border-border px-8 py-4">
          <button
            onClick={back}
            disabled={step === 0}
            className="flex items-center gap-1 rounded-xl px-3 py-2 text-sm text-ink-soft hover:bg-surface-2 disabled:opacity-0"
          >
            <ArrowLeft size={15} /> Back
          </button>
          <span className="text-xs text-ink-faint">
            {step + 1} / {steps.length}
          </span>
          {step < last ? (
            <button
              onClick={next}
              className="flex items-center gap-1 rounded-xl bg-accent px-4 py-2 text-sm font-medium text-white hover:opacity-90"
            >
              Next <ArrowRight size={15} />
            </button>
          ) : (
            <button
              onClick={finish}
              className="flex items-center gap-1 rounded-xl bg-accent px-4 py-2 text-sm font-medium text-white hover:opacity-90"
            >
              <Check size={15} /> Finish
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

function StepHeader({
  icon,
  title,
  subtitle,
}: {
  icon: ReactNode;
  title: string;
  subtitle: string;
}) {
  return (
    <div className="mb-5">
      <div className="mb-3 flex size-11 items-center justify-center rounded-2xl bg-accent-soft text-accent">
        {icon}
      </div>
      <h2 className="font-serif text-2xl">{title}</h2>
      <p className="mt-1 text-sm text-ink-soft">{subtitle}</p>
    </div>
  );
}

function Welcome() {
  return (
    <div>
      <div className="mb-4 flex items-center gap-3">
        <img
          src="/image-src/logo/logo.png"
          alt="Eve logo"
          className="h-12 w-12 rounded-2xl object-cover"
        />
        <div>
          <h2 className="font-serif text-2xl">Welcome to Eve</h2>
          <p className="text-sm text-ink-soft">Let's get you set up in under a minute.</p>
        </div>
      </div>
      <p className="text-ink-soft">
        Eve turns your voice into text anywhere on your computer. Hold a hotkey, speak, and
        release — Eve transcribes, cleans up, and types it into whatever app you're using.
      </p>
      <ul className="mt-4 space-y-2 text-sm text-ink-soft">
        {[
          "Choose cloud or private transcription",
          "Pick a push-to-talk hotkey",
          "Choose your languages",
          "Test your microphone",
        ].map((t) => (
          <li key={t} className="flex items-center gap-2">
            <Check size={15} className="text-accent" /> {t}
          </li>
        ))}
      </ul>
    </div>
  );
}

/** Parity Phase B: Cloud (Groq, fast setup) vs Private (on-device) fork. */
function ModeStep({ mode, onPick }: { mode: SetupMode; onPick: (m: SetupMode) => void }) {
  const options: {
    value: SetupMode;
    icon: ReactNode;
    title: string;
    blurb: string;
  }[] = [
    {
      value: "cloud",
      icon: <Cloud size={18} />,
      title: "Cloud",
      blurb:
        "Fast setup and fastest transcription via Groq. Needs a free API key; audio is sent to Groq while you dictate.",
    },
    {
      value: "private",
      icon: <Lock size={18} />,
      title: "Private",
      blurb:
        "Runs entirely on this computer - no API key, nothing leaves your machine. Downloads a speech model (~500 MB) next.",
    },
  ];
  return (
    <div>
      <StepHeader
        icon={<Mic size={20} />}
        title="How should Eve transcribe?"
        subtitle="You can switch anytime in Settings → Local models."
      />
      <div className="space-y-2">
        {options.map((o) => (
          <button
            key={o.value}
            onClick={() => onPick(o.value)}
            className={
              "flex w-full items-start gap-3 rounded-xl border px-4 py-3 text-left transition-colors " +
              (mode === o.value
                ? "border-accent bg-accent-soft"
                : "border-border bg-surface hover:bg-surface-2")
            }
          >
            <span
              className={
                "mt-0.5 flex size-9 shrink-0 items-center justify-center rounded-xl " +
                (mode === o.value ? "bg-accent text-white" : "bg-surface-2 text-ink-soft")
              }
            >
              {o.icon}
            </span>
            <span>
              <span className="font-medium text-ink">{o.title}</span>
              <span className="block text-xs text-ink-soft">{o.blurb}</span>
            </span>
          </button>
        ))}
      </div>
    </div>
  );
}

type StepProgress = { downloaded: number; total: number };

/**
 * Parity Phase B: the Private branch. Reuses the same download machinery as
 * LocalModelsPage (`list_models` / `download_model` + `model://*` events); a
 * finished download auto-selects that model for `localWhisperModel`.
 */
function LocalModelStep({
  selected,
  onSelect,
}: {
  selected: string;
  onSelect: (id: string) => void;
}) {
  const [models, setModels] = useState<ModelStatus[]>([]);
  const [loading, setLoading] = useState(true);
  const [progress, setProgress] = useState<Record<string, StepProgress>>({});
  const [errors, setErrors] = useState<Record<string, string>>({});

  // Keep the latest onSelect reachable from the one-time event subscriptions.
  const onSelectRef = useRef(onSelect);
  onSelectRef.current = onSelect;
  // `model://*` events are global (any catalog download); only auto-select
  // ids that belong to the speech catalog shown here.
  const whisperIdsRef = useRef<Set<string>>(new Set());

  const load = async () => {
    try {
      const whisper = (await api.listModels()).filter((m) => m.kind === "whisper");
      whisperIdsRef.current = new Set(whisper.map((m) => m.id));
      setModels(whisper);
    } catch {
      setModels([]);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    load();
    const subs = [
      on<ModelProgressPayload>(EVT.modelProgress, (e) =>
        setProgress((prev) => ({
          ...prev,
          [e.payload.id]: { downloaded: e.payload.downloaded, total: e.payload.total },
        })),
      ),
      on<ModelStatusPayload>(EVT.modelDone, (e) => {
        setProgress((prev) => {
          const next = { ...prev };
          delete next[e.payload.id];
          return next;
        });
        if (whisperIdsRef.current.has(e.payload.id)) onSelectRef.current(e.payload.id);
        load();
      }),
      on<ModelStatusPayload>(EVT.modelError, (e) => {
        setProgress((prev) => {
          const next = { ...prev };
          delete next[e.payload.id];
          return next;
        });
        if (e.payload.message)
          setErrors((prev) => ({ ...prev, [e.payload.id]: e.payload.message as string }));
        load();
      }),
    ];
    return () => {
      subs.forEach((s) => s.then((un) => un()).catch(() => {}));
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const download = async (id: string) => {
    setErrors((prev) => {
      const next = { ...prev };
      delete next[id];
      return next;
    });
    setProgress((prev) => ({ ...prev, [id]: { downloaded: 0, total: 0 } }));
    await api.downloadModel(id).catch((e) => {
      setProgress((prev) => {
        const next = { ...prev };
        delete next[id];
        return next;
      });
      setErrors((prev) => ({ ...prev, [id]: String(e) }));
    });
  };

  const cancel = (id: string) => api.cancelModelDownload(id).catch(() => {});

  const fmtBytes = (n: number) =>
    n >= 1e9 ? (n / 1e9).toFixed(1) + " GB" : Math.round(n / 1e6) + " MB";

  const anyUsable = models.some((m) => m.installed);

  return (
    <div>
      <StepHeader
        icon={<Lock size={20} />}
        title="Download a speech model"
        subtitle="Whisper Small is the recommended starting point. Models are stored on this computer; manage them later in Settings → Local models."
      />
      {loading ? (
        <p className="text-sm text-ink-faint">Loading catalog…</p>
      ) : (
        <div className="space-y-2">
          {models.map((m) => {
            const prog = progress[m.id];
            const downloading = m.downloading || !!prog;
            const pct = prog && prog.total > 0 ? (prog.downloaded / prog.total) * 100 : 0;
            const isSelected = m.installed && m.id === selected;
            return (
              <div
                key={m.id}
                className={
                  "rounded-xl border px-4 py-3 " +
                  (isSelected ? "border-accent bg-accent-soft/40" : "border-border bg-surface")
                }
              >
                <div className="flex items-center justify-between gap-3">
                  <div>
                    <div className="flex items-center gap-2 text-sm font-medium text-ink">
                      {m.name}
                      {m.id === "whisper-small.en" && (
                        <span className="rounded-full bg-accent-soft px-2 py-0.5 text-[10px] font-medium uppercase tracking-wide text-accent">
                          Recommended
                        </span>
                      )}
                    </div>
                    <div className="text-xs text-ink-faint">{fmtBytes(m.sizeBytes)}</div>
                  </div>
                  {downloading ? (
                    <button
                      onClick={() => cancel(m.id)}
                      className="flex items-center gap-1 rounded-lg px-2.5 py-1.5 text-xs text-ink-faint hover:text-ink"
                    >
                      <X size={13} /> Cancel
                    </button>
                  ) : m.installed ? (
                    isSelected ? (
                      <span className="flex items-center gap-1.5 text-xs text-accent">
                        <Check size={14} /> Ready to use
                      </span>
                    ) : (
                      <button
                        onClick={() => onSelect(m.id)}
                        className="rounded-lg border border-border px-2.5 py-1.5 text-xs text-ink-soft hover:border-accent/50"
                      >
                        Use
                      </button>
                    )
                  ) : (
                    <button
                      onClick={() => download(m.id)}
                      className="flex items-center gap-1.5 rounded-lg bg-accent-soft px-3 py-1.5 text-xs text-ink hover:bg-accent-soft/70"
                    >
                      <Download size={13} /> Download
                    </button>
                  )}
                </div>
                {downloading && (
                  <div className="mt-2">
                    <div className="h-1.5 w-full overflow-hidden rounded-full bg-canvas">
                      <div
                        className="h-full rounded-full bg-accent transition-[width]"
                        style={{ width: `${pct}%` }}
                      />
                    </div>
                    <div className="mt-1 flex items-center gap-1.5 text-[11px] text-ink-faint">
                      <Loader2 size={11} className="animate-spin" />
                      {prog && prog.total > 0
                        ? `${fmtBytes(prog.downloaded)} / ${fmtBytes(prog.total)} (${Math.round(pct)}%)`
                        : "Starting…"}
                    </div>
                  </div>
                )}
                {errors[m.id] && !downloading && (
                  <div className="mt-2 text-xs text-danger">{errors[m.id]}</div>
                )}
              </div>
            );
          })}
        </div>
      )}
      {!loading && !anyUsable && (
        <p className="mt-3 text-xs text-ink-faint">
          Eve can't transcribe offline until a model is downloaded. You can continue setup
          and download one later in Settings → Local models.
        </p>
      )}
    </div>
  );
}

function ApiKeyStep({
  hasKey,
  apiKey,
  setApiKey,
  onSaved,
}: {
  hasKey: boolean;
  apiKey: string;
  setApiKey: (v: string) => void;
  onSaved: () => void;
}) {
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const save = async () => {
    if (!apiKey.trim()) return;
    try {
      await api.storeApiKey(apiKey.trim());
    } catch {
      // Surface the failure instead of claiming success — otherwise the user
      // thinks the key saved and dictation silently fails later.
      setError("Couldn't save the key. Check it and try again.");
      return;
    }
    setApiKey("");
    setSaved(true);
    setError(null);
    onSaved();
  };
  return (
    <div>
      <StepHeader
        icon={<KeyRound size={20} />}
        title="Groq API key"
        subtitle="Eve uses Groq for fast transcription. Your key is stored in the Windows Credential Manager, never on disk."
      />
      {hasKey && !saved ? (
        <div className="flex items-center gap-2 rounded-xl border border-accent/40 bg-accent-soft/40 px-4 py-3 text-sm">
          <Check size={16} className="text-accent" /> A key is already configured.
        </div>
      ) : (
        <div className="flex gap-2">
          <input
            type="password"
            value={apiKey}
            onChange={(e) => setApiKey(e.target.value)}
            placeholder="gsk_..."
            className="flex-1 rounded-xl border border-border bg-surface px-3 py-2 outline-none focus:border-accent"
          />
          <button
            onClick={save}
            className="rounded-xl bg-accent px-4 py-2 text-sm font-medium text-white hover:opacity-90"
          >
            {saved ? "Saved ✓" : "Save"}
          </button>
        </div>
      )}
      {error && (
        <p className="mt-3 text-xs text-danger">{error}</p>
      )}
      <p className="mt-3 text-xs text-ink-faint">
        Get a free key at console.groq.com. This step is skippable - you can add the key
        later in Settings - but Eve can't transcribe in cloud mode until it has one.
      </p>
    </div>
  );
}

function HotkeyStep({
  value,
  onChange,
}: {
  value: string;
  onChange: (v: string) => void;
}) {
  return (
    <div>
      <StepHeader
        icon={<Keyboard size={20} />}
        title="Push-to-talk hotkey"
        subtitle="Hold this key to record, release to transcribe. You can change it anytime."
      />
      <div className="grid grid-cols-2 gap-2">
        {SHORTCUT_CHOICES.map((s) => (
          <button
            key={s}
            onClick={() => onChange(s)}
            className={
              "rounded-xl border px-3 py-3 text-left font-mono text-sm transition-colors " +
              (value === s
                ? "border-accent bg-accent-soft text-ink"
                : "border-border bg-surface text-ink-soft hover:bg-surface-2")
            }
          >
            {s}
          </button>
        ))}
      </div>
    </div>
  );
}

function LanguageStep({
  value,
  onChange,
}: {
  value: string[];
  onChange: (v: string[]) => void;
}) {
  return (
    <div>
      <StepHeader
        icon={<Languages size={20} />}
        title="Languages"
        subtitle="Pick the languages you speak. Choose one to lock transcription to it, or several (or Auto-detect) to let Eve figure it out."
      />
      <LanguageMultiSelect value={value} onChange={onChange} />
    </div>
  );
}

function MicCheckStep() {
  const [level, setLevel] = useState(0);
  const [status, setStatus] = useState<"idle" | "live" | "denied">("idle");
  const rafRef = useRef<number | null>(null);
  const streamRef = useRef<MediaStream | null>(null);

  useEffect(() => {
    let ctx: AudioContext | null = null;
    let cancelled = false;
    (async () => {
      try {
        const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
        if (cancelled) {
          stream.getTracks().forEach((t) => t.stop());
          return;
        }
        streamRef.current = stream;
        ctx = new AudioContext();
        const src = ctx.createMediaStreamSource(stream);
        const analyser = ctx.createAnalyser();
        analyser.fftSize = 512;
        src.connect(analyser);
        const data = new Uint8Array(analyser.frequencyBinCount);
        setStatus("live");
        const tick = () => {
          analyser.getByteTimeDomainData(data);
          let peak = 0;
          for (let i = 0; i < data.length; i++) {
            peak = Math.max(peak, Math.abs(data[i] - 128) / 128);
          }
          setLevel(peak);
          rafRef.current = requestAnimationFrame(tick);
        };
        tick();
      } catch {
        setStatus("denied");
      }
    })();
    return () => {
      cancelled = true;
      if (rafRef.current) cancelAnimationFrame(rafRef.current);
      streamRef.current?.getTracks().forEach((t) => t.stop());
      ctx?.close().catch(() => {});
    };
  }, []);

  const bars = 32;
  return (
    <div>
      <StepHeader
        icon={<Mic size={20} />}
        title="Microphone check"
        subtitle="Say something — the meter should jump while you talk."
      />
      {status === "denied" ? (
        <div className="rounded-xl border border-danger/40 bg-danger/5 px-4 py-3 text-sm text-danger">
          Couldn't access the microphone. Allow mic access in Windows Settings → Privacy →
          Microphone, then revisit this step. You can still finish setup.
        </div>
      ) : (
        <>
          <div className="flex h-20 items-center justify-center gap-[3px] rounded-2xl border border-border bg-surface-2/50 px-4">
            {Array.from({ length: bars }).map((_, i) => {
              const center = Math.abs(i - bars / 2) / (bars / 2); // 0 center → 1 edges
              const h = Math.max(4, level * 100 * (1 - center * 0.6));
              return (
                <span
                  key={i}
                  className="w-[3px] rounded-full bg-accent transition-[height] duration-75"
                  style={{ height: `${Math.min(100, h)}%`, opacity: 0.4 + level }}
                />
              );
            })}
          </div>
          <p className="mt-3 text-xs text-ink-faint">
            {status === "live"
              ? "Microphone is live. This preview uses the browser mic; dictation itself records natively."
              : "Requesting microphone access…"}
          </p>
        </>
      )}
    </div>
  );
}

function CleanupStep({
  value,
  hasKey,
  onChange,
}: {
  value: CleanupLevel;
  hasKey: boolean;
  onChange: (v: CleanupLevel) => void;
}) {
  return (
    <div>
      <StepHeader
        icon={<Sparkles size={20} />}
        title="Cleanup level"
        subtitle="How much should Eve polish your words? Higher levels use Groq Llama to rewrite for clarity."
      />
      <div className="space-y-2">
        {CLEANUP.map((c) => (
          <button
            key={c.value}
            onClick={() => onChange(c.value)}
            className={
              "flex w-full items-start gap-3 rounded-xl border px-4 py-3 text-left transition-colors " +
              (value === c.value
                ? "border-accent bg-accent-soft"
                : "border-border bg-surface hover:bg-surface-2")
            }
          >
            <span
              className={
                "mt-0.5 size-4 shrink-0 rounded-full border-2 " +
                (value === c.value ? "border-accent bg-accent" : "border-ink-faint")
              }
            />
            <span>
              <span className="font-medium text-ink">{c.label}</span>
              <span className="block text-xs text-ink-soft">{c.hint}</span>
            </span>
          </button>
        ))}
      </div>
      {value !== "none" && !hasKey && (
        <p className="mt-3 flex items-center gap-1.5 text-xs text-danger">
          <ShieldCheck size={13} /> This level needs your Groq API key (add it in
          Settings).
        </p>
      )}
    </div>
  );
}

/**
 * Shared language multi-select (also used by Hub Settings). Toggling a specific
 * language clears "auto"; clearing all selections falls back to "auto".
 */
export function LanguageMultiSelect({
  value,
  onChange,
}: {
  value: string[];
  onChange: (v: string[]) => void;
}) {
  const selected = new Set(value);
  const toggle = (code: string) => {
    if (code === "auto") {
      onChange(["auto"]);
      return;
    }
    const nextSet = new Set(value.filter((c) => c !== "auto"));
    if (nextSet.has(code)) nextSet.delete(code);
    else nextSet.add(code);
    const arr = [...nextSet];
    onChange(arr.length ? arr : ["auto"]);
  };
  return (
    <div className="flex flex-wrap gap-2">
      {LANGUAGES.map((l) => {
        const on = l.code === "auto" ? selected.has("auto") : selected.has(l.code);
        return (
          <button
            key={l.code}
            onClick={() => toggle(l.code)}
            className={
              "rounded-full border px-3 py-1.5 text-sm transition-colors " +
              (on
                ? "border-accent bg-accent-soft text-ink"
                : "border-border bg-surface text-ink-soft hover:bg-surface-2")
            }
          >
            {l.label}
          </button>
        );
      })}
    </div>
  );
}
