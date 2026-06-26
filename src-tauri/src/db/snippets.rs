//! Snippet queries (Phase 5). A snippet maps a spoken `trigger_phrase` to a
//! longer `expansion` that is substituted into the text after `finalize`, just
//! before injection. Structs serialize as `camelCase` for the TS mirror and
//! also (de)serialize for JSON import/export.

use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

/// One snippet as shown on the Snippets page.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snippet {
    pub id: i64,
    pub trigger_phrase: String,
    pub expansion: String,
    pub is_active: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// A trigger/expansion pair as carried in a JSON import/export file. `isActive`
/// is optional on import (defaults to true).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnippetImport {
    pub trigger_phrase: String,
    pub expansion: String,
    #[serde(default = "default_true")]
    pub is_active: bool,
}

fn default_true() -> bool {
    true
}

fn row_to_snippet(row: &Row) -> rusqlite::Result<Snippet> {
    Ok(Snippet {
        id: row.get("id")?,
        trigger_phrase: row.get("trigger_phrase")?,
        expansion: row.get("expansion")?,
        is_active: row.get::<_, i64>("is_active")? != 0,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

/// Insert or update a snippet by `trigger_phrase` (UNIQUE, case-insensitive).
/// Returns the row id. On conflict, updates the expansion/active flag and bumps
/// `updated_at`, preserving the original `created_at`.
pub fn upsert(
    conn: &Connection,
    trigger_phrase: &str,
    expansion: &str,
    is_active: bool,
    now: i64,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO snippets (trigger_phrase, expansion, is_active, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?4)
         ON CONFLICT(trigger_phrase) DO UPDATE SET
             expansion  = excluded.expansion,
             is_active  = excluded.is_active,
             updated_at = excluded.updated_at",
        params![trigger_phrase, expansion, is_active as i64, now],
    )?;
    let id: i64 = conn.query_row(
        "SELECT id FROM snippets WHERE trigger_phrase = ?1",
        params![trigger_phrase],
        |r| r.get(0),
    )?;
    Ok(id)
}

/// All snippets, optionally filtered by a substring of trigger or expansion.
/// Active first, then most-recently-updated.
pub fn list(conn: &Connection, query: Option<&str>) -> rusqlite::Result<Vec<Snippet>> {
    let order = "ORDER BY is_active DESC, updated_at DESC";
    if let Some(q) = query.map(str::trim).filter(|q| !q.is_empty()) {
        let like = format!("%{q}%");
        let mut stmt = conn.prepare(&format!(
            "SELECT * FROM snippets
              WHERE trigger_phrase LIKE ?1 OR expansion LIKE ?1 {order}"
        ))?;
        let rows = stmt
            .query_map(params![like], row_to_snippet)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    } else {
        let mut stmt = conn.prepare(&format!("SELECT * FROM snippets {order}"))?;
        let rows = stmt
            .query_map([], row_to_snippet)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}

pub fn delete(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM snippets WHERE id = ?1", params![id])?;
    Ok(())
}

/// Active trigger→expansion pairs applied in the pipeline. Longer triggers first
/// so multi-word phrases win over any shorter substrings.
pub fn active_expansions(conn: &Connection) -> rusqlite::Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT trigger_phrase, expansion FROM snippets
          WHERE is_active <> 0
          ORDER BY LENGTH(trigger_phrase) DESC",
    )?;
    let pairs = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(pairs)
}
