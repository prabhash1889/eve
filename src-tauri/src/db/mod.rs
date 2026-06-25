//! SQLite persistence (Phase 3). Opens `eve.db` in the app data dir, runs
//! migrations via a hand-rolled `PRAGMA user_version` gate, and exposes the
//! history/stats queries used by `commands.rs` and `pipeline.rs`.

use std::path::Path;
use std::sync::Arc;

use parking_lot::Mutex;
use rusqlite::Connection;

pub mod dictionary;
pub mod queries;

/// Shared, lockable connection. rusqlite's `Connection` is `Send` but `!Sync`,
/// so the `Mutex` makes it safe to share across the Tauri app state.
pub type Db = Arc<Mutex<Connection>>;

const MIGRATION_001: &str = include_str!("migrations/001_initial.sql");
const MIGRATION_002: &str = include_str!("migrations/002_dictionary.sql");

/// Open (or create) the database at `path` and apply any pending migrations.
pub fn open(path: &Path) -> anyhow::Result<Db> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    migrate(&conn)?;
    Ok(Arc::new(Mutex::new(conn)))
}

/// Apply migrations newer than the stored `user_version`. Each migration bumps
/// the version so re-running is a no-op.
fn migrate(conn: &Connection) -> anyhow::Result<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version < 1 {
        conn.execute_batch(MIGRATION_001)?;
        conn.pragma_update(None, "user_version", 1i64)?;
    }
    if version < 2 {
        conn.execute_batch(MIGRATION_002)?;
        conn.pragma_update(None, "user_version", 2i64)?;
    }
    Ok(())
}
