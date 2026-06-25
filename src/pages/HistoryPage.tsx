import { useCallback, useEffect, useRef, useState } from "react";
import { Search, Trash2, RotateCcw, Play, ChevronLeft, ChevronRight } from "lucide-react";
import { api, audioSrc, type Transcript } from "../lib/api";

const PER_PAGE = 20;

export function HistoryPage() {
  const [query, setQuery] = useState("");
  const [page, setPage] = useState(1);
  const [items, setItems] = useState<Transcript[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(true);
  // Rows soft-deleted this session, kept visible briefly so they can be recovered.
  const [recoverable, setRecoverable] = useState<Transcript[]>([]);

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

  const onDelete = async (t: Transcript) => {
    await api.deleteTranscript(t.id).catch(() => {});
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
          items.map((t) => <HistoryCard key={t.id} t={t} onDelete={() => onDelete(t)} />)
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

function HistoryCard({ t, onDelete }: { t: Transcript; onDelete: () => void }) {
  // Show polished by default; toggle to raw when they differ.
  const hasBoth = t.rawText.trim() !== t.polishedText.trim();
  const [showRaw, setShowRaw] = useState(false);
  const audioRef = useRef<HTMLAudioElement | null>(null);

  const text = showRaw ? t.rawText : t.polishedText;

  return (
    <div className="rounded-2xl border border-border bg-surface p-4">
      <div className="flex items-start justify-between gap-3">
        <p className="whitespace-pre-wrap text-ink">{text || <span className="text-ink-faint">(empty)</span>}</p>
        <button
          onClick={onDelete}
          title="Delete"
          className="shrink-0 rounded-lg p-1.5 text-ink-faint hover:bg-surface-2 hover:text-danger"
        >
          <Trash2 size={16} />
        </button>
      </div>

      <div className="mt-3 flex flex-wrap items-center gap-x-3 gap-y-2 text-xs text-ink-faint">
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

        {hasBoth && (
          <button
            onClick={() => setShowRaw((v) => !v)}
            className="ml-auto rounded-md border border-border px-2 py-0.5 text-ink-soft hover:bg-surface-2"
          >
            {showRaw ? "Show polished" : "Show raw"}
          </button>
        )}

        {t.audioPath && (
          <button
            onClick={() => audioRef.current?.play()}
            className={
              "flex items-center gap-1 rounded-md border border-border px-2 py-0.5 text-ink-soft hover:bg-surface-2 " +
              (hasBoth ? "" : "ml-auto")
            }
          >
            <Play size={12} /> Play
          </button>
        )}
      </div>

      {t.audioPath && <audio ref={audioRef} src={audioSrc(t.audioPath)} preload="none" />}
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
