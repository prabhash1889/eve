//! Shared application state, managed by Tauri and accessed from the hotkey
//! handler, audio thread, and commands. All mutable fields are behind Arc so
//! the audio capture thread can own clones.

use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32};
use std::sync::Arc;

use parking_lot::Mutex;
use tauri_plugin_global_shortcut::Shortcut;

use crate::config::Settings;
use crate::context::AppContext;
use crate::db::Db;
use crate::polish::{Polisher, RoutingPolisher};
use crate::transcription::{RoutingTranscriber, Transcriber};

pub struct AppState {
    pub is_recording: Arc<AtomicBool>,
    pub audio_buffer: Arc<Mutex<Vec<f32>>>,
    pub sample_rate: Arc<AtomicU32>,
    pub current_amplitude: Arc<Mutex<f32>>,
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
    /// Phase 7: registered transform accelerators paired with their transform
    /// id. A Vec (not a map) so we don't depend on `Shortcut: Hash`; the handler
    /// linear-scans it like the other reserved shortcuts. Rebuilt at launch and
    /// whenever transforms are edited.
    pub transform_shortcuts: Arc<Mutex<Vec<(Shortcut, i64)>>>,
    pub last_transcript: Arc<Mutex<Option<String>>>,
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
}

impl AppState {
    pub fn new(settings: Settings, settings_path: PathBuf, db: Db, models_dir: PathBuf) -> Self {
        let main = parse_shortcut(&settings.shortcut);
        let copy = parse_shortcut(&settings.copy_shortcut);
        let command = parse_shortcut(&settings.command_shortcut);
        let escape = Shortcut::from_str("Escape").expect("valid escape shortcut");
        // Build the shared settings Arc first so the routers can read live
        // backend selections from the same source the commands write to.
        let settings = Arc::new(Mutex::new(settings));
        Self {
            is_recording: Arc::new(AtomicBool::new(false)),
            audio_buffer: Arc::new(Mutex::new(Vec::new())),
            sample_rate: Arc::new(AtomicU32::new(16_000)),
            current_amplitude: Arc::new(Mutex::new(0.0)),
            foreground_hwnd: Arc::new(AtomicIsize::new(0)),
            current_context: Arc::new(Mutex::new(None)),
            main_shortcut: Arc::new(Mutex::new(main)),
            escape_shortcut: escape,
            copy_shortcut: Arc::new(Mutex::new(copy)),
            command_shortcut: Arc::new(Mutex::new(command)),
            is_command_mode: Arc::new(AtomicBool::new(false)),
            transform_shortcuts: Arc::new(Mutex::new(Vec::new())),
            last_transcript: Arc::new(Mutex::new(None)),
            transcriber: Arc::new(RoutingTranscriber::new(
                models_dir.clone(),
                settings.clone(),
            )),
            polisher: Arc::new(RoutingPolisher::new(models_dir, settings.clone())),
            settings,
            settings_path,
            db,
            model_downloads: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

/// Parse an accelerator string (e.g. "F8", "CmdOrCtrl+Shift+Space") into a
/// `Shortcut`, falling back to F8 if invalid.
pub fn parse_shortcut(s: &str) -> Shortcut {
    Shortcut::from_str(s).unwrap_or_else(|_| Shortcut::from_str("F8").unwrap())
}
