import { useCallback, useEffect, useRef, useState } from "react";
import { Search, Trash2, Star, Plus, Upload, Download, Pencil, X, Check } from "lucide-react";
import { api, type DictionaryEntry } from "../lib/api";

export function DictionaryPage() {
  const [query, setQuery] = useState("");
  const [items, setItems] = useState<DictionaryEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [editing, setEditing] = useState<DictionaryEntry | null>(null);
  const [adding, setAdding] = useState(false);
  const fileRef = useRef<HTMLInputElement | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      setItems(await api.getDictionary(query.trim() || undefined));
    } catch {
      setItems([]);
    } finally {
      setLoading(false);
    }
  }, [query]);

  // Debounce search + reload whenever the query changes.
  useEffect(() => {
    const t = setTimeout(load, query ? 250 : 0);
    return () => clearTimeout(t);
  }, [load, query]);

  const onSave = async (word: string, replacement: string | null, isStarred: boolean) => {
    await api.upsertDictionaryEntry(word, replacement, isStarred).catch(() => {});
    setEditing(null);
    setAdding(false);
    load();
  };

  const onDelete = async (e: DictionaryEntry) => {
    await api.deleteDictionaryEntry(e.id).catch(() => {});
    setItems((xs) => xs.filter((x) => x.id !== e.id));
  };

  const onToggleStar = async (e: DictionaryEntry) => {
    await api.upsertDictionaryEntry(e.word, e.replacement, !e.isStarred).catch(() => {});
    setItems((xs) => xs.map((x) => (x.id === e.id ? { ...x, isStarred: !x.isStarred } : x)));
  };

  const onImportFile = async (file: File) => {
    const text = await file.text();
    const n = await api.importDictionaryCsv(text).catch(() => 0);
    if (fileRef.current) fileRef.current.value = "";
    alert(`Imported ${n} ${n === 1 ? "term" : "terms"}.`);
    load();
  };

  const onExport = async () => {
    const csv = await api.exportDictionaryCsv().catch(() => "");
    if (!csv) return;
    const blob = new Blob([csv], { type: "text/csv" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = "eve-dictionary.csv";
    a.click();
    URL.revokeObjectURL(url);
  };

  return (
    <div>
      <div className="flex items-center justify-between">
        <h1 className="font-serif text-3xl">Dictionary</h1>
        <div className="flex items-center gap-3 text-xs">
          <button
            onClick={() => fileRef.current?.click()}
            className="flex items-center gap-1 text-ink-soft hover:text-ink"
          >
            <Upload size={14} /> Import
          </button>
          <button
            onClick={onExport}
            className="flex items-center gap-1 text-ink-soft hover:text-ink"
          >
            <Download size={14} /> Export
          </button>
          <input
            ref={fileRef}
            type="file"
            accept=".csv,text/csv"
            className="hidden"
            onChange={(e) => {
              const f = e.target.files?.[0];
              if (f) onImportFile(f);
            }}
          />
        </div>
      </div>

      <p className="mt-2 max-w-lg text-sm text-ink-soft">
        Boost recognition of names and jargon, or map a misspelling to its correct
        form. Starred terms are always sent to the transcriber as a hint.
      </p>

      <div className="mt-5 flex items-center gap-2">
        <div className="flex flex-1 items-center gap-2 rounded-xl border border-border bg-surface px-3 py-2 focus-within:border-accent">
          <Search size={16} className="text-ink-faint" />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search terms…"
            className="flex-1 bg-transparent outline-none placeholder:text-ink-faint"
          />
        </div>
        <button
          onClick={() => {
            setAdding(true);
            setEditing(null);
          }}
          className="flex items-center gap-1 rounded-xl bg-accent px-4 py-2 text-sm font-medium text-white hover:opacity-90"
        >
          <Plus size={16} /> Add
        </button>
      </div>

      {adding && (
        <div className="mt-4">
          <EntryForm onSave={onSave} onCancel={() => setAdding(false)} />
        </div>
      )}

      <div className="mt-6 space-y-2">
        {loading ? (
          <p className="text-sm text-ink-faint">Loading…</p>
        ) : items.length === 0 ? (
          <EmptyState searching={!!query.trim()} />
        ) : (
          items.map((e) =>
            editing?.id === e.id ? (
              <EntryForm
                key={e.id}
                entry={e}
                onSave={onSave}
                onCancel={() => setEditing(null)}
              />
            ) : (
              <EntryRow
                key={e.id}
                entry={e}
                onEdit={() => {
                  setEditing(e);
                  setAdding(false);
                }}
                onDelete={() => onDelete(e)}
                onToggleStar={() => onToggleStar(e)}
              />
            ),
          )
        )}
      </div>
    </div>
  );
}

function EntryRow({
  entry,
  onEdit,
  onDelete,
  onToggleStar,
}: {
  entry: DictionaryEntry;
  onEdit: () => void;
  onDelete: () => void;
  onToggleStar: () => void;
}) {
  return (
    <div className="flex items-center gap-3 rounded-xl border border-border bg-surface px-4 py-3">
      <button
        onClick={onToggleStar}
        title={entry.isStarred ? "Unstar" : "Star (always boost)"}
        className={
          "shrink-0 rounded-lg p-1 " +
          (entry.isStarred ? "text-accent" : "text-ink-faint hover:text-ink")
        }
      >
        <Star size={16} fill={entry.isStarred ? "currentColor" : "none"} />
      </button>

      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="truncate font-medium text-ink">{entry.word}</span>
          {entry.replacement && (
            <span className="truncate text-sm text-ink-soft">→ {entry.replacement}</span>
          )}
        </div>
        <div className="mt-0.5 text-xs text-ink-faint">
          {entry.replacement ? "correction" : "boost only"}
          {entry.source !== "user" && ` · ${entry.source}`}
        </div>
      </div>

      <button
        onClick={onEdit}
        title="Edit"
        className="shrink-0 rounded-lg p-1.5 text-ink-faint hover:bg-surface-2 hover:text-ink"
      >
        <Pencil size={15} />
      </button>
      <button
        onClick={onDelete}
        title="Delete"
        className="shrink-0 rounded-lg p-1.5 text-ink-faint hover:bg-surface-2 hover:text-danger"
      >
        <Trash2 size={15} />
      </button>
    </div>
  );
}

function EntryForm({
  entry,
  onSave,
  onCancel,
}: {
  entry?: DictionaryEntry;
  onSave: (word: string, replacement: string | null, isStarred: boolean) => void;
  onCancel: () => void;
}) {
  const [word, setWord] = useState(entry?.word ?? "");
  const [replacement, setReplacement] = useState(entry?.replacement ?? "");
  const [starred, setStarred] = useState(entry?.isStarred ?? false);

  const submit = () => {
    if (!word.trim()) return;
    onSave(word.trim(), replacement.trim() || null, starred);
  };

  return (
    <div className="rounded-xl border border-accent/40 bg-surface p-4">
      <div className="flex flex-col gap-3 sm:flex-row">
        <label className="flex-1">
          <span className="mb-1 block text-xs text-ink-faint">Word / phrase</span>
          <input
            autoFocus
            value={word}
            onChange={(e) => setWord(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && submit()}
            placeholder="Tailwind"
            className="w-full rounded-lg border border-border bg-surface px-3 py-2 outline-none focus:border-accent"
          />
        </label>
        <label className="flex-1">
          <span className="mb-1 block text-xs text-ink-faint">Replacement (optional)</span>
          <input
            value={replacement}
            onChange={(e) => setReplacement(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && submit()}
            placeholder="leave blank to only boost"
            className="w-full rounded-lg border border-border bg-surface px-3 py-2 outline-none focus:border-accent"
          />
        </label>
      </div>

      <div className="mt-3 flex items-center justify-between">
        <label className="flex cursor-pointer items-center gap-2 text-sm text-ink-soft">
          <input
            type="checkbox"
            checked={starred}
            onChange={(e) => setStarred(e.target.checked)}
            className="accent-accent"
          />
          Star (always boost)
        </label>
        <div className="flex items-center gap-2">
          <button
            onClick={onCancel}
            className="flex items-center gap-1 rounded-lg px-3 py-1.5 text-sm text-ink-soft hover:bg-surface-2"
          >
            <X size={14} /> Cancel
          </button>
          <button
            onClick={submit}
            disabled={!word.trim()}
            className="flex items-center gap-1 rounded-lg bg-accent px-3 py-1.5 text-sm font-medium text-white hover:opacity-90 disabled:opacity-40"
          >
            <Check size={14} /> Save
          </button>
        </div>
      </div>
    </div>
  );
}

function EmptyState({ searching }: { searching: boolean }) {
  return (
    <div className="rounded-2xl border border-dashed border-border bg-surface/50 p-10 text-center">
      <p className="text-ink-soft">
        {searching ? "No terms match your search." : "Your dictionary is empty."}
      </p>
      {!searching && (
        <p className="mt-1 text-sm text-ink-faint">
          Add a name or term Eve keeps mishearing — it'll be boosted on every dictation.
        </p>
      )}
    </div>
  );
}
