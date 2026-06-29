//! Phase 1: lightweight stage timing for the dictation pipeline.
//!
//! `Timings` records the wall-clock cost of each pipeline stage (drain,
//! resample/encode, transcribe, polish, inject) and the total release-to-done
//! latency. `finish` logs a one-line breakdown and appends a CSV row to
//! `app_data_dir/metrics/latency.csv`, so sessions can be compared across runs
//! and before/after an optimization. Diagnostics only: a failure to persist must
//! never affect the dictation flow.

use std::time::Instant;

use tauri::{AppHandle, Manager};

/// A running collection of stage timings for one dictation session.
pub struct Timings {
    /// When the pipeline started (key release) — the reference for total latency.
    start: Instant,
    /// End of the previous stage, so `mark` measures only this stage's slice.
    last: Instant,
    stages: Vec<(&'static str, u128)>,
    /// Transcription backend in effect ("groq" / "local"), for the persisted row.
    backend: String,
    /// Local model id when the local backend ran, else empty.
    model: String,
}

impl Timings {
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            start: now,
            last: now,
            stages: Vec::new(),
            backend: String::new(),
            model: String::new(),
        }
    }

    /// Record the time elapsed since the previous mark under `stage`.
    pub fn mark(&mut self, stage: &'static str) {
        let now = Instant::now();
        self.stages
            .push((stage, now.duration_since(self.last).as_millis()));
        self.last = now;
    }

    /// Note the backend/model that handled this session (for the persisted row).
    pub fn set_context(&mut self, backend: &str, model: &str) {
        self.backend = backend.to_string();
        self.model = model.to_string();
    }

    /// Total elapsed since `new` (release-to-now), in milliseconds.
    pub fn total_ms(&self) -> u128 {
        self.last.duration_since(self.start).as_millis()
    }

    fn breakdown(&self) -> String {
        let mut parts: Vec<String> = self
            .stages
            .iter()
            .map(|(name, ms)| format!("{name}={ms}ms"))
            .collect();
        parts.push(format!("total={}ms", self.total_ms()));
        parts.join(" ")
    }

    /// Log the breakdown and append a CSV row. Best-effort; called once per
    /// session from `pipeline::process` after injection.
    pub fn finish(&self, app: &AppHandle) {
        eprintln!(
            "[timing] backend={} model={} {}",
            if self.backend.is_empty() { "?" } else { &self.backend },
            if self.model.is_empty() { "-" } else { &self.model },
            self.breakdown()
        );
        self.persist(app);
    }

    /// Append a single CSV line: `timestamp,backend,model,<stage>=ms…,total`.
    fn persist(&self, app: &AppHandle) {
        use std::io::Write;
        let Ok(data_dir) = app.path().app_data_dir() else {
            return;
        };
        let dir = data_dir.join("metrics");
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let path = dir.join("latency.csv");
        let ts = chrono::Utc::now().timestamp_millis();
        let stages = self
            .stages
            .iter()
            .map(|(name, ms)| format!("{name}={ms}"))
            .collect::<Vec<_>>()
            .join(";");
        let line = format!(
            "{ts},{},{},{},{}\n",
            self.backend, self.model, stages, self.total_ms()
        );
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            let _ = f.write_all(line.as_bytes());
        }
    }
}
