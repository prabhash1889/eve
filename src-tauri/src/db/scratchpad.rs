//! Scratchpad tab persistence (Phase 9). The floating Scratchpad window keeps
//! multiple rich-text tabs; each row stores the tab's title, HTML content, and
//! position. Rows are autosaved from the UI on every edit. Structs serialize as
//! `camelCase` for the TS mirror.

use rusqlite::{params, Connection, Row};
use serde::Serialize;

/// One Scratchpad tab as shown in the window.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScratchpadTab {
    pub id: i64,
    pub title: String,
    pub content: String,
    pub position: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

fn row_to_tab(row: &Row) -> rusqlite::Result<ScratchpadTab> {
    Ok(ScratchpadTab {
        id: row.get("id")?,
        title: row.get("title")?,
        content: row.get("content")?,
        position: row.get("position")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

/// All tabs in display order (position asc, then creation).
pub fn list(conn: &Connection) -> rusqlite::Result<Vec<ScratchpadTab>> {
    let mut stmt = conn.prepare("SELECT * FROM scratchpad_tabs ORDER BY position ASC, id ASC")?;
    let rows = stmt
        .query_map([], row_to_tab)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Create a new empty tab after the last one and return the full row.
pub fn create(conn: &Connection, title: &str, now: i64) -> rusqlite::Result<ScratchpadTab> {
    let position: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(position) + 1, 0) FROM scratchpad_tabs",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    conn.execute(
        "INSERT INTO scratchpad_tabs (title, content, position, created_at, updated_at)
         VALUES (?1, '', ?2, ?3, ?3)",
        params![title, position, now],
    )?;
    let id = conn.last_insert_rowid();
    conn.query_row(
        "SELECT * FROM scratchpad_tabs WHERE id = ?1",
        params![id],
        row_to_tab,
    )
}

/// Autosave a tab's title + content (called on every edit, debounced in the UI).
pub fn save(conn: &Connection, id: i64, title: &str, content: &str, now: i64) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE scratchpad_tabs SET title = ?2, content = ?3, updated_at = ?4 WHERE id = ?1",
        params![id, title, content, now],
    )?;
    Ok(())
}

pub fn delete(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM scratchpad_tabs WHERE id = ?1", params![id])?;
    Ok(())
}
