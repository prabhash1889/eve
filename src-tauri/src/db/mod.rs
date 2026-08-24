//! SQLite persistence (Phase 3). Opens `eve.db` in the app data dir, runs
//! migrations via a hand-rolled `PRAGMA user_version` gate, and exposes the
//! history/stats queries used by `commands.rs` and `pipeline.rs`.

use std::path::Path;
use std::sync::Arc;

use parking_lot::Mutex;
use rusqlite::Connection;

pub mod dictionary;
pub mod flow_styles;
pub mod queries;
pub mod scratchpad;
pub mod snippets;
pub mod transforms;

/// Shared, lockable connection. rusqlite's `Connection` is `Send` but `!Sync`,
/// so the `Mutex` makes it safe to share across the Tauri app state.
pub type Db = Arc<Mutex<Connection>>;

const MIGRATION_001: &str = include_str!("migrations/001_initial.sql");
const MIGRATION_002: &str = include_str!("migrations/002_dictionary.sql");
const MIGRATION_003: &str = include_str!("migrations/003_snippets.sql");
const MIGRATION_004: &str = include_str!("migrations/004_flow_styles.sql");
const MIGRATION_005: &str = include_str!("migrations/005_transforms.sql");
const MIGRATION_006: &str = include_str!("migrations/006_scratchpad.sql");
const MIGRATION_007: &str = include_str!("migrations/007_file_source.sql");
const MIGRATION_008: &str = include_str!("migrations/008_flow_style_process.sql");
const MIGRATION_009: &str = include_str!("migrations/009_flow_style_shortcut.sql");

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
    if version < 3 {
        conn.execute_batch(MIGRATION_003)?;
        conn.pragma_update(None, "user_version", 3i64)?;
    }
    if version < 4 {
        conn.execute_batch(MIGRATION_004)?;
        conn.pragma_update(None, "user_version", 4i64)?;
    }
    if version < 5 {
        conn.execute_batch(MIGRATION_005)?;
        conn.pragma_update(None, "user_version", 5i64)?;
    }
    if version < 6 {
        conn.execute_batch(MIGRATION_006)?;
        conn.pragma_update(None, "user_version", 6i64)?;
    }
    if version < 7 {
        conn.execute_batch(MIGRATION_007)?;
        conn.pragma_update(None, "user_version", 7i64)?;
    }
    if version < 8 {
        conn.execute_batch(MIGRATION_008)?;
        conn.pragma_update(None, "user_version", 8i64)?;
    }
    if version < 9 {
        conn.execute_batch(MIGRATION_009)?;
        conn.pragma_update(None, "user_version", 9i64)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::dictionary;
    use crate::db::flow_styles;
    use crate::db::queries::{self, NewTranscript};
    use crate::db::scratchpad;
    use crate::db::snippets;
    use crate::db::transforms;

    fn conn() -> rusqlite::Connection {
        let c = rusqlite::Connection::open_in_memory().unwrap();
        c.pragma_update(None, "foreign_keys", "ON").unwrap();
        migrate(&c).unwrap();
        c
    }

    #[test]
    fn migrations_apply_cleanly_and_are_idempotent() {
        let c = conn();
        let v: i64 = c
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, 9, "all migrations should have run");
        // Re-running migrate on a fully-migrated DB is a no-op.
        migrate(&c).unwrap();
        let v2: i64 = c
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v2, 9);
    }

    fn transcript(created_at: i64, text: &str) -> NewTranscript {
        NewTranscript {
            created_at,
            raw_text: text.to_string(),
            polished_text: text.to_string(),
            cleanup_level: "medium".into(),
            language: "en".into(),
            app_process: "code.exe".into(),
            app_title: "editor".into(),
            app_category: "code".into(),
            word_count: 2,
            duration_ms: 1500,
            was_polished: true,
            source_file: None,
        }
    }

    #[test]
    fn transcript_insert_list_soft_delete_recover_round_trip() {
        let c = conn();
        let id = queries::insert_transcript(&c, &transcript(1000, "hello world")).unwrap();
        let page = queries::get_history(&c, 1, 10, None).unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].id, id);
        assert_eq!(page.items[0].raw_text, "hello world");
        assert!(page.items[0].deleted_at.is_none());

        queries::soft_delete(&c, id, 2000).unwrap();
        let page = queries::get_history(&c, 1, 10, None).unwrap();
        assert_eq!(page.total, 0, "soft-deleted rows leave the history page");

        queries::recover(&c, id).unwrap();
        let page = queries::get_history(&c, 1, 10, None).unwrap();
        assert_eq!(page.total, 1);
    }

    #[test]
    fn history_search_matches_raw_and_polished_text() {
        let c = conn();
        queries::insert_transcript(&c, &transcript(1000, "the quick brown fox")).unwrap();
        queries::insert_transcript(&c, &transcript(2000, "lorem ipsum dolor")).unwrap();

        let hits = queries::get_history(&c, 1, 10, Some("fox".into())).unwrap();
        assert_eq!(hits.total, 1);
        assert_eq!(hits.items[0].raw_text, "the quick brown fox");

        let none = queries::get_history(&c, 1, 10, Some("zebra".into())).unwrap();
        assert_eq!(none.total, 0);
    }

    #[test]
    fn clear_history_removes_everything() {
        let c = conn();
        queries::insert_transcript(&c, &transcript(1000, "a")).unwrap();
        queries::insert_transcript(&c, &transcript(2000, "b")).unwrap();
        queries::clear_history(&c, 3000).unwrap();
        assert_eq!(queries::get_history(&c, 1, 10, None).unwrap().total, 0);
    }

    #[test]
    fn daily_rollup_feeds_stats() {
        let c = conn();
        queries::insert_transcript(&c, &transcript(1000, "hello world")).unwrap();
        queries::record_daily(&c, 1000, 2, 1500, 3, "code").unwrap();

        let stats = queries::get_stats(&c, 0).unwrap();
        assert_eq!(stats.total_sessions, 1);
        assert_eq!(stats.total_words, 2);
        assert_eq!(stats.corrections, 3);
        assert_eq!(stats.app_usage.len(), 1);
        assert_eq!(stats.app_usage[0].category, "code");
        assert!(!stats.daily.is_empty());
    }

    #[test]
    fn dictionary_upsert_hints_and_corrections() {
        let c = conn();
        dictionary::upsert(&c, "kubernetes", None, true, "user", 100).unwrap();
        dictionary::upsert(&c, "rusqlte", Some("rusqlite"), false, "user", 200).unwrap();

        // Case-insensitive conflict updates in place (single row).
        let id = dictionary::upsert(&c, "Kubernetes", None, true, "import", 300).unwrap();
        assert_eq!(dictionary::list(&c, None).unwrap().len(), 2);

        // Starred-first hint order.
        let hints = dictionary::hints(&c, 10).unwrap();
        assert_eq!(hints[0], "kubernetes");

        let corrections = dictionary::corrections(&c).unwrap();
        assert_eq!(
            corrections,
            vec![("rusqlte".to_string(), "rusqlite".to_string())]
        );

        dictionary::delete(&c, id).unwrap();
        assert_eq!(dictionary::list(&c, None).unwrap().len(), 1);
    }

    #[test]
    fn snippets_active_expansions_skip_inactive_rows() {
        let c = conn();
        snippets::upsert(&c, "my email", "me@example.com", true, 100).unwrap();
        snippets::upsert(&c, "sig", "- Full Name", false, 110).unwrap();
        let expansions = snippets::active_expansions(&c).unwrap();
        assert_eq!(
            expansions,
            vec![("my email".to_string(), "me@example.com".to_string())]
        );
    }

    #[test]
    fn flow_styles_exact_process_wins_over_category_default() {
        let c = conn();
        flow_styles::upsert(&c, "Email", "email", "", "formal", "", "", true, "", 100)
            .unwrap();
        flow_styles::upsert(&c, "Slack pithy", "workmsg", "slack.exe", "casual", "", "", true, "", 110)
            .unwrap();

        // Exact-process lookup resolves only its own row.
        let slack =
            flow_styles::active_for_process(&c, "workmsg", "slack.exe").unwrap().unwrap();
        assert_eq!(slack.name, "Slack pithy");
        assert!(flow_styles::active_for_process(&c, "workmsg", "teams.exe")
            .unwrap()
            .is_none());

        // Category default ignores exact-app rows.
        let email = flow_styles::active_for(&c, "email").unwrap().unwrap();
        assert_eq!(email.name, "Email");
        assert!(flow_styles::active_for(&c, "workmsg").unwrap().is_none());
    }

    #[test]
    fn transforms_get_and_active_shortcuts() {
        let c = conn();
        let id = transforms::upsert(
            &c, None, "Fix grammar", "Fix all grammar mistakes.", "CmdOrCtrl+Shift+T", false, "",
            true, 100,
        )
        .unwrap();
        let t = transforms::get(&c, id).unwrap().unwrap();
        assert_eq!(t.name, "Fix grammar");
        assert_eq!(t.system_prompt, "Fix all grammar mistakes.");
        assert!(t.is_active);

        let shorts = transforms::active_shortcuts(&c).unwrap();
        assert_eq!(shorts, vec![(id, "CmdOrCtrl+Shift+T".to_string())]);
    }

    #[test]
    fn scratchpad_tab_crud_round_trip() {
        let c = conn();
        let tab = scratchpad::create(&c, "Notes", 100).unwrap();
        scratchpad::save(&c, tab.id, "Renamed", "some content", 200).unwrap();
        let tabs = scratchpad::list(&c).unwrap();
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs[0].title, "Renamed");
        assert_eq!(tabs[0].content, "some content");
        scratchpad::delete(&c, tab.id).unwrap();
        assert!(scratchpad::list(&c).unwrap().is_empty());
    }
}
