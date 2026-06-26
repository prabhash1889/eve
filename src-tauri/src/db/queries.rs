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

/// Per-app-category usage breakdown (Phase 8 Insights).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUsage {
    pub category: String,
    pub sessions: i64,
    pub words: i64,
}

/// One day's totals, for the Insights streak heatmap (Phase 8).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyPoint {
    /// Local calendar day, `YYYY-MM-DD`.
    pub date: String,
    pub words: i64,
    pub sessions: i64,
}

/// Aggregate usage over a time window.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub total_words: i64,
    pub total_sessions: i64,
    pub total_ms: i64,
    /// Sum of per-session correction counts (filler/punctuation/dictionary/polish
    /// edits) recorded in `daily_stats` within the window (Phase 8).
    pub corrections: i64,
    /// Sessions + words grouped by focused-app category (Phase 8).
    pub app_usage: Vec<AppUsage>,
    /// Per-day word/session totals for the streak heatmap (Phase 8).
    pub daily: Vec<DailyPoint>,
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
    let (total_words, total_sessions, total_ms) = conn.query_row(
        "SELECT COALESCE(SUM(word_count), 0), COUNT(*), COALESCE(SUM(duration_ms), 0)
           FROM transcripts
          WHERE deleted_at IS NULL AND created_at >= ?1",
        params![since],
        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)),
    )?;

    // Corrections come from the forward-only `daily_stats` rollup. Compare the
    // text `date` column against `since` converted to a local calendar day.
    let corrections: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(correction_count), 0) FROM daily_stats
              WHERE date >= date(?1 / 1000, 'unixepoch', 'localtime')",
            params![since],
            |r| r.get(0),
        )
        .unwrap_or(0);

    // App-usage breakdown (empty category normalized to "other").
    let app_usage = {
        let mut stmt = conn.prepare(
            "SELECT CASE WHEN app_category = '' THEN 'other' ELSE app_category END AS cat,
                    COUNT(*), COALESCE(SUM(word_count), 0)
               FROM transcripts
              WHERE deleted_at IS NULL AND created_at >= ?1
              GROUP BY cat
              ORDER BY COUNT(*) DESC",
        )?;
        let rows = stmt
            .query_map(params![since], |r| {
                Ok(AppUsage {
                    category: r.get(0)?,
                    sessions: r.get(1)?,
                    words: r.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };

    // Per-day series (local calendar day) for the streak heatmap.
    let daily = {
        let mut stmt = conn.prepare(
            "SELECT date(created_at / 1000, 'unixepoch', 'localtime') AS d,
                    COALESCE(SUM(word_count), 0), COUNT(*)
               FROM transcripts
              WHERE deleted_at IS NULL AND created_at >= ?1
              GROUP BY d
              ORDER BY d ASC",
        )?;
        let rows = stmt
            .query_map(params![since], |r| {
                Ok(DailyPoint {
                    date: r.get(0)?,
                    words: r.get(1)?,
                    sessions: r.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };

    Ok(Stats {
        total_words,
        total_sessions,
        total_ms,
        corrections,
        app_usage,
        daily,
        since,
    })
}

/// Phase 8: fold one finished dictation into the `daily_stats` rollup, keyed by
/// the session's local calendar day (derived from `created_at` so reads and
/// writes use the same day boundary). Increments the counters and bumps the
/// session's app-category tally inside the `app_usage` JSON map.
pub fn record_daily(
    conn: &Connection,
    created_at: i64,
    words: i64,
    duration_ms: i64,
    corrections: i64,
    category: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO daily_stats
            (date, word_count, session_count, total_ms, correction_count, app_usage)
         VALUES (date(?1 / 1000, 'unixepoch', 'localtime'), ?2, 1, ?3, ?4, '{}')
         ON CONFLICT(date) DO UPDATE SET
            word_count       = word_count + ?2,
            session_count    = session_count + 1,
            total_ms         = total_ms + ?3,
            correction_count = correction_count + ?4",
        params![created_at, words, duration_ms, corrections],
    )?;

    // Read-modify-write the JSON usage map for this day (SQLite has no native
    // JSON object merge with arithmetic we can lean on portably).
    let date: String = conn.query_row(
        "SELECT date(?1 / 1000, 'unixepoch', 'localtime')",
        params![created_at],
        |r| r.get(0),
    )?;
    let usage_json: String = conn.query_row(
        "SELECT app_usage FROM daily_stats WHERE date = ?1",
        params![date],
        |r| r.get(0),
    )?;
    let mut map: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&usage_json).unwrap_or_default();
    let cat = if category.is_empty() { "other" } else { category };
    let n = map.get(cat).and_then(|v| v.as_i64()).unwrap_or(0) + 1;
    map.insert(cat.to_string(), serde_json::json!(n));
    let updated = serde_json::to_string(&map).unwrap_or_else(|_| "{}".into());
    conn.execute(
        "UPDATE daily_stats SET app_usage = ?2 WHERE date = ?1",
        params![date, updated],
    )?;
    Ok(())
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
