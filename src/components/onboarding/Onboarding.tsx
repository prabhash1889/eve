import { useEffect, useRef, useState, type ReactNode } from "react";
import {
  ArrowRight,
  ArrowLeft,
  Check,
  KeyRound,
  Keyboard,
  Languages,
  Mic,
  Sparkles,
  ShieldCheck,
} from "lucide-react";
import {
  api,
  effectiveLanguage,
  type CleanupLevel,
  type Settings,
} from "../../lib/api";
import { LANGUAGES, CLEANUP, SHORTCUT_CHOICES } from "../../lib/options";

/**
 * Phase 10 first-run onboarding. Shown over the Hub while
 * `settings.onboardingComplete` is false. Walks through the key, hotkey,
 * languages, a live mic test, and cleanup level, then persists everything and
 * flips the flag via `onComplete`.
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

  const steps = ["Welcome", "API key", "Hotkey", "Languages", "Mic check", "Cleanup"];
  const last = steps.length - 1;

  const next = () => setStep((s) => Math.min(last, s + 1));
  const back = () => setStep((s) => Math.max(0, s - 1));

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
          {step === 1 && (
            <ApiKeyStep
              hasKey={hasKey}
              apiKey={apiKey}
              setApiKey={setApiKey}
              onSaved={() => setHasKey(true)}
            />
          )}
          {step === 2 && (
            <HotkeyStep
              value={draft.shortcut}
              onChange={(shortcut) => setDraft((d) => ({ ...d, shortcut }))}
            />
          )}
          {step === 3 && (
            <LanguageStep
              value={draft.languages}
              onChange={(languages) => setDraft((d) => ({ ...d, languages }))}
            />
          )}
          {step === 4 && <MicCheckStep />}
          {step === 5 && (
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
          "Connect your Groq API key",
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
        Get a free key at console.groq.com. You can also add it later in Settings — Eve just
        can't transcribe until it has one.
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
          <ShieldCheck size={13} /> This level needs your Groq API key (add it in step 2 or
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
