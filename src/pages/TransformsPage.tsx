import { useCallback, useEffect, useState } from "react";
import { Trash2, Plus, Pencil, X, Check, Power, Wand2, Keyboard, Repeat } from "lucide-react";
import { api, type Transform } from "../lib/api";

// Scope options for auto-apply: "" = every app, otherwise a focused-app category.
const CATEGORIES: { value: string; label: string }[] = [
  { value: "", label: "All apps" },
  { value: "email", label: "Email" },
  { value: "workmsg", label: "Work chat" },
  { value: "personalmsg", label: "Personal chat" },
  { value: "code", label: "Code" },
  { value: "other", label: "Everything else" },
];

const SHORTCUT_CHOICES = [
  "",
  "CmdOrCtrl+Shift+1",
  "CmdOrCtrl+Shift+2",
  "CmdOrCtrl+Shift+3",
  "CmdOrCtrl+Alt+R",
  "CmdOrCtrl+Alt+T",
];

export function TransformsPage() {
  const [items, setItems] = useState<Transform[]>([]);
  const [loading, setLoading] = useState(true);
  const [editing, setEditing] = useState<Transform | null>(null);
  const [adding, setAdding] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      setItems(await api.getTransforms());
    } catch {
      setItems([]);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const onSave = async (t: Omit<Transform, "createdAt" | "updatedAt">) => {
    await api
      .upsertTransform({
        id: t.id || null,
        name: t.name,
        systemPrompt: t.systemPrompt,
        shortcut: t.shortcut,
        autoApply: t.autoApply,
        appCategory: t.appCategory,
        isActive: t.isActive,
      })
      .catch(() => {});
    setEditing(null);
    setAdding(false);
    load();
  };

  const onDelete = async (t: Transform) => {
    await api.deleteTransform(t.id).catch(() => {});
    setItems((xs) => xs.filter((x) => x.id !== t.id));
  };

  const onToggleActive = async (t: Transform) => {
    await api
      .upsertTransform({
        id: t.id,
        name: t.name,
        systemPrompt: t.systemPrompt,
        shortcut: t.shortcut,
        autoApply: t.autoApply,
        appCategory: t.appCategory,
        isActive: !t.isActive,
      })
      .catch(() => {});
    load();
  };

  return (
    <div>
      <div className="flex items-center justify-between">
        <h1 className="font-serif text-3xl">Transforms</h1>
        <button
          onClick={() => {
            setAdding(true);
            setEditing(null);
          }}
          className="flex items-center gap-1 rounded-xl bg-accent px-4 py-2 text-sm font-medium text-white hover:opacity-90"
        >
          <Plus size={16} /> New transform
        </button>
      </div>

      <p className="mt-2 max-w-lg text-sm text-ink-soft">
        A transform is a saved rewrite prompt. Bind it to a shortcut to rewrite your
        current selection on the fly, or turn on <span className="font-medium text-ink">auto-apply</span>{" "}
        to run it on every dictation. You can also rewrite by voice anytime with{" "}
        <span className="font-medium text-ink">Command Mode</span>.
      </p>

      {adding && (
        <div className="mt-5">
          <TransformForm onSave={onSave} onCancel={() => setAdding(false)} />
        </div>
      )}

      <div className="mt-6 space-y-2">
        {loading ? (
          <p className="text-sm text-ink-faint">Loading…</p>
        ) : items.length === 0 && !adding ? (
          <EmptyState />
        ) : (
          items.map((t) =>
            editing?.id === t.id ? (
              <TransformForm
                key={t.id}
                transform={t}
                onSave={onSave}
                onCancel={() => setEditing(null)}
              />
            ) : (
              <TransformRow
                key={t.id}
                transform={t}
                onEdit={() => {
                  setEditing(t);
                  setAdding(false);
                }}
                onDelete={() => onDelete(t)}
                onToggleActive={() => onToggleActive(t)}
              />
            ),
          )
        )}
      </div>
    </div>
  );
}

function TransformRow({
  transform,
  onEdit,
  onDelete,
  onToggleActive,
}: {
  transform: Transform;
  onEdit: () => void;
  onDelete: () => void;
  onToggleActive: () => void;
}) {
  const catLabel = CATEGORIES.find((c) => c.value === transform.appCategory)?.label ?? "All apps";
  return (
    <div
      className={
        "flex items-center gap-3 rounded-xl border border-border bg-surface px-4 py-3 " +
        (transform.isActive ? "" : "opacity-60")
      }
    >
      <button
        onClick={onToggleActive}
        title={transform.isActive ? "Disable" : "Enable"}
        className={
          "shrink-0 rounded-lg p-1 " +
          (transform.isActive ? "text-accent" : "text-ink-faint hover:text-ink")
        }
      >
        <Power size={16} />
      </button>

      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <Wand2 size={14} className="shrink-0 text-ink-faint" />
          <span className="truncate font-medium text-ink">{transform.name}</span>
        </div>
        <div className="mt-1 flex flex-wrap items-center gap-2 text-xs text-ink-faint">
          {transform.shortcut && (
            <span className="flex items-center gap-1">
              <Keyboard size={12} />
              <kbd className="rounded border border-border bg-surface-2 px-1.5 py-0.5 font-mono">
                {transform.shortcut}
              </kbd>
            </span>
          )}
          {transform.autoApply && (
            <span className="flex items-center gap-1 text-accent">
              <Repeat size={12} /> auto · {catLabel}
            </span>
          )}
          {transform.systemPrompt && (
            <span className="truncate text-ink-soft">{transform.systemPrompt}</span>
          )}
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

function TransformForm({
  transform,
  onSave,
  onCancel,
}: {
  transform?: Transform;
  onSave: (t: Omit<Transform, "createdAt" | "updatedAt">) => void;
  onCancel: () => void;
}) {
  const [name, setName] = useState(transform?.name ?? "");
  const [systemPrompt, setSystemPrompt] = useState(transform?.systemPrompt ?? "");
  const [shortcut, setShortcut] = useState(transform?.shortcut ?? "");
  const [autoApply, setAutoApply] = useState(transform?.autoApply ?? false);
  const [appCategory, setAppCategory] = useState(transform?.appCategory ?? "");
  const [active, setActive] = useState(transform?.isActive ?? true);

  const submit = () => {
    if (!name.trim() || !systemPrompt.trim()) return;
    onSave({
      id: transform?.id ?? 0,
      name: name.trim(),
      systemPrompt: systemPrompt.trim(),
      shortcut,
      autoApply,
      appCategory,
      isActive: active,
    });
  };

  return (
    <div className="rounded-xl border border-accent/40 bg-surface p-4">
      <div className="flex flex-col gap-3">
        <label>
          <span className="mb-1 block text-xs text-ink-faint">Name</span>
          <input
            autoFocus
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="Make concise"
            className="w-full rounded-lg border border-border bg-surface px-3 py-2 outline-none focus:border-accent"
          />
        </label>
        <label>
          <span className="mb-1 block text-xs text-ink-faint">Instruction (prompt)</span>
          <textarea
            value={systemPrompt}
            onChange={(e) => setSystemPrompt(e.target.value)}
            placeholder="Rewrite the text to be more concise without losing meaning."
            rows={3}
            className="w-full resize-y rounded-lg border border-border bg-surface px-3 py-2 outline-none focus:border-accent"
          />
        </label>

        <div className="grid grid-cols-2 gap-3">
          <label>
            <span className="mb-1 block text-xs text-ink-faint">Shortcut</span>
            <select
              value={shortcut}
              onChange={(e) => setShortcut(e.target.value)}
              className="w-full rounded-lg border border-border bg-surface px-3 py-2 outline-none focus:border-accent"
            >
              {SHORTCUT_CHOICES.map((s) => (
                <option key={s || "none"} value={s}>
                  {s || "None"}
                </option>
              ))}
            </select>
          </label>
          <label>
            <span className="mb-1 block text-xs text-ink-faint">Auto-apply scope</span>
            <select
              value={appCategory}
              onChange={(e) => setAppCategory(e.target.value)}
              disabled={!autoApply}
              className="w-full rounded-lg border border-border bg-surface px-3 py-2 outline-none focus:border-accent disabled:opacity-50"
            >
              {CATEGORIES.map((c) => (
                <option key={c.value || "all"} value={c.value}>
                  {c.label}
                </option>
              ))}
            </select>
          </label>
        </div>
      </div>

      <div className="mt-3 flex items-center justify-between">
        <div className="flex items-center gap-4">
          <label className="flex cursor-pointer items-center gap-2 text-sm text-ink-soft">
            <input
              type="checkbox"
              checked={autoApply}
              onChange={(e) => setAutoApply(e.target.checked)}
              className="accent-accent"
            />
            Auto-apply after dictation
          </label>
          <label className="flex cursor-pointer items-center gap-2 text-sm text-ink-soft">
            <input
              type="checkbox"
              checked={active}
              onChange={(e) => setActive(e.target.checked)}
              className="accent-accent"
            />
            Active
          </label>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={onCancel}
            className="flex items-center gap-1 rounded-lg px-3 py-1.5 text-sm text-ink-soft hover:bg-surface-2"
          >
            <X size={14} /> Cancel
          </button>
          <button
            onClick={submit}
            disabled={!name.trim() || !systemPrompt.trim()}
            className="flex items-center gap-1 rounded-lg bg-accent px-3 py-1.5 text-sm font-medium text-white hover:opacity-90 disabled:opacity-40"
          >
            <Check size={14} /> Save
          </button>
        </div>
      </div>
    </div>
  );
}

function EmptyState() {
  return (
    <div className="rounded-2xl border border-dashed border-border bg-surface/50 p-10 text-center">
      <p className="text-ink-soft">You haven't created any transforms yet.</p>
      <p className="mt-1 text-sm text-ink-faint">
        Save a rewrite prompt like “make this more concise” or “translate to formal
        English”, bind it to a shortcut, and run it on any selected text.
      </p>
    </div>
  );
}
