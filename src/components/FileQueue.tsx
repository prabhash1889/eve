import { useEffect, useRef, useState } from "react";
import { FileAudio, Upload, X, Loader2, AlertCircle } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import {
  api,
  on,
  EVT,
  type QueueProgressPayload,
  type QueueDonePayload,
  type QueueErrorPayload,
} from "../lib/api";

/** Audio containers symphonia can decode on the backend. */
const AUDIO_EXTS = ["wav", "mp3", "m4a", "mp4", "flac", "ogg", "oga", "aac"];

type QItem =
  | { id: number; fileName: string; kind: "active"; stage: string }
  | { id: number; fileName: string; kind: "error"; message: string };

/**
 * Phase C: drop audio files (or pick them) to transcribe. Files are read in
 * place on the backend, processed serially, and land in History when done.
 * `onItemDone` lets the parent refresh the History list.
 */
export function FileQueue({ hasKey, onItemDone }: { hasKey: boolean; onItemDone: () => void }) {
  const [items, setItems] = useState<QItem[]>([]);
  const [dragging, setDragging] = useState(false);
  // Keep a live ref so the window-level drag/drop listener (registered once)
  // always sees the current handler without re-subscribing.
  const addPathsRef = useRef<(paths: string[]) => void>(() => {});

  const addPaths = async (paths: string[]) => {
    const audio = paths.filter((p) => {
      const ext = p.split(".").pop()?.toLowerCase() ?? "";
      return AUDIO_EXTS.includes(ext);
    });
    if (audio.length === 0) return;
    try {
      const queued = await api.transcribeFiles(audio);
      setItems((xs) => [
        ...queued.map((q) => ({ id: q.id, fileName: q.fileName, kind: "active" as const, stage: "Queued" })),
        ...xs,
      ]);
    } catch {
      // Backend rejected the batch — nothing queued, so nothing to show.
    }
  };
  addPathsRef.current = addPaths;

  // Queue lifecycle events (backend → Hub window).
  useEffect(() => {
    const unlisteners = [
      on<QueueProgressPayload>(EVT.queueProgress, (e) => {
        const { id, stage } = e.payload;
        setItems((xs) =>
          xs.map((it) => (it.id === id && it.kind === "active" ? { ...it, stage } : it)),
        );
      }),
      on<QueueDonePayload>(EVT.queueDone, (e) => {
        setItems((xs) => xs.filter((it) => it.id !== e.payload.id));
        onItemDone();
      }),
      on<QueueErrorPayload>(EVT.queueError, (e) => {
        const { id, fileName, message } = e.payload;
        setItems((xs) => {
          const next = xs.filter((it) => it.id !== id);
          return [{ id, fileName, kind: "error" as const, message }, ...next];
        });
      }),
    ];
    return () => {
      unlisteners.forEach((u) => u.then((fn) => fn()));
    };
  }, [onItemDone]);

  // Window-level file drag & drop (registered once).
  useEffect(() => {
    const un = getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type === "enter" || event.payload.type === "over") {
        setDragging(true);
      } else if (event.payload.type === "drop") {
        setDragging(false);
        addPathsRef.current(event.payload.paths);
      } else {
        setDragging(false);
      }
    });
    return () => {
      un.then((fn) => fn());
    };
  }, []);

  const pickFiles = async () => {
    const selected = await open({
      multiple: true,
      filters: [{ name: "Audio", extensions: AUDIO_EXTS }],
    }).catch(() => null);
    if (!selected) return;
    addPaths(Array.isArray(selected) ? selected : [selected]);
  };

  const cancel = (id: number) => {
    api.cancelQueueItem(id).catch(() => {});
    setItems((xs) => xs.filter((it) => it.id !== id));
  };

  const dismiss = (id: number) => setItems((xs) => xs.filter((it) => it.id !== id));

  return (
    <div
      className={
        "rounded-2xl border bg-surface p-5 transition-colors " +
        (dragging ? "border-accent bg-accent-soft/40" : "border-border")
      }
    >
      <div className="flex items-center justify-between gap-3">
        <div>
          <div className="flex items-center gap-2 text-sm font-medium text-ink">
            <FileAudio size={16} className="text-ink-faint" />
            Transcribe audio files
          </div>
          <div className="mt-1 text-xs text-ink-faint">
            Drop files here or pick them — wav, mp3, m4a, flac, ogg. Results appear in History.
            {!hasKey && " Needs a Groq API key, or a local model."}
          </div>
        </div>
        <button
          onClick={pickFiles}
          className="flex shrink-0 items-center gap-1.5 rounded-xl bg-accent px-4 py-2 text-sm font-medium text-white hover:opacity-90"
        >
          <Upload size={14} /> Transcribe files…
        </button>
      </div>

      {items.length > 0 && (
        <div className="mt-4 space-y-2">
          {items.map((it) =>
            it.kind === "active" ? (
              <div
                key={it.id}
                className="flex items-center gap-3 rounded-xl border border-border bg-surface-2 px-3 py-2 text-sm"
              >
                <Loader2 size={15} className="shrink-0 animate-spin text-accent" />
                <span className="truncate text-ink">{it.fileName}</span>
                <span className="ml-auto shrink-0 text-xs text-ink-faint">{it.stage}…</span>
                <button
                  onClick={() => cancel(it.id)}
                  title="Cancel"
                  className="shrink-0 rounded-md p-1 text-ink-faint hover:bg-surface hover:text-danger"
                >
                  <X size={14} />
                </button>
              </div>
            ) : (
              <div
                key={it.id}
                className="flex items-center gap-3 rounded-xl border border-danger/40 bg-danger/5 px-3 py-2 text-sm"
              >
                <AlertCircle size={15} className="shrink-0 text-danger" />
                <span className="truncate text-ink">{it.fileName}</span>
                <span className="ml-auto shrink-0 text-xs text-danger">{it.message}</span>
                <button
                  onClick={() => dismiss(it.id)}
                  title="Dismiss"
                  className="shrink-0 rounded-md p-1 text-ink-faint hover:bg-surface hover:text-danger"
                >
                  <X size={14} />
                </button>
              </div>
            ),
          )}
        </div>
      )}
    </div>
  );
}
