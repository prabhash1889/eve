//! Flow Style queries (Phase 6). A Flow Style adapts the polish prompt to the
//! focused app's category (one style per category). `active_for` is the pipeline
//! lookup; the rest back the Styles page CRUD. Structs serialize as `camelCase`
//! for the TS mirror.

use rusqlite::{params, Connection, Row};
use serde::Serialize;

/// One Flow Style as shown on the Styles page.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FlowStyle {
    pub id: i64,
    pub name: String,
    pub app_category: String,
    pub tone: String,
    pub system_prompt: String,
    pub writing_sample: String,
    pub is_active: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

fn row_to_style(row: &Row) -> rusqlite::Result<FlowStyle> {
    Ok(FlowStyle {
        id: row.get("id")?,
        name: row.get("name")?,
        app_category: row.get("app_category")?,
        tone: row.get("tone")?,
        system_prompt: row.get("system_prompt")?,
        writing_sample: row.get("writing_sample")?,
        is_active: row.get::<_, i64>("is_active")? != 0,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

/// Insert or update the style for a category (UNIQUE). Returns the row id. On
/// conflict, updates the fields and bumps `updated_at`, preserving `created_at`.
#[allow(clippy::too_many_arguments)]
pub fn upsert(
    conn: &Connection,
    name: &str,
    app_category: &str,
    tone: &str,
    system_prompt: &str,
    writing_sample: &str,
    is_active: bool,
    now: i64,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO flow_styles
            (name, app_category, tone, system_prompt, writing_sample, is_active, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
         ON CONFLICT(app_category) DO UPDATE SET
             name           = excluded.name,
             tone           = excluded.tone,
             system_prompt  = excluded.system_prompt,
             writing_sample = excluded.writing_sample,
             is_active      = excluded.is_active,
             updated_at     = excluded.updated_at",
        params![
            name,
            app_category,
            tone,
            system_prompt,
            writing_sample,
            is_active as i64,
            now
        ],
    )?;
    let id: i64 = conn.query_row(
        "SELECT id FROM flow_styles WHERE app_category = ?1",
        params![app_category],
        |r| r.get(0),
    )?;
    Ok(id)
}

/// All styles, ordered by category for a stable grid.
pub fn list(conn: &Connection) -> rusqlite::Result<Vec<FlowStyle>> {
    let mut stmt = conn.prepare("SELECT * FROM flow_styles ORDER BY app_category")?;
    let rows = stmt
        .query_map([], row_to_style)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn delete(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM flow_styles WHERE id = ?1", params![id])?;
    Ok(())
}

/// The active style for a category, or `None` if there isn't one (or it's
/// disabled). Used by the pipeline to shape the polish prompt.
pub fn active_for(conn: &Connection, app_category: &str) -> rusqlite::Result<Option<FlowStyle>> {
    let mut stmt = conn
        .prepare("SELECT * FROM flow_styles WHERE app_category = ?1 AND is_active <> 0 LIMIT 1")?;
    let mut rows = stmt.query_map(params![app_category], row_to_style)?;
    match rows.next() {
        Some(r) => Ok(Some(r?)),
        None => Ok(None),
    }
}
