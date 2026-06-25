//! Typed queries over the history database. Structs serialize as `camelCase` to
//! match the TypeScript mirror in `src/lib/api.ts`.

use rusqlite::{params, Connection, Row};
use serde::Serialize;

/// A stored dictation, returned to the History page.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Transcript {
    pub id: i64,
    pub created_at: i64,
    pub raw_text: String,
    pub polished_text: String,
    pub cleanup_level: String,
    pub language: String,
    pub audio_path: Option<String>,
    pub app_process: String,
    pub app_title: String,
    pub app_category: String,
    pub word_count: i64,
    pub duration_ms: i64,
    pub was_polished: bool,
    pub deleted_at: Option<i64>,
}

/// Fields for a freshly recorded dictation (id/created_at are assigned on insert).
pub struct NewTranscript {
    pub created_at: i64,
    pub raw_text: String,
    pub polished_text: String,
    pub cleanup_level: String,
    pub language: String,
    pub audio_path: Option<String>,
    pub app_process: String,
    pub app_title: String,
    pub app_category: String,
    pub word_count: i64,
    pub duration_ms: i64,
    pub was_polished: bool,
}

/// One page of history plus the total matching count (for pagination UI).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPage {
    pub items: Vec<Transcript>,
    pub total: i64,
    pub page: i64,
    pub per_page: i64,
}

/// Aggregate usage over a time window.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub total_words: i64,
    pub total_sessions: i64,
    pub total_ms: i64,
    /// The epoch-ms lower bound used for this window (0 = all time).
    pub since: i64,
}

fn row_to_transcript(row: &Row) -> rusqlite::Result<Transcript> {
    Ok(Transcript {
        id: row.get("id")?,
        created_at: row.get("created_at")?,
        raw_text: row.get("raw_text")?,
        polished_text: row.get("polished_text")?,
        cleanup_level: row.get("cleanup_level")?,
        language: row.get("language")?,
        audio_path: row.get("audio_path")?,
        app_process: row.get("app_process")?,
        app_title: row.get("app_title")?,
        app_category: row.get("app_category")?,
        word_count: row.get("word_count")?,
        duration_ms: row.get("duration_ms")?,
        was_polished: row.get::<_, i64>("was_polished")? != 0,
        deleted_at: row.get("deleted_at")?,
    })
}

pub fn insert_transcript(conn: &Connection, t: &NewTranscript) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO transcripts
            (created_at, raw_text, polished_text, cleanup_level, language, audio_path,
             app_process, app_title, app_category, word_count, duration_ms, was_polished)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            t.created_at,
            t.raw_text,
            t.polished_text,
            t.cleanup_level,
            t.language,
            t.audio_path,
            t.app_process,
            t.app_title,
            t.app_category,
            t.word_count,
            t.duration_ms,
            t.was_polished as i64,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Turn free-form search input into a safe FTS5 prefix query. Each token is
/// reduced to alphanumerics and suffixed with `*` so partial words match;
/// returns `None` if nothing usable remains (caller then lists unfiltered).
fn fts_match(q: &str) -> Option<String> {
    let expr = q
        .split_whitespace()
        .map(|tok| tok.chars().filter(|c| c.is_alphanumeric()).collect::<String>())
        .filter(|t| !t.is_empty())
        .map(|t| format!("{t}*"))
        .collect::<Vec<_>>()
        .join(" ");
    if expr.is_empty() {
        None
    } else {
        Some(expr)
    }
}

pub fn get_history(
    conn: &Connection,
    page: i64,
    per_page: i64,
    query: Option<String>,
) -> rusqlite::Result<HistoryPage> {
    let page = page.max(1);
    let per_page = per_page.clamp(1, 200);
    let offset = (page - 1) * per_page;

    let match_expr = query.as_deref().and_then(fts_match);

    let (total, items) = if let Some(m) = match_expr {
        let total: i64 = conn.query_row(
            "SELECT COUNT(*) FROM transcripts t
               JOIN transcripts_fts f ON f.rowid = t.id
              WHERE f.transcripts_fts MATCH ?1 AND t.deleted_at IS NULL",
            params![m],
            |r| r.get(0),
        )?;
        let mut stmt = conn.prepare(
            "SELECT t.* FROM transcripts t
               JOIN transcripts_fts f ON f.rowid = t.id
              WHERE f.transcripts_fts MATCH ?1 AND t.deleted_at IS NULL
              ORDER BY t.created_at DESC LIMIT ?2 OFFSET ?3",
        )?;
        let rows = stmt
            .query_map(params![m, per_page, offset], row_to_transcript)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        (total, rows)
    } else {
        let total: i64 = conn.query_row(
            "SELECT COUNT(*) FROM transcripts WHERE deleted_at IS NULL",
            [],
            |r| r.get(0),
        )?;
        let mut stmt = conn.prepare(
            "SELECT * FROM transcripts WHERE deleted_at IS NULL
              ORDER BY created_at DESC LIMIT ?1 OFFSET ?2",
        )?;
        let rows = stmt
            .query_map(params![per_page, offset], row_to_transcript)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        (total, rows)
    };

    Ok(HistoryPage {
        items,
        total,
        page,
        per_page,
    })
}

/// Soft delete: mark `deleted_at` so the row drops out of history but can be
/// recovered. No-op if already deleted.
pub fn soft_delete(conn: &Connection, id: i64, now: i64) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE transcripts SET deleted_at = ?2 WHERE id = ?1 AND deleted_at IS NULL",
        params![id, now],
    )?;
    Ok(())
}

pub fn recover(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE transcripts SET deleted_at = NULL WHERE id = ?1",
        params![id],
    )?;
    Ok(())
}

/// Soft-delete every currently active row (reversible per-row via `recover`).
pub fn clear_history(conn: &Connection, now: i64) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE transcripts SET deleted_at = ?1 WHERE deleted_at IS NULL",
        params![now],
    )?;
    Ok(())
}

pub fn get_stats(conn: &Connection, since: i64) -> rusqlite::Result<Stats> {
    conn.query_row(
        "SELECT COALESCE(SUM(word_count), 0), COUNT(*), COALESCE(SUM(duration_ms), 0)
           FROM transcripts
          WHERE deleted_at IS NULL AND created_at >= ?1",
        params![since],
        |r| {
            Ok(Stats {
                total_words: r.get(0)?,
                total_sessions: r.get(1)?,
                total_ms: r.get(2)?,
                since,
            })
        },
    )
}

/// Find saved audio recorded before `cutoff`, clear their `audio_path`, and
/// return the file paths so the caller can delete them from disk. Transcript
/// text is preserved — only the audio ages out.
pub fn prune_audio(conn: &Connection, cutoff: i64) -> rusqlite::Result<Vec<String>> {
    let paths: Vec<String> = {
        let mut stmt = conn.prepare(
            "SELECT audio_path FROM transcripts
              WHERE audio_path IS NOT NULL AND created_at < ?1",
        )?;
        let rows = stmt
            .query_map(params![cutoff], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };
    conn.execute(
        "UPDATE transcripts SET audio_path = NULL
          WHERE audio_path IS NOT NULL AND created_at < ?1",
        params![cutoff],
    )?;
    Ok(paths)
}
