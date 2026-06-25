-- Phase 3 initial schema: dictation history + an FTS5 mirror for search, plus
-- a per-day rollup for the dashboard stats.

CREATE TABLE transcripts (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at    INTEGER NOT NULL,            -- unix epoch milliseconds (UTC)
    raw_text      TEXT    NOT NULL,
    polished_text TEXT    NOT NULL,
    cleanup_level TEXT    NOT NULL,
    language      TEXT    NOT NULL,
    audio_path    TEXT,                         -- absolute path to the saved WAV, or NULL
    app_process   TEXT    NOT NULL DEFAULT '',  -- filled in Phase 6
    app_title     TEXT    NOT NULL DEFAULT '',
    app_category  TEXT    NOT NULL DEFAULT '',
    word_count    INTEGER NOT NULL DEFAULT 0,
    duration_ms   INTEGER NOT NULL DEFAULT 0,
    was_polished  INTEGER NOT NULL DEFAULT 0,
    deleted_at    INTEGER                       -- soft delete; NULL = active
);

CREATE INDEX idx_transcripts_created ON transcripts(created_at DESC);

-- External-content FTS5 index mirroring raw + polished text. Kept in sync by the
-- triggers below so search and the base table never drift.
CREATE VIRTUAL TABLE transcripts_fts USING fts5(
    raw_text,
    polished_text,
    content='transcripts',
    content_rowid='id'
);

CREATE TRIGGER transcripts_ai AFTER INSERT ON transcripts BEGIN
    INSERT INTO transcripts_fts(rowid, raw_text, polished_text)
    VALUES (new.id, new.raw_text, new.polished_text);
END;

CREATE TRIGGER transcripts_ad AFTER DELETE ON transcripts BEGIN
    INSERT INTO transcripts_fts(transcripts_fts, rowid, raw_text, polished_text)
    VALUES ('delete', old.id, old.raw_text, old.polished_text);
END;

CREATE TRIGGER transcripts_au AFTER UPDATE ON transcripts BEGIN
    INSERT INTO transcripts_fts(transcripts_fts, rowid, raw_text, polished_text)
    VALUES ('delete', old.id, old.raw_text, old.polished_text);
    INSERT INTO transcripts_fts(rowid, raw_text, polished_text)
    VALUES (new.id, new.raw_text, new.polished_text);
END;

CREATE TABLE daily_stats (
    date             TEXT PRIMARY KEY,          -- YYYY-MM-DD (local)
    word_count       INTEGER NOT NULL DEFAULT 0,
    session_count    INTEGER NOT NULL DEFAULT 0,
    total_ms         INTEGER NOT NULL DEFAULT 0,
    correction_count INTEGER NOT NULL DEFAULT 0,
    app_usage        TEXT    NOT NULL DEFAULT '{}'  -- JSON: {category: count}
);
