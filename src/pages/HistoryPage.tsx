import { useCallback, useEffect, useState } from "react";
import {
  Search,
  Trash2,
  RotateCcw,
  Copy,
  Check,
  ChevronLeft,
  ChevronRight,
  FileAudio,
  ClipboardPaste,
  Wand2,
  X,
} from "lucide-react";
import { api, type Transcript, type Transform } from "../lib/api";

const PER_PAGE = 20;

export function HistoryPage({ reloadSignal }: { reloadSignal?: number }) {
  const [query, setQuery] = useState("");
  const [page, setPage] = useState(1);
  const [items, setItems] = useState<Transcript[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(true);
  // Rows soft-deleted this session, kept visible briefly so they can be recovered.
  const [recoverable, setRecoverable] = useState<Transcript[]>([]);
  // Saved transforms for the per-card "Re-polish…" action (loaded once).
  const [transforms, setTransforms] = useState<Transform[]>([]);

  useEffect(() => {
    api.getTransforms().then(setTransforms).catch(() => setTransforms([]));
  }, []);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const res = await api.getHistory(page, PER_PAGE, query.trim() || undefined);
      setItems(res.items);
      setTotal(res.total);
    } catch {
      setItems([]);
      setTotal(0);
    } finally {
      setLoading(false);
    }
  }, [page, query]);

  // Debounce search + reload whenever the query or page changes.
  useEffect(() => {
    const t = setTimeout(load, query ? 250 : 0);
    return () => clearTimeout(t);
  }, [load, query]);

  // Reset to the first page on a new search term.
  useEffect(() => {
    setPage(1);
  }, [query]);

  // Reload when a file transcription finishes (parent bumps the signal).
  useEffect(() => {
    if (reloadSignal) load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [reloadSignal]);

  const onDelete = async (t: Transcript) => {
    // Only drop the row from the UI if the backend delete actually succeeded —
    // otherwise it would vanish from the list while still living in the DB.
    try {
      await api.deleteTranscript(t.id);
    } catch {
      return;
    }
    setItems((xs) => xs.filter((x) => x.id !== t.id));
    setTotal((n) => Math.max(0, n - 1));
    setRecoverable((xs) => [t, ...xs]);
  };

  const onRecover = async (t: Transcript) => {
    await api.recoverTranscript(t.id).catch(() => {});
    setRecoverable((xs) => xs.filter((x) => x.id !== t.id));
    load();
  };

  const onClearAll = async () => {
    if (!confirm("Move all transcripts to deleted? You can recover them per-item afterwards.")) return;
    await api.clearHistory().catch(() => {});
    load();
  };

  const totalPages = Math.max(1, Math.ceil(total / PER_PAGE));

  return (
    <div>
      <div className="flex items-center justify-between">
        <h1 className="font-serif text-3xl">History</h1>
        {total > 0 && (
          <button
            onClick={onClearAll}
            className="text-xs text-ink-faint underline hover:text-danger"
          >
            Clear all
          </button>
        )}
      </div>

      <div className="mt-5 flex items-center gap-2 rounded-xl border border-border bg-surface px-3 py-2 focus-within:border-accent">
        <Search size={16} className="text-ink-faint" />
        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Search transcripts…"
          className="flex-1 bg-transparent outline-none placeholder:text-ink-faint"
        />
      </div>

      {recoverable.length > 0 && (
        <div className="mt-4 space-y-2">
          {recoverable.map((t) => (
            <div
              key={t.id}
              className="flex items-center justify-between rounded-xl border border-border bg-surface-2 px-4 py-2 text-sm"
            >
              <span className="truncate text-ink-soft">Deleted: “{preview(t)}”</span>
              <button
                onClick={() => onRecover(t)}
                className="ml-3 flex shrink-0 items-center gap-1 text-accent hover:underline"
              >
                <RotateCcw size={14} /> Undo
              </button>
            </div>
          ))}
        </div>
      )}

      <div className="mt-6 space-y-3">
        {loading ? (
          <p className="text-sm text-ink-faint">Loading…</p>
        ) : items.length === 0 ? (
          <EmptyState searching={!!query.trim()} />
        ) : (
          items.map((t) => (
            <HistoryCard
              key={t.id}
              t={t}
              transforms={transforms}
              onDelete={() => onDelete(t)}
              query={query}
            />
          ))
        )}
      </div>

      {totalPages > 1 && (
        <div className="mt-6 flex items-center justify-center gap-4 text-sm">
          <button
            disabled={page <= 1}
            onClick={() => setPage((p) => p - 1)}
            className="flex items-center gap-1 rounded-lg px-2 py-1 text-ink-soft enabled:hover:bg-surface-2 disabled:opacity-40"
          >
            <ChevronLeft size={16} /> Prev
          </button>
          <span className="text-ink-faint">
            Page {page} of {totalPages}
          </span>
          <button
            disabled={page >= totalPages}
            onClick={() => setPage((p) => p + 1)}
            className="flex items-center gap-1 rounded-lg px-2 py-1 text-ink-soft enabled:hover:bg-surface-2 disabled:opacity-40"
          >
            Next <ChevronRight size={16} />
          </button>
        </div>
      )}
    </div>
  );
}

function HistoryCard({
  t,
  transforms,
  onDelete,
  query,
}: {
  t: Transcript;
  transforms: Transform[];
  onDelete: () => void;
  query: string;
}) {
  // Show polished by default; toggle to raw when they differ.
  const hasBoth = t.rawText.trim() !== t.polishedText.trim();
  const [showRaw, setShowRaw] = useState(false);
  const [copied, setCopied] = useState(false);
  // Result of a "Re-polish…" run over the raw transcript, shown in place.
  const [repolished, setRepolished] = useState<{ name: string; text: string } | null>(null);
  const [repolishing, setRepolishing] = useState(false);
  const [pasted, setPasted] = useState(false);

  const text = repolished ? repolished.text : showRaw ? t.rawText : t.polishedText;

  const onCopy = async () => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 1200);
    } catch {
      // Clipboard unavailable — nothing more we can do here.
    }
  };

  const onPaste = async () => {
    try {
      await api.pasteText(text);
      setPasted(true);
      setTimeout(() => setPasted(false), 1200);
    } catch {
      // Injection failed (e.g. no focusable window) - nothing to show.
    }
  };

  const onRepublish = async (id: string) => {
    const transform = transforms.find((x) => String(x.id) === id);
    if (!transform || repolishing) return;
    const source = t.rawText.trim() || t.polishedText;
    setRepolishing(true);
    try {
      const out = await api.applyTransform(transform.id, source);
      if (out.trim()) setRepolished({ name: transform.name, text: out });
    } catch {
      // LLM failure - keep the current text on display.
    } finally {
      setRepolishing(false);
    }
  };


  return (
    <div className="rounded-2xl border border-border bg-surface p-4">
      <div className="flex items-start justify-between gap-3">
        <p className="whitespace-pre-wrap text-ink">
          {text ? highlightText(text, query) : <span className="text-ink-faint">(empty)</span>}
        </p>
        <button
          onClick={onDelete}
          title="Delete"
          className="shrink-0 rounded-lg p-1.5 text-ink-faint hover:bg-surface-2 hover:text-danger"
        >
          <Trash2 size={16} />
        </button>
      </div>

      <div className="mt-3 flex flex-wrap items-center gap-x-3 gap-y-2 text-xs text-ink-faint">
        {t.sourceFile && (
          <>
            <span
              className="flex items-center gap-1 text-accent"
              title={t.sourceFile}
            >
              <FileAudio size={12} /> {fileName(t.sourceFile)}
            </span>
            <Dot />
          </>
        )}
        <span>{formatTime(t.createdAt)}</span>
        <Dot />
        <span>{t.wordCount} words</span>
        <Dot />
        <span>{formatDuration(t.durationMs)}</span>
        {t.wasPolished && (
          <>
            <Dot />
            <span className="capitalize">{t.cleanupLevel} polish</span>
          </>
        )}

        {hasBoth && !repolished && (
          <button
            onClick={() => setShowRaw((v) => !v)}
            className="ml-auto rounded-md border border-border px-2 py-0.5 text-ink-soft hover:bg-surface-2"
          >
            {showRaw ? "Show polished" : "Show raw"}
          </button>
        )}

        {repolished && (
          <button
            onClick={() => setRepolished(null)}
            title="Back to the saved transcript"
            className="ml-auto flex items-center gap-1 rounded-md border border-border px-2 py-0.5 text-accent hover:bg-surface-2"
          >
            <X size={12} /> {repolished.name}
          </button>
        )}

        <button
          onClick={onCopy}
          title="Copy text"
          className={
            "flex items-center gap-1 rounded-md border border-border px-2 py-0.5 text-ink-soft hover:bg-surface-2 " +
            (hasBoth || repolished ? "" : "ml-auto")
          }
        >
          {copied ? <Check size={12} /> : <Copy size={12} />}
          {copied ? "Copied" : "Copy"}
        </button>

        <button
          onClick={onPaste}
          title="Paste into the focused app"
          className="flex items-center gap-1 rounded-md border border-border px-2 py-0.5 text-ink-soft hover:bg-surface-2"
        >
          {pasted ? <Check size={12} /> : <ClipboardPaste size={12} />}
          {pasted ? "Pasted" : "Paste"}
        </button>

        {!repolished && transforms.length > 0 && (
          <span
            className="flex items-center gap-1 rounded-md border border-border px-2 py-0.5 text-ink-soft"
            title="Run a saved transform over the raw transcript"
          >
            <Wand2 size={12} className="text-ink-faint" />
            <select
              value=""
              disabled={repolishing}
              onChange={(e) => onRepublish(e.target.value)}
              className="cursor-pointer bg-transparent text-xs outline-none disabled:opacity-50"
            >
              <option value="">{repolishing ? "Polishing…" : "Re-polish…"}</option>
              {transforms.map((tr) => (
                <option key={tr.id} value={String(tr.id)}>
                  {tr.name}
                </option>
              ))}
            </select>
          </span>
        )}
      </div>
    </div>
  );
}

function EmptyState({ searching }: { searching: boolean }) {
  return (
    <div className="rounded-2xl border border-dashed border-border bg-surface/50 p-10 text-center">
      <p className="text-ink-soft">
        {searching ? "No transcripts match your search." : "No dictations yet."}
      </p>
      {!searching && (
        <p className="mt-1 text-sm text-ink-faint">Hold your hotkey, speak, and release — it'll show up here.</p>
      )}
    </div>
  );
}

const Dot = () => <span className="text-ink-faint/50">·</span>;

/** Last path segment of a source-file path, for the History badge. */
function fileName(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

function preview(t: Transcript): string {
  const s = (t.polishedText || t.rawText).trim();
  return s.length > 60 ? s.slice(0, 60) + "…" : s;
}

function formatTime(ms: number): string {
  return new Date(ms).toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  });
}

function formatDuration(ms: number): string {
  const s = Math.round(ms / 1000);
  if (s < 60) return `${s}s`;
  return `${Math.floor(s / 60)}m ${s % 60}s`;
}

function escapeRegExp(s: string) {
  return s.replace(/[/\-\\^$*+?.()|[\]{}]/g, "\\$&");
}

function highlightText(text: string, query: string): React.ReactNode {
  if (!text) return "";
  const terms = query.trim().split(/\s+/).filter(Boolean);
  if (terms.length === 0) return text;

  const regex = new RegExp(`(${terms.map(escapeRegExp).join("|")})`, "gi");
  const parts = text.split(regex);
  return (
    <>
      {parts.map((part, i) =>
        regex.test(part) ? (
          <mark key={i} className="bg-amber-500/20 text-amber-900 dark:text-amber-100 rounded-[2px] px-0.5">
            {part}
          </mark>
        ) : (
          part
        )
      )}
    </>
  );
}
