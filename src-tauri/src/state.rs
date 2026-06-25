//! Shared application state, managed by Tauri and accessed from the hotkey
//! handler, audio thread, and commands. All mutable fields are behind Arc so
//! the audio capture thread can own clones.

use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32};
use std::sync::Arc;

use parking_lot::Mutex;
use tauri_plugin_global_shortcut::Shortcut;

use crate::config::Settings;
use crate::db::Db;
use crate::polish::{GroqPolisher, Polisher};
use crate::transcription::{GroqTranscriber, Transcriber};

pub struct AppState {
    pub is_recording: Arc<AtomicBool>,
    pub audio_buffer: Arc<Mutex<Vec<f32>>>,
    pub sample_rate: Arc<AtomicU32>,
    pub current_amplitude: Arc<Mutex<f32>>,
    /// Foreground window (HWND as isize) captured when recording starts, so we
    /// can restore focus to it before pasting.
    pub foreground_hwnd: Arc<AtomicIsize>,
    pub main_shortcut: Arc<Mutex<Shortcut>>,
    pub escape_shortcut: Shortcut,
    /// Phase 2: global shortcut to copy the last transcript to the clipboard.
    pub copy_shortcut: Arc<Mutex<Shortcut>>,
    pub last_transcript: Arc<Mutex<Option<String>>>,
    pub settings: Arc<Mutex<Settings>>,
    pub settings_path: PathBuf,
    pub transcriber: Arc<dyn Transcriber>,
    pub polisher: Arc<dyn Polisher>,
    /// Phase 3: history/stats store (SQLite), shared with the audio thread-free
    /// pipeline and the history commands.
    pub db: Db,
}

impl AppState {
    pub fn new(settings: Settings, settings_path: PathBuf, db: Db) -> Self {
        let main = parse_shortcut(&settings.shortcut);
        let copy = parse_shortcut(&settings.copy_shortcut);
        let escape = Shortcut::from_str("Escape").expect("valid escape shortcut");
        Self {
            is_recording: Arc::new(AtomicBool::new(false)),
            audio_buffer: Arc::new(Mutex::new(Vec::new())),
            sample_rate: Arc::new(AtomicU32::new(16_000)),
            current_amplitude: Arc::new(Mutex::new(0.0)),
            foreground_hwnd: Arc::new(AtomicIsize::new(0)),
            main_shortcut: Arc::new(Mutex::new(main)),
            escape_shortcut: escape,
            copy_shortcut: Arc::new(Mutex::new(copy)),
            last_transcript: Arc::new(Mutex::new(None)),
            settings: Arc::new(Mutex::new(settings)),
            settings_path,
            transcriber: Arc::new(GroqTranscriber::new()),
            // Always GroqPolisher: it no-ops for CleanupLevel::None, so the
            // level can change at runtime without rebuilding state.
            polisher: Arc::new(GroqPolisher::new()),
            db,
        }
    }
}

/// Parse an accelerator string (e.g. "F8", "CmdOrCtrl+Shift+Space") into a
/// `Shortcut`, falling back to F8 if invalid.
pub fn parse_shortcut(s: &str) -> Shortcut {
    Shortcut::from_str(s).unwrap_or_else(|_| Shortcut::from_str("F8").unwrap())
}
