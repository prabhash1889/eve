-- Phase 5: text snippets. A spoken trigger phrase expands to long-form text in
-- the pipeline after finalize, just before injection (e.g. "my email" → the
-- full address). Matching is case-insensitive with a small fuzzy tolerance for
-- short triggers.

CREATE TABLE snippets (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    trigger_phrase TEXT    NOT NULL UNIQUE COLLATE NOCASE,  -- spoken phrase to match
    expansion      TEXT    NOT NULL,                        -- text it expands to
    is_active      INTEGER NOT NULL DEFAULT 1,              -- 0 = disabled, skipped in pipeline
    created_at     INTEGER NOT NULL,                        -- unix epoch ms (UTC)
    updated_at     INTEGER NOT NULL
);

CREATE INDEX idx_snippets_active ON snippets(is_active, updated_at DESC);
