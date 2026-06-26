//! Transform queries (Phase 7). A transform is a saved rewrite prompt that can
//! be triggered on the current selection via a bound shortcut, or auto-applied
//! to dictated text after polish. Structs serialize as `camelCase` for the TS
//! mirror.

use rusqlite::{params, Connection, Row};
use serde::Serialize;

/// One transform as shown on the Transforms page.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Transform {
    pub id: i64,
    pub name: String,
    pub system_prompt: String,
    pub shortcut: String,
    pub auto_apply: bool,
    pub app_category: String,
    pub is_active: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

fn row_to_transform(row: &Row) -> rusqlite::Result<Transform> {
    Ok(Transform {
        id: row.get("id")?,
        name: row.get("name")?,
        system_prompt: row.get("system_prompt")?,
        shortcut: row.get("shortcut")?,
        auto_apply: row.get::<_, i64>("auto_apply")? != 0,
        app_category: row.get("app_category")?,
        is_active: row.get::<_, i64>("is_active")? != 0,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

/// Insert a new transform (when `id` is `None`) or update an existing one.
/// Returns the row id.
#[allow(clippy::too_many_arguments)]
pub fn upsert(
    conn: &Connection,
    id: Option<i64>,
    name: &str,
    system_prompt: &str,
    shortcut: &str,
    auto_apply: bool,
    app_category: &str,
    is_active: bool,
    now: i64,
) -> rusqlite::Result<i64> {
    match id {
        Some(id) => {
            conn.execute(
                "UPDATE transforms SET
                     name = ?2, system_prompt = ?3, shortcut = ?4,
                     auto_apply = ?5, app_category = ?6, is_active = ?7, updated_at = ?8
                 WHERE id = ?1",
                params![
                    id,
                    name,
                    system_prompt,
                    shortcut,
                    auto_apply as i64,
                    app_category,
                    is_active as i64,
                    now
                ],
            )?;
            Ok(id)
        }
        None => {
            conn.execute(
                "INSERT INTO transforms
                    (name, system_prompt, shortcut, auto_apply, app_category, is_active, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
                params![
                    name,
                    system_prompt,
                    shortcut,
                    auto_apply as i64,
                    app_category,
                    is_active as i64,
                    now
                ],
            )?;
            Ok(conn.last_insert_rowid())
        }
    }
}

/// All transforms, newest first.
pub fn list(conn: &Connection) -> rusqlite::Result<Vec<Transform>> {
    let mut stmt = conn.prepare("SELECT * FROM transforms ORDER BY updated_at DESC")?;
    let rows = stmt
        .query_map([], row_to_transform)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Fetch a single transform by id.
pub fn get(conn: &Connection, id: i64) -> rusqlite::Result<Option<Transform>> {
    let mut stmt = conn.prepare("SELECT * FROM transforms WHERE id = ?1")?;
    let mut rows = stmt.query_map(params![id], row_to_transform)?;
    match rows.next() {
        Some(r) => Ok(Some(r?)),
        None => Ok(None),
    }
}

pub fn delete(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM transforms WHERE id = ?1", params![id])?;
    Ok(())
}

/// Active transforms that have a non-empty shortcut, as `(id, shortcut)` pairs.
/// Used at launch (and after edits) to register global accelerators.
pub fn active_shortcuts(conn: &Connection) -> rusqlite::Result<Vec<(i64, String)>> {
    let mut stmt = conn.prepare(
        "SELECT id, shortcut FROM transforms
          WHERE is_active <> 0 AND shortcut <> ''",
    )?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Active auto-apply transforms whose scope matches `category` (or that apply to
/// all apps, i.e. an empty `app_category`). Applied in the pipeline after polish.
pub fn auto_apply_for(conn: &Connection, category: &str) -> rusqlite::Result<Vec<Transform>> {
    let mut stmt = conn.prepare(
        "SELECT * FROM transforms
          WHERE is_active <> 0 AND auto_apply <> 0
            AND (app_category = '' OR app_category = ?1)
          ORDER BY updated_at",
    )?;
    let rows = stmt
        .query_map(params![category], row_to_transform)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}
