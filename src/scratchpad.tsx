import { useCallback, useEffect, useRef, useState } from "react";
import ReactDOM from "react-dom/client";
import { useEditor, EditorContent, type Editor } from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";
import Image from "@tiptap/extension-image";
import { Plus, X } from "lucide-react";
import "@fontsource/figtree/400.css";
import "@fontsource/figtree/500.css";
import "@fontsource/figtree/600.css";
import "@fontsource/fraunces/500.css";
import "@fontsource/fraunces/600.css";
import "./styles/globals.css";
import { api, on, EVT, type ScratchpadTab, type TranscriptPayload } from "./lib/api";

// Respect the system theme (the Scratchpad has no toggle of its own).
if (window.matchMedia("(prefers-color-scheme: dark)").matches) {
  document.documentElement.classList.add("dark");
}

/** Escape HTML and turn a plain-text dictation into paragraph/line-break HTML. */
function textToHtml(text: string): string {
  const esc = (s: string) =>
    s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  return text
    .split(/\n\n+/)
    .map((p) => `<p>${esc(p).replace(/\n/g, "<br>")}</p>`)
    .join("");
}

function Scratchpad() {
  const [tabs, setTabs] = useState<ScratchpadTab[]>([]);
  const [activeId, setActiveId] = useState<number | null>(null);

  // Refs so the editor callbacks (created once) always see the latest values.
  const editorRef = useRef<Editor | null>(null);
  const activeIdRef = useRef<number | null>(null);
  const tabsRef = useRef<ScratchpadTab[]>([]);
  const saveTimer = useRef<number | null>(null);
  // The save currently waiting on `saveTimer`, so we can flush or cancel it
  // before switching/closing tabs (otherwise the debounce can drop the last
  // edits or fire against a deleted tab).
  const pending = useRef<{ id: number; title: string; content: string } | null>(null);
  activeIdRef.current = activeId;
  tabsRef.current = tabs;

  const editor = useEditor({
    extensions: [
      StarterKit,
      Image.configure({ inline: false, allowBase64: true }),
    ],
    content: "",
    editorProps: {
      attributes: { class: "scratchpad-editor" },
      // Paste an image from the clipboard as an inline base64 data URL.
      handlePaste(_view, event) {
        const items = event.clipboardData?.items;
        if (!items) return false;
        for (const item of items) {
          if (item.type.startsWith("image/")) {
            const file = item.getAsFile();
            if (!file) continue;
            const reader = new FileReader();
            reader.onload = () => {
              const src = reader.result as string;
              editorRef.current?.chain().focus().setImage({ src }).run();
            };
            reader.readAsDataURL(file);
            return true;
          }
        }
        return false;
      },
    },
    onUpdate: ({ editor }) => onEditorChange(editor.getHTML()),
  });

  useEffect(() => {
    editorRef.current = editor;
  }, [editor]);

  // Debounced autosave of the active tab's title + content.
  const persist = useCallback((id: number, title: string, content: string) => {
    if (saveTimer.current) window.clearTimeout(saveTimer.current);
    pending.current = { id, title, content };
    saveTimer.current = window.setTimeout(() => {
      api.saveScratchpadTab(id, title, content).catch(() => {});
      pending.current = null;
      saveTimer.current = null;
    }, 500);
  }, []);

  // Immediately write any pending debounced save and clear the timer.
  const flushPending = useCallback(() => {
    if (saveTimer.current) {
      window.clearTimeout(saveTimer.current);
      saveTimer.current = null;
    }
    const p = pending.current;
    if (p) {
      api.saveScratchpadTab(p.id, p.title, p.content).catch(() => {});
      pending.current = null;
    }
  }, []);

  // Drop a pending save without writing it (used when its tab is being deleted).
  const cancelPending = useCallback(() => {
    if (saveTimer.current) {
      window.clearTimeout(saveTimer.current);
      saveTimer.current = null;
    }
    pending.current = null;
  }, []);

  const onEditorChange = useCallback(
    (html: string) => {
      const id = activeIdRef.current;
      if (id == null) return;
      const title = tabsRef.current.find((t) => t.id === id)?.title ?? "Untitled";
      setTabs((ts) => ts.map((t) => (t.id === id ? { ...t, content: html } : t)));
      persist(id, title, html);
    },
    [persist],
  );

  // Load tabs once (creating a first tab if the store is empty).
  useEffect(() => {
    (async () => {
      let list = await api.getScratchpadTabs().catch(() => [] as ScratchpadTab[]);
      if (list.length === 0) {
        const first = await api.createScratchpadTab().catch(() => null);
        if (first) list = [first];
      }
      setTabs(list);
      setActiveId(list[0]?.id ?? null);
    })();
  }, []);

  // Load the active tab's content into the editor when the selection changes.
  // `emitUpdate=false` avoids re-triggering autosave on a programmatic load.
  useEffect(() => {
    if (!editor || activeId == null) return;
    const tab = tabsRef.current.find((t) => t.id === activeId);
    editor.commands.setContent(tab?.content || "", false);
  }, [editor, activeId]);

  // Phase 9: dictation routed here lands at the cursor of the active editor.
  useEffect(() => {
    const un = on<TranscriptPayload>(EVT.scratchpadInsert, (e) => {
      const ed = editorRef.current;
      if (!ed) return;
      ed.chain().focus().insertContent(textToHtml(e.payload.text)).run();
    });
    return () => {
      un.then((f) => f()).catch(() => {});
    };
  }, []);

  const switchTo = (id: number) => {
    if (id === activeId) return;
    // Flush the outgoing tab's pending edits before the editor reloads with the
    // new tab's content, so a switch within the debounce window doesn't lose them.
    flushPending();
    setActiveId(id);
  };

  const addTab = async () => {
    const tab = await api.createScratchpadTab().catch(() => null);
    if (!tab) return;
    setTabs((ts) => [...ts, tab]);
    setActiveId(tab.id);
  };

  const closeTab = async (id: number) => {
    // If the pending save targets the tab being closed, drop it so it can't
    // fire against a deleted row; otherwise flush it so a different tab's edits
    // aren't lost.
    if (pending.current?.id === id) {
      cancelPending();
    } else {
      flushPending();
    }
    await api.deleteScratchpadTab(id).catch(() => {});
    const remaining = tabs.filter((t) => t.id !== id);
    if (remaining.length === 0) {
      const tab = await api.createScratchpadTab().catch(() => null);
      setTabs(tab ? [tab] : []);
      setActiveId(tab?.id ?? null);
      return;
    }
    setTabs(remaining);
    if (activeId === id) setActiveId(remaining[remaining.length - 1].id);
  };

  const renameTab = (id: number, title: string) => {
    setTabs((ts) => ts.map((t) => (t.id === id ? { ...t, title } : t)));
    const content = tabsRef.current.find((t) => t.id === id)?.content ?? "";
    persist(id, title.trim() || "Untitled", content);
  };

  const active = tabs.find((t) => t.id === activeId) ?? null;

  return (
    <div className="flex h-screen flex-col bg-canvas text-ink">
      {/* Tab strip */}
      <div className="flex items-center gap-1 overflow-x-auto border-b border-border bg-surface/60 px-2 py-1.5">
        {tabs.map((t) => (
          <button
            key={t.id}
            onClick={() => switchTo(t.id)}
            className={
              "group flex max-w-[160px] shrink-0 items-center gap-1.5 rounded-lg px-2.5 py-1.5 text-sm transition-colors " +
              (t.id === activeId
                ? "bg-accent-soft text-ink font-medium"
                : "text-ink-soft hover:bg-surface-2")
            }
          >
            <span className="truncate">{t.title || "Untitled"}</span>
            <span
              role="button"
              tabIndex={-1}
              onClick={(e) => {
                e.stopPropagation();
                closeTab(t.id);
              }}
              className="shrink-0 rounded p-0.5 text-ink-faint opacity-0 hover:text-danger group-hover:opacity-100"
              title="Close tab"
            >
              <X size={13} />
            </span>
          </button>
        ))}
        <button
          onClick={addTab}
          title="New tab"
          className="shrink-0 rounded-lg p-1.5 text-ink-faint hover:bg-surface-2 hover:text-ink"
        >
          <Plus size={16} />
        </button>
      </div>

      {/* Title */}
      {active && (
        <input
          value={active.title}
          onChange={(e) => renameTab(active.id, e.target.value)}
          placeholder="Untitled"
          className="border-b border-border bg-transparent px-5 py-2.5 font-serif text-lg outline-none placeholder:text-ink-faint"
        />
      )}

      {/* Editor */}
      <div
        className="flex-1 cursor-text overflow-y-auto px-5 py-4"
        onClick={() => editor?.chain().focus().run()}
      >
        <EditorContent editor={editor} />
      </div>

      <div className="border-t border-border bg-surface/60 px-5 py-1.5 text-xs text-ink-faint">
        Hold your dictation hotkey here and speak — text lands at the cursor.
      </div>
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <Scratchpad />,
);
