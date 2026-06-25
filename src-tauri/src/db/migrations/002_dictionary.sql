-- Phase 4: custom dictionary. Boosts recognition of starred/known terms (passed
-- to Whisper as a prompt) and applies misspelling→correction substitutions in
-- the pipeline after transcription.

CREATE TABLE dictionary (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    word          TEXT    NOT NULL UNIQUE COLLATE NOCASE,  -- the spoken/heard term
    replacement   TEXT,                          -- NULL = boost-only (no substitution)
    is_starred    INTEGER NOT NULL DEFAULT 0,    -- starred terms always boosted
    source        TEXT    NOT NULL DEFAULT 'user', -- user | auto | import
    learned_count INTEGER NOT NULL DEFAULT 0,    -- times auto-learn saw this term
    created_at    INTEGER NOT NULL,              -- unix epoch ms (UTC)
    updated_at    INTEGER NOT NULL
);

CREATE INDEX idx_dictionary_starred ON dictionary(is_starred DESC, updated_at DESC);
