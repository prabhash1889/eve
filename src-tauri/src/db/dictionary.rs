//! Dictionary queries (Phase 4). The dictionary drives two pipeline features:
//! starred/known terms are passed to Whisper as a soft vocabulary `prompt`
//! (boosting), and entries with a `replacement` rewrite misspellings after
//! transcription. Structs serialize as `camelCase` for the TS mirror.

use rusqlite::{params, Connection, Row};
use serde::Serialize;

/// One dictionary term as shown on the Dictionary page.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryEntry {
    pub id: i64,
    pub word: String,
    pub replacement: Option<String>,
    pub is_starred: bool,
    pub source: String,
    pub learned_count: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

fn row_to_entry(row: &Row) -> rusqlite::Result<DictionaryEntry> {
    Ok(DictionaryEntry {
        id: row.get("id")?,
        word: row.get("word")?,
        replacement: row.get("replacement")?,
        is_starred: row.get::<_, i64>("is_starred")? != 0,
        source: row.get("source")?,
        learned_count: row.get("learned_count")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

/// Insert or update a term by `word` (UNIQUE, case-insensitive). Returns the
/// row id. On conflict, updates the replacement/star/source and bumps
/// `updated_at`, preserving the original `created_at`.
pub fn upsert(
    conn: &Connection,
    word: &str,
    replacement: Option<&str>,
    is_starred: bool,
    source: &str,
    now: i64,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO dictionary (word, replacement, is_starred, source, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)
         ON CONFLICT(word) DO UPDATE SET
             replacement = excluded.replacement,
             is_starred  = excluded.is_starred,
             source      = excluded.source,
             updated_at  = excluded.updated_at",
        params![word, replacement, is_starred as i64, source, now],
    )?;
    let id: i64 = conn.query_row(
        "SELECT id FROM dictionary WHERE word = ?1",
        params![word],
        |r| r.get(0),
    )?;
    Ok(id)
}

/// All entries, optionally filtered by a substring of word or replacement.
/// Starred first, then most-recently-updated.
pub fn list(conn: &Connection, query: Option<&str>) -> rusqlite::Result<Vec<DictionaryEntry>> {
    let order = "ORDER BY is_starred DESC, updated_at DESC";
    if let Some(q) = query.map(str::trim).filter(|q| !q.is_empty()) {
        let like = format!("%{q}%");
        let mut stmt = conn.prepare(&format!(
            "SELECT * FROM dictionary
              WHERE word LIKE ?1 OR replacement LIKE ?1 {order}"
        ))?;
        let rows = stmt
            .query_map(params![like], row_to_entry)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    } else {
        let mut stmt = conn.prepare(&format!("SELECT * FROM dictionary {order}"))?;
        let rows = stmt
            .query_map([], row_to_entry)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}

pub fn delete(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM dictionary WHERE id = ?1", params![id])?;
    Ok(())
}

/// Vocabulary hints for Whisper boosting: starred terms first, then the most
/// recently updated, capped at `limit`. These become the transcription `prompt`.
pub fn hints(conn: &Connection, limit: i64) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT word FROM dictionary
          ORDER BY is_starred DESC, updated_at DESC
          LIMIT ?1",
    )?;
    let words = stmt
        .query_map(params![limit], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(words)
}

/// Misspelling→correction pairs (entries with a non-empty `replacement`),
/// applied after transcription. Longer words first so multi-word terms win
/// over any substrings.
pub fn corrections(conn: &Connection) -> rusqlite::Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT word, replacement FROM dictionary
          WHERE replacement IS NOT NULL AND replacement <> ''
          ORDER BY LENGTH(word) DESC",
    )?;
    let pairs = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(pairs)
}
