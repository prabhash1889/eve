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
use crate::db::Db;
use crate::polish::{Polisher, RoutingPolisher};
use crate::transcription::{RoutingTranscriber, Transcriber, TranscriptionBenchmark};

pub struct AppState {
    pub is_recording: Arc<AtomicBool>,
    /// Set true from key-up until `pipeline::process` finishes (success, error,
    /// or cancel). `hotkey::on_press` refuses to start a new capture while it is
    /// set, so a rapid press while the previous dictation is still transcribing
    /// can't spawn a second, overlapping pipeline.
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
    pub fn new(settings: Settings, settings_path: PathBuf, db: Db, models_dir: PathBuf) -> Self {
        let main = parse_shortcut(&settings.shortcut);
        let copy = parse_shortcut(&settings.copy_shortcut);
        let command = parse_shortcut(&settings.command_shortcut);
        let scratchpad = parse_shortcut(&settings.scratchpad_shortcut);
        // "Escape" always parses, but fall back gracefully instead of panicking
        // at startup if a future toolkit change ever rejects it.
        let escape = parse_shortcut("Escape");
        // Build the shared settings Arc first so the routers can read live
        // backend selections from the same source the commands write to.
        let settings = Arc::new(Mutex::new(settings));
        Self {
            is_recording: Arc::new(AtomicBool::new(false)),
            is_processing: Arc::new(AtomicBool::new(false)),
            press_at: Arc::new(Mutex::new(None)),
            saw_release: Arc::new(AtomicBool::new(false)),
            trigger_down: Arc::new(AtomicBool::new(false)),
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
            transform_shortcuts: Arc::new(Mutex::new(Vec::new())),
            last_transcript: Arc::new(Mutex::new(None)),
            last_transcription_benchmark: Arc::new(Mutex::new(None)),
            transcriber: Arc::new(RoutingTranscriber::new(
                models_dir.clone(),
                settings.clone(),
            )),
            polisher: Arc::new(RoutingPolisher::new(models_dir, settings.clone())),
            settings,
            settings_path,
            db,
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
