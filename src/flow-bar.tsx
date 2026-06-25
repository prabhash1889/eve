import React, { useEffect, useState } from "react";
import ReactDOM from "react-dom/client";
import { Check, Sparkles, Copy } from "lucide-react";
import "@fontsource/figtree/400.css";
import "@fontsource/figtree/500.css";
import "./styles/globals.css";
import {
  EVT,
  on,
  type ErrorPayload,
  type TranscriptPayload,
  type StartPayload,
} from "./lib/api";

document.body.classList.add("flowbar");
if (window.matchMedia("(prefers-color-scheme: dark)").matches) {
  document.documentElement.classList.add("dark");
}

type State = "idle" | "listening" | "processing" | "preview" | "done" | "error" | "copied";

const BARS = 28;

function FlowBar() {
  const [state, setState] = useState<State>("listening");
  const [errMsg, setErrMsg] = useState("");
  const [levels, setLevels] = useState<number[]>(() => new Array(BARS).fill(0.05));
  const [transcript, setTranscript] = useState("");
  const [polished, setPolished] = useState(false);
  // Flow Bar appearance, pushed from Rust on `start`.
  const [scale, setScale] = useState(1);
  const [opacity, setOpacity] = useState(1);

  useEffect(() => {
    const unlisteners: Array<Promise<() => void>> = [
      on<StartPayload>(EVT.start, (e) => {
        if (e.payload) {
          setScale(e.payload.bubbleScale || 1);
          setOpacity(e.payload.bubbleOpacity ?? 1);
        }
        setState("listening");
        setTranscript("");
        setPolished(false);
        setLevels(new Array(BARS).fill(0.05));
      }),
      on<number>(EVT.amplitude, (e) => {
        const v = Math.min(1, Math.max(0.04, e.payload * 6));
        setLevels((prev) => [...prev.slice(1), v]);
      }),
      on(EVT.processing, () => setState("processing")),
      on<TranscriptPayload>(EVT.transcriptRaw, (e) => {
        setTranscript(e.payload?.text ?? "");
        setPolished(false);
        setState("preview");
      }),
      on<TranscriptPayload>(EVT.transcriptPolished, (e) => {
        setTranscript(e.payload?.text ?? "");
        setPolished(true);
        setState("preview");
      }),
      on(EVT.done, () => setState("done")),
      on<ErrorPayload>(EVT.error, (e) => {
        setErrMsg(e.payload?.message ?? "Something went wrong");
        setState("error");
      }),
      on(EVT.cancel, () => setState("idle")),
      on(EVT.copied, () => setState("copied")),
    ];
    return () => {
      unlisteners.forEach((u) => u.then((fn) => fn()));
    };
  }, []);

  // Collapse newlines so the preview stays a single, readable line in the pill.
  const preview = transcript.replace(/\s*\n+\s*/g, " ").trim();

  return (
    <div className="flex h-screen w-screen items-end justify-center p-2">
      <div
        className="flex items-center gap-3 rounded-full border border-border bg-surface/95 px-4 py-2.5 shadow-lg backdrop-blur transition-all duration-300 ease-out"
        style={{ transform: `scale(${scale})`, opacity, transformOrigin: "center bottom" }}
      >
        <StatusDot state={state} />
        {state === "listening" && <Waveform levels={levels} />}
        {state === "processing" && <Dots />}
        {state === "preview" && (
          <span className="flex items-center gap-1.5">
            {polished && <Sparkles size={13} className="shrink-0 text-accent" />}
            <span
              className={`max-w-[200px] truncate text-sm transition-colors duration-300 ${
                polished ? "text-ink" : "text-ink-soft"
              }`}
            >
              {preview || "…"}
            </span>
          </span>
        )}
        {state === "done" && (
          <span className="flex items-center gap-1.5 text-sm text-accent">
            <Check size={14} /> Inserted
          </span>
        )}
        {state === "copied" && (
          <span className="flex items-center gap-1.5 text-sm text-accent">
            <Copy size={13} /> Copied
          </span>
        )}
        {state === "error" && (
          <span className="max-w-[200px] truncate text-sm text-danger">{errMsg}</span>
        )}
        {state === "idle" && <span className="text-sm text-ink-faint">Ready</span>}
      </div>
    </div>
  );
}

function StatusDot({ state }: { state: State }) {
  const color =
    state === "listening"
      ? "bg-accent"
      : state === "processing" || state === "preview"
        ? "bg-accent/70"
        : state === "error"
          ? "bg-danger"
          : "bg-ink-faint";
  return (
    <span
      className={`h-2.5 w-2.5 shrink-0 rounded-full transition-colors duration-300 ${color} ${
        state === "listening" ? "animate-pulse" : ""
      }`}
    />
  );
}

function Waveform({ levels }: { levels: number[] }) {
  return (
    <div className="flex h-6 items-center gap-[2px]">
      {levels.map((v, i) => (
        <span
          key={i}
          className="w-[3px] rounded-full bg-accent transition-[height] duration-75 ease-out"
          style={{ height: `${Math.round(v * 100)}%`, opacity: 0.45 + v * 0.55 }}
        />
      ))}
    </div>
  );
}

function Dots() {
  return (
    <div className="flex items-center gap-1.5">
      {[0, 1, 2].map((i) => (
        <span
          key={i}
          className="h-1.5 w-1.5 animate-bounce rounded-full bg-accent"
          style={{ animationDelay: `${i * 120}ms` }}
        />
      ))}
      <span className="ml-1 text-sm text-ink-soft">Transcribing</span>
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <FlowBar />
  </React.StrictMode>,
);
