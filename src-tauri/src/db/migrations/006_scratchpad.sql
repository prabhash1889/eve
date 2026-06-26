-- Phase 9: scratchpad. The floating Scratchpad window keeps multiple rich-text
-- tabs you can dictate into. Each row stores the tab's title, HTML content, and
-- left-to-right position. Autosaved from the UI on every edit.

CREATE TABLE scratchpad_tabs (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    title      TEXT    NOT NULL DEFAULT 'Untitled',  -- tab label
    content    TEXT    NOT NULL DEFAULT '',           -- editor HTML
    position   INTEGER NOT NULL DEFAULT 0,            -- left-to-right order
    created_at INTEGER NOT NULL,                      -- unix epoch ms (UTC)
    updated_at INTEGER NOT NULL
);

CREATE INDEX idx_scratchpad_position ON scratchpad_tabs(position ASC, id ASC);
