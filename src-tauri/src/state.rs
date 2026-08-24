//! Shared application state, managed by Tauri and accessed from the hotkey
//! handler, audio thread, and commands. All mutable fields are behind Arc so
//! the audio capture thread can own clones.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, AtomicU64};
use std::sync::Arc;

use parking_lot::Mutex;
use tauri_plugin_global_shortcut::Shortcut;

use crate::config::Settings;
use crate::context::AppContext;
use crate::db::flow_styles::FlowStyle;
use crate::db::transforms::Transform;
use crate::db::{dictionary, flow_styles, snippets, transforms, Db};
use crate::polish::{Polisher, RoutingPolisher};
use crate::transcription::{RoutingTranscriber, Transcriber, TranscriptionBenchmark};

/// Hint-prompt cap used by the pipeline's former direct read
/// (`dictionary::hints(conn, 100)`); kept here so the cached path stays
/// byte-identical to it.
const PIPELINE_HINTS_LIMIT: i64 = 100;

/// 1.P5: in-process cache of the dictionary / snippet / Flow Style / transform
/// lookups the dictation pipeline reads on every session. Populated lazily on
/// first read (the three global lookups are warmed at startup) and cleared by
/// `invalidate` from every command that writes one of those tables, so a
/// session never touches the DB connection lock for these reads and can't
/// contend with history writes.
///
/// Lock discipline mirrors the rest of the app: the inner guard is never held
/// while acquiring the `Db` lock (each method drops it before querying), so a
/// concurrent writer holding the DB lock can't deadlock against a reader.
#[derive(Default)]
struct HotPathCacheInner {
    hints: Option<Vec<String>>,
    corrections: Option<Vec<(String, String)>>,
    expansions: Option<Vec<(String, String)>>,
    /// Active style per focused-app category (at most one style per category).
    styles: HashMap<String, Option<FlowStyle>>,
    /// Auto-apply transforms per focused-app category.
    auto_transforms: HashMap<String, Vec<Transform>>,
}

/// Cheap-clonable handle sharing the cache internals (see `AppState::hot_cache`).
#[derive(Clone, Default)]
pub struct HotPathCache {
    inner: Arc<Mutex<HotPathCacheInner>>,
}

impl HotPathCache {
    /// Create the cache and warm the category-independent lookups so the first
    /// dictation after launch doesn't pay the miss. Category-keyed entries fill
    /// lazily on their first session.
    pub fn new(db: &Db) -> Self {
        let cache = Self::default();
        let _ = cache.hints(db);
        let _ = cache.corrections(db);
        let _ = cache.snippet_expansions(db);
        cache
    }

    /// Whisper vocabulary hints: starred terms first, then most-recently
    /// updated, capped (mirrors the pipeline's former `hints(conn, 100)`).
    pub fn hints(&self, db: &Db) -> Vec<String> {
        if let Some(cached) = self.inner.lock().hints.clone() {
            return cached;
        }
        let fresh = {
            let conn = db.lock();
            dictionary::hints(&conn, PIPELINE_HINTS_LIMIT).unwrap_or_default()
        };
        self.inner.lock().hints.get_or_insert(fresh).clone()
    }

    /// Misspelling→correction pairs, longest word first.
    pub fn corrections(&self, db: &Db) -> Vec<(String, String)> {
        if let Some(cached) = self.inner.lock().corrections.clone() {
            return cached;
        }
        let fresh = {
            let conn = db.lock();
            dictionary::corrections(&conn).unwrap_or_default()
        };
        self.inner.lock().corrections.get_or_insert(fresh).clone()
    }

    /// Active trigger→expansion pairs, longest trigger first.
    pub fn snippet_expansions(&self, db: &Db) -> Vec<(String, String)> {
        if let Some(cached) = self.inner.lock().expansions.clone() {
            return cached;
        }
        let fresh = {
            let conn = db.lock();
            snippets::active_expansions(&conn).unwrap_or_default()
        };
        self.inner.lock().expansions.get_or_insert(fresh).clone()
    }

    /// The active Flow Style for the focused app: the exact-process profile
    /// (4.3) when one exists, else the whole-category default - or `None`
    /// (same fallback shape as the pipeline's former direct read: a query
    /// error yielded no style). Cache keys are `"{category}"` and
    /// `"{category}|{process}"` respectively.
    pub fn active_style(&self, db: &Db, category: &str, process: &str) -> Option<FlowStyle> {
        let proc = process.trim().to_ascii_lowercase();
        if !proc.is_empty() {
            let key = format!("{category}|{proc}");
            if let Some(cached) = self.inner.lock().styles.get(&key) {
                return cached.clone();
            }
            let fresh = {
                let conn = db.lock();
                flow_styles::active_for_process(&conn, category, &proc)
                    .ok()
                    .flatten()
            };
            self.inner
                .lock()
                .styles
                .insert(key, fresh.clone());
            if fresh.is_some() {
                return fresh;
            }
            // No exact-app match: fall through to the category default below,
            // but don't cache it under this key (the exact-app row may appear).
        }
        if let Some(cached) = self.inner.lock().styles.get(category) {
            return cached.clone();
        }
        let fresh = {
            let conn = db.lock();
            flow_styles::active_for(&conn, category).ok().flatten()
        };
        self.inner
            .lock()
            .styles
            .insert(category.to_string(), fresh.clone());
        fresh
    }

    /// Auto-apply transforms scoped to a category (or to all apps).
    pub fn auto_transforms(&self, db: &Db, category: &str) -> Vec<Transform> {
        if let Some(cached) = self.inner.lock().auto_transforms.get(category) {
            return cached.clone();
        }
        let fresh = {
            let conn = db.lock();
            transforms::auto_apply_for(&conn, category).unwrap_or_default()
        };
        self.inner
            .lock()
            .auto_transforms
            .insert(category.to_string(), fresh.clone());
        fresh
    }

    /// Drop every cached entry. Must be called after any successful write to
    /// the dictionary, snippets, flow_styles, or transforms tables so the next
    /// session re-reads current data.
    pub fn invalidate(&self) {
        *self.inner.lock() = HotPathCacheInner::default();
    }
}

pub struct AppState {
    pub is_recording: Arc<AtomicBool>,
    /// Set true from key-up until the pipeline (`pipeline::process`,
    /// `command_mode::process_command`, or `run_transform_shortcut`) finishes
    /// (success, error, or cancel). The dictation/command/transform handlers
    /// refuse to start or re-fire while it is set, so a rapid press or a key
    /// auto-repeat while the previous pipeline is still running can't spawn a
    /// second, overlapping one.
    pub is_processing: Arc<AtomicBool>,
    /// Parity A1: when the recording started (stamped on the trigger press).
    /// Hybrid activation compares against this to tell a quick tap (arms a
    /// toggle) from a genuine push-to-talk hold.
    pub press_at: Arc<Mutex<Option<std::time::Instant>>>,
    /// Parity A1: set once the trigger has been released since recording
    /// started. Toggle/hybrid modes only treat a `Pressed` event as "stop" after
    /// this is set - the OS auto-repeats `Pressed` while a key is held, and
    /// those repeats must not stop the recording.
    pub saw_release: Arc<AtomicBool>,
    /// Parity A1: true while the trigger is physically down (set on the first
    /// `Pressed`, cleared on `Released`). The OS auto-repeats `Pressed` while a
    /// key is held; `saw_release` only filters repeats while recording, but
    /// after a toggle/hybrid stop-press the app is idle again and a repeat
    /// arriving once the pipeline finishes would start an unintended new
    /// recording - this latch drops every `Pressed` that isn't a fresh press.
    pub trigger_down: Arc<AtomicBool>,
    /// Phase 0: physical-down latch for the Command Mode shortcut, mirroring
    /// `trigger_down`. The OS auto-repeats `Pressed` for the whole hold; without
    /// this the repeat that arrives once `is_processing` clears mid-hold would
    /// start a capture on the tail of the instruction and inject spuriously.
    pub command_down: Arc<AtomicBool>,
    /// Phase 0: physical-down latch for transform accelerators, same shape as
    /// `command_down`. Prevents the auto-repeat after a transform finishes from
    /// re-firing the transform (re-capturing the selection + re-injecting).
    pub transform_down: Arc<AtomicBool>,
    pub audio_buffer: Arc<Mutex<Vec<f32>>>,
    pub sample_rate: Arc<AtomicU32>,
    pub current_amplitude: Arc<Mutex<f32>>,
    /// Owns the single microphone capture thread. Recording is driven by
    /// `capture.start()`/`capture.stop()`; the thread serializes stream
    /// create/destroy so rapid taps can't open two streams on one device.
    pub capture: crate::audio::CaptureHandle,
    /// Foreground window (HWND as isize) captured when recording starts, so we
    /// can restore focus to it before pasting.
    pub foreground_hwnd: Arc<AtomicIsize>,
    /// Phase 6: focused-app context (process/title/category) resolved at record
    /// start, used to pick a Flow Style and to attribute history rows.
    pub current_context: Arc<Mutex<Option<AppContext>>>,
    pub main_shortcut: Arc<Mutex<Shortcut>>,
    pub escape_shortcut: Shortcut,
    /// Phase 2: global shortcut to copy the last transcript to the clipboard.
    pub copy_shortcut: Arc<Mutex<Shortcut>>,
    /// Phase 7: Command Mode push-to-talk shortcut, and a flag set while a
    /// Command Mode capture is in flight (so key-up routes to the command
    /// pipeline rather than the dictation one).
    pub command_shortcut: Arc<Mutex<Shortcut>>,
    pub is_command_mode: Arc<AtomicBool>,
    /// Phase 9: global shortcut that opens the Scratchpad window, plus a flag set
    /// at record start when the Scratchpad window had focus — so the pipeline
    /// routes the dictation into its editor instead of OS-pasting.
    pub scratchpad_shortcut: Arc<Mutex<Shortcut>>,
    pub to_scratchpad: Arc<AtomicBool>,
    /// 4.2: global shortcut that deletes the last injection.
    pub undo_shortcut: Arc<Mutex<Shortcut>>,
    /// Phase 7: registered transform accelerators paired with their transform
    /// id. A Vec (not a map) so we don't depend on `Shortcut: Hash`; the handler
    /// linear-scans it like the other reserved shortcuts. Rebuilt at launch and
    /// whenever transforms are edited.
    pub transform_shortcuts: Arc<Mutex<Vec<(Shortcut, i64)>>>,
    pub last_transcript: Arc<Mutex<Option<String>>>,
    pub last_transcription_benchmark: Arc<Mutex<Option<TranscriptionBenchmark>>>,
    pub settings: Arc<Mutex<Settings>>,
    pub settings_path: PathBuf,
    /// Routing transcriber/polisher: each holds both the Groq and local backends
    /// plus a clone of `settings`, and picks per-call so the backend can be
    /// switched in the UI without a restart (falls back to Groq on local error).
    pub transcriber: Arc<dyn Transcriber>,
    pub polisher: Arc<dyn Polisher>,
    /// Phase 3: history/stats store (SQLite), shared with the audio thread-free
    /// pipeline and the history commands.
    pub db: Db,
    /// 1.P5: cache of the hot-path DB reads the dictation pipeline performs per
    /// session (dictionary hints/corrections, snippet expansions, Flow Styles,
    /// auto-apply transforms). Invalidated by every command that writes those
    /// tables; see `HotPathCache`.
    pub hot_cache: HotPathCache,
    /// Local-models: in-flight downloads keyed by model id; the bool is a
    /// cancel-requested flag the download task observes.
    pub model_downloads: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
    /// Phase C (file transcription): pending files awaiting transcription,
    /// drained serially by a single worker task (`file_transcribe::run_worker`).
    pub file_queue: Arc<Mutex<VecDeque<crate::file_transcribe::QueuedFile>>>,
    /// Monotonic id source for queue items.
    pub queue_next_id: Arc<AtomicU64>,
    /// True while the queue worker is draining. Guards against spawning a second
    /// worker; the worker clears it (under the `file_queue` lock) when it empties.
    pub queue_worker_running: Arc<AtomicBool>,
    /// Ids the user cancelled: a pending item is dropped, a processing item is
    /// abandoned at the next stage boundary (checked in the worker).
    pub queue_cancelled: Arc<Mutex<HashSet<u64>>>,
}

impl AppState {
    pub fn new(
        settings: Settings,
        settings_path: PathBuf,
        db: Db,
        models_dir: PathBuf,
        bundled_models_dir: Option<PathBuf>,
    ) -> Self {
        let main = parse_shortcut(&settings.shortcut);
        let copy = parse_shortcut(&settings.copy_shortcut);
        let command = parse_shortcut(&settings.command_shortcut);
        let scratchpad = parse_shortcut(&settings.scratchpad_shortcut);
        let undo = parse_shortcut(&settings.undo_shortcut);
        // "Escape" always parses, but fall back gracefully instead of panicking
        // at startup if a future toolkit change ever rejects it.
        let escape = parse_shortcut("Escape");
        // Build the shared settings Arc first so the routers can read live
        // backend selections from the same source the commands write to.
        let settings = Arc::new(Mutex::new(settings));
        // Warm the hot-path read cache off the connection before the app (and
        // its history writes) start contending for it.
        let hot_cache = HotPathCache::new(&db);
        Self {
            is_recording: Arc::new(AtomicBool::new(false)),
            is_processing: Arc::new(AtomicBool::new(false)),
            press_at: Arc::new(Mutex::new(None)),
            saw_release: Arc::new(AtomicBool::new(false)),
            trigger_down: Arc::new(AtomicBool::new(false)),
            command_down: Arc::new(AtomicBool::new(false)),
            transform_down: Arc::new(AtomicBool::new(false)),
            audio_buffer: Arc::new(Mutex::new(Vec::new())),
            sample_rate: Arc::new(AtomicU32::new(16_000)),
            current_amplitude: Arc::new(Mutex::new(0.0)),
            capture: crate::audio::CaptureHandle::new(),
            foreground_hwnd: Arc::new(AtomicIsize::new(0)),
            current_context: Arc::new(Mutex::new(None)),
            main_shortcut: Arc::new(Mutex::new(main)),
            escape_shortcut: escape,
            copy_shortcut: Arc::new(Mutex::new(copy)),
            command_shortcut: Arc::new(Mutex::new(command)),
            is_command_mode: Arc::new(AtomicBool::new(false)),
            scratchpad_shortcut: Arc::new(Mutex::new(scratchpad)),
            to_scratchpad: Arc::new(AtomicBool::new(false)),
            undo_shortcut: Arc::new(Mutex::new(undo)),
            transform_shortcuts: Arc::new(Mutex::new(Vec::new())),
            last_transcript: Arc::new(Mutex::new(None)),
            last_transcription_benchmark: Arc::new(Mutex::new(None)),
            transcriber: Arc::new(RoutingTranscriber::new(
                models_dir.clone(),
                bundled_models_dir,
                settings.clone(),
            )),
            polisher: Arc::new(RoutingPolisher::new(models_dir, settings.clone())),
            settings,
            settings_path,
            db,
            hot_cache,
            model_downloads: Arc::new(Mutex::new(HashMap::new())),
            file_queue: Arc::new(Mutex::new(VecDeque::new())),
            queue_next_id: Arc::new(AtomicU64::new(1)),
            queue_worker_running: Arc::new(AtomicBool::new(false)),
            queue_cancelled: Arc::new(Mutex::new(HashSet::new())),
        }
    }
}

/// Parse an accelerator string (e.g. "F8", "CmdOrCtrl+Shift+Space") into a
/// `Shortcut`, falling back to F8 if invalid.
pub fn parse_shortcut(s: &str) -> Shortcut {
    Shortcut::from_str(s).unwrap_or_else(|_| Shortcut::from_str("F8").unwrap())
}
