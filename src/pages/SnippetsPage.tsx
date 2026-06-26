import { useCallback, useEffect, useRef, useState } from "react";
import { Search, Trash2, Plus, Upload, Download, Pencil, X, Check, Power } from "lucide-react";
import { api, type Snippet } from "../lib/api";

export function SnippetsPage() {
  const [query, setQuery] = useState("");
  const [items, setItems] = useState<Snippet[]>([]);
  const [loading, setLoading] = useState(true);
  const [editing, setEditing] = useState<Snippet | null>(null);
  const [adding, setAdding] = useState(false);
  const fileRef = useRef<HTMLInputElement | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      setItems(await api.getSnippets(query.trim() || undefined));
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

  const onSave = async (trigger: string, expansion: string, isActive: boolean) => {
    await api.upsertSnippet(trigger, expansion, isActive).catch(() => {});
    setEditing(null);
    setAdding(false);
    load();
  };

  const onDelete = async (s: Snippet) => {
    await api.deleteSnippet(s.id).catch(() => {});
    setItems((xs) => xs.filter((x) => x.id !== s.id));
  };

  const onToggleActive = async (s: Snippet) => {
    await api.upsertSnippet(s.triggerPhrase, s.expansion, !s.isActive).catch(() => {});
    setItems((xs) => xs.map((x) => (x.id === s.id ? { ...x, isActive: !x.isActive } : x)));
  };

  const onImportFile = async (file: File) => {
    const text = await file.text();
    const n = await api.importSnippetsJson(text).catch(() => 0);
    if (fileRef.current) fileRef.current.value = "";
    alert(`Imported ${n} ${n === 1 ? "snippet" : "snippets"}.`);
    load();
  };

  const onExport = async () => {
    const json = await api.exportSnippetsJson().catch(() => "");
    if (!json) return;
    const blob = new Blob([json], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = "eve-snippets.json";
    a.click();
    URL.revokeObjectURL(url);
  };

  return (
    <div>
      <div className="flex items-center justify-between">
        <h1 className="font-serif text-3xl">Snippets</h1>
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
            accept=".json,application/json"
            className="hidden"
            onChange={(e) => {
              const f = e.target.files?.[0];
              if (f) onImportFile(f);
            }}
          />
        </div>
      </div>

      <p className="mt-2 max-w-lg text-sm text-ink-soft">
        Say a short trigger phrase and Eve types the full text. Define{" "}
        <span className="font-medium text-ink">my email</span> → your address, then
        just say it. Triggers match case-insensitively, with a little fuzziness for
        short ones.
      </p>

      <div className="mt-5 flex items-center gap-2">
        <div className="flex flex-1 items-center gap-2 rounded-xl border border-border bg-surface px-3 py-2 focus-within:border-accent">
          <Search size={16} className="text-ink-faint" />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search snippets…"
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
          <SnippetForm onSave={onSave} onCancel={() => setAdding(false)} />
        </div>
      )}

      <div className="mt-6 space-y-2">
        {loading ? (
          <p className="text-sm text-ink-faint">Loading…</p>
        ) : items.length === 0 ? (
          <EmptyState searching={!!query.trim()} />
        ) : (
          items.map((s) =>
            editing?.id === s.id ? (
              <SnippetForm
                key={s.id}
                snippet={s}
                onSave={onSave}
                onCancel={() => setEditing(null)}
              />
            ) : (
              <SnippetRow
                key={s.id}
                snippet={s}
                onEdit={() => {
                  setEditing(s);
                  setAdding(false);
                }}
                onDelete={() => onDelete(s)}
                onToggleActive={() => onToggleActive(s)}
              />
            ),
          )
        )}
      </div>
    </div>
  );
}

function SnippetRow({
  snippet,
  onEdit,
  onDelete,
  onToggleActive,
}: {
  snippet: Snippet;
  onEdit: () => void;
  onDelete: () => void;
  onToggleActive: () => void;
}) {
  return (
    <div
      className={
        "flex items-center gap-3 rounded-xl border border-border bg-surface px-4 py-3 " +
        (snippet.isActive ? "" : "opacity-60")
      }
    >
      <button
        onClick={onToggleActive}
        title={snippet.isActive ? "Disable (skip in dictation)" : "Enable"}
        className={
          "shrink-0 rounded-lg p-1 " +
          (snippet.isActive ? "text-accent" : "text-ink-faint hover:text-ink")
        }
      >
        <Power size={16} />
      </button>

      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="truncate font-medium text-ink">{snippet.triggerPhrase}</span>
          <span className="shrink-0 text-ink-faint">→</span>
          <span className="truncate text-sm text-ink-soft">{snippet.expansion}</span>
        </div>
        <div className="mt-0.5 text-xs text-ink-faint">
          {snippet.isActive ? "active" : "disabled"}
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

function SnippetForm({
  snippet,
  onSave,
  onCancel,
}: {
  snippet?: Snippet;
  onSave: (trigger: string, expansion: string, isActive: boolean) => void;
  onCancel: () => void;
}) {
  const [trigger, setTrigger] = useState(snippet?.triggerPhrase ?? "");
  const [expansion, setExpansion] = useState(snippet?.expansion ?? "");
  const [active, setActive] = useState(snippet?.isActive ?? true);

  const submit = () => {
    if (!trigger.trim() || !expansion.trim()) return;
    onSave(trigger.trim(), expansion.trim(), active);
  };

  return (
    <div className="rounded-xl border border-accent/40 bg-surface p-4">
      <div className="flex flex-col gap-3">
        <label>
          <span className="mb-1 block text-xs text-ink-faint">Trigger phrase</span>
          <input
            autoFocus
            value={trigger}
            onChange={(e) => setTrigger(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && submit()}
            placeholder="my email"
            className="w-full rounded-lg border border-border bg-surface px-3 py-2 outline-none focus:border-accent"
          />
        </label>
        <label>
          <span className="mb-1 block text-xs text-ink-faint">Expands to</span>
          <textarea
            value={expansion}
            onChange={(e) => setExpansion(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) submit();
            }}
            placeholder="bob@example.com"
            rows={3}
            className="w-full resize-y rounded-lg border border-border bg-surface px-3 py-2 outline-none focus:border-accent"
          />
        </label>
      </div>

      <div className="mt-3 flex items-center justify-between">
        <label className="flex cursor-pointer items-center gap-2 text-sm text-ink-soft">
          <input
            type="checkbox"
            checked={active}
            onChange={(e) => setActive(e.target.checked)}
            className="accent-accent"
          />
          Active
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
            disabled={!trigger.trim() || !expansion.trim()}
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
        {searching ? "No snippets match your search." : "You haven't added any snippets."}
      </p>
      {!searching && (
        <p className="mt-1 text-sm text-ink-faint">
          Map a phrase you say often — an email, an address, a sign-off — to the full
          text. Eve expands it on every dictation.
        </p>
      )}
    </div>
  );
}
