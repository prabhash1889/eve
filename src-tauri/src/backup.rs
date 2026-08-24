//! 4.6: full backup bundle. One JSON file carrying settings (API keys are NOT
//! part of it - they live only in the OS keychain, by design), the dictionary,
//! snippets, Flow Styles, transforms, and optionally every history transcript.
//!
//! Restore merges: dictionary/snippet/style rows are upserted by their natural
//! keys, transforms are inserted fresh (ids reassigned), history rows are
//! appended with their original timestamps, and settings are applied wholesale
//! (the caller re-registers shortcuts/triggers around it).

use std::fs;

use serde::{Deserialize, Serialize};

use crate::config::Settings;
use crate::db::{dictionary, flow_styles, queries, snippets, transforms, Db};

/// Bump when the bundle shape changes in a way restore cares about.
pub const BUNDLE_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct BackupBundle {
    pub version: u32,
    /// Unix epoch ms of the export.
    pub exported_at: i64,
    pub settings: Settings,
    pub dictionary: Vec<DictionaryEntry>,
    pub snippets: Vec<Snippet>,
    pub flow_styles: Vec<FlowStyle>,
    pub transforms: Vec<Transform>,
    /// Present when the export included history; `None` makes restore skip
    /// history entirely.
    pub history: Option<Vec<queries::Transcript>>,
}

// Re-aliases keep the signature lines readable; the structs themselves live in
// their modules (and now derive Deserialize for this use).
use crate::db::dictionary::DictionaryEntry;
use crate::db::flow_styles::FlowStyle;
use crate::db::snippets::Snippet;
use crate::db::transforms::Transform;

/// What a restore changed, surfaced back to the UI.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub dictionary: usize,
    pub snippets: usize,
    pub flow_styles: usize,
    pub transforms: usize,
    pub transcripts: usize,
}

/// Collect everything for a bundle from the live DB + settings snapshot.
pub fn build(db: &Db, settings: &Settings, include_history: bool) -> anyhow::Result<BackupBundle> {
    let conn = db.lock();
    let history = if include_history {
        Some(queries::list_all_transcripts(&conn)?)
    } else {
        None
    };
    Ok(BackupBundle {
        version: BUNDLE_VERSION,
        exported_at: chrono::Utc::now().timestamp_millis(),
        settings: settings.clone(),
        dictionary: dictionary::list(&conn, None)?,
        snippets: snippets::list(&conn, None)?,
        flow_styles: flow_styles::list(&conn)?,
        transforms: transforms::list(&conn)?,
        history,
    })
}

/// Write a bundle to `path` as pretty-printed JSON.
pub fn write_to(bundle: &BackupBundle, path: &std::path::Path) -> anyhow::Result<()> {
    let json = serde_json::to_string_pretty(bundle)?;
    fs::write(path, json)?;
    Ok(())
}

/// Read a bundle from `path`.
pub fn read_from(path: &std::path::Path) -> anyhow::Result<BackupBundle> {
    let json = fs::read_to_string(path)?;
    let mut bundle: BackupBundle = serde_json::from_str(&json)?;
    if bundle.version == 0 {
        bundle.version = 1;
    }
    Ok(bundle)
}

/// Merge a bundle into the live DB (everything except settings, which the
/// caller applies separately since shortcut re-registration needs the app
/// handle). Returns per-table counts of restored rows.
pub fn merge_data(db: &Db, bundle: &BackupBundle) -> anyhow::Result<ImportSummary> {
    let now = chrono::Utc::now().timestamp_millis();
    let conn = db.lock();
    let mut summary = ImportSummary::default();

    for e in &bundle.dictionary {
        let word = e.word.trim();
        if word.is_empty() {
            continue;
        }
        if dictionary::upsert(&conn, word, e.replacement.as_deref(), e.is_starred, &e.source, now)
            .is_ok()
        {
            summary.dictionary += 1;
        }
    }

    for s in &bundle.snippets {
        if s.trigger_phrase.trim().is_empty() || s.expansion.trim().is_empty() {
            continue;
        }
        if snippets::upsert(&conn, &s.trigger_phrase, &s.expansion, s.is_active, now).is_ok() {
            summary.snippets += 1;
        }
    }

    for f in &bundle.flow_styles {
        if f.app_category.trim().is_empty() {
            continue;
        }
        if flow_styles::upsert(
            &conn,
            &f.name,
            f.app_category.trim(),
            f.app_process.trim(),
            &f.tone,
            &f.system_prompt,
            &f.writing_sample,
            f.is_active,
            &f.shortcut,
            now,
        )
        .is_ok()
        {
            summary.flow_styles += 1;
        }
    }

    for t in &bundle.transforms {
        if t.name.trim().is_empty() {
            continue;
        }
        // Inserted fresh: ids are reassigned, avoiding collisions with rows
        // already in the target DB.
        if transforms::upsert(
            &conn,
            None,
            &t.name,
            &t.system_prompt,
            &t.shortcut,
            t.auto_apply,
            &t.app_category,
            t.is_active,
            now,
        )
        .is_ok()
        {
            summary.transforms += 1;
        }
    }

    if let Some(history) = &bundle.history {
        for h in history {
            let row = queries::NewTranscript {
                created_at: h.created_at,
                raw_text: h.raw_text.clone(),
                polished_text: h.polished_text.clone(),
                cleanup_level: h.cleanup_level.clone(),
                language: h.language.clone(),
                app_process: h.app_process.clone(),
                app_title: h.app_title.clone(),
                app_category: h.app_category.clone(),
                word_count: h.word_count,
                duration_ms: h.duration_ms,
                was_polished: h.was_polished,
                source_file: h.source_file.clone(),
            };
            if queries::insert_transcript(&conn, &row).is_ok() {
                summary.transcripts += 1;
            }
        }
    }

    Ok(summary)
}
