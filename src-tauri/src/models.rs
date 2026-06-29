//! Local-model download manager. Maintains a static catalog of on-device
//! Whisper (GGML) and polish-LLM (GGUF) models, downloads them from Hugging Face
//! with streamed progress events, verifies an optional SHA-256, and stores them
//! under `app_data_dir/models/`. The actual inference engines live in
//! `transcription.rs` / `polish.rs`; this module only manages the files.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use futures_util::StreamExt;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};

use crate::events;
use crate::state::AppState;

/// What an on-device model is used for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelKind {
    /// Speech→text (whisper.cpp, GGML `.bin`).
    Whisper,
    /// Text polish (llama.cpp, GGUF).
    Llm,
}

/// One downloadable model in the catalog.
#[derive(Debug, Clone)]
pub struct ModelInfo {
    /// Stable catalog id, also persisted in Settings and used as the filename stem.
    pub id: &'static str,
    pub kind: ModelKind,
    pub name: &'static str,
    /// Approximate download size, for display only.
    pub size_bytes: u64,
    pub url: &'static str,
    /// On-disk filename (extension matters: `.bin` for GGML, `.gguf` for GGUF).
    pub file_name: &'static str,
    /// Optional lowercase hex SHA-256. When `None`, verification is skipped.
    pub sha256: Option<&'static str>,
}

/// The static catalog. Whisper GGML weights come from `ggerganov/whisper.cpp`;
/// LLM GGUF weights from their respective `*-GGUF` repos. Sizes are approximate.
pub fn catalog() -> &'static [ModelInfo] {
    &[
        ModelInfo {
            id: "whisper-tiny.en",
            kind: ModelKind::Whisper,
            name: "Whisper Tiny (English)",
            size_bytes: 77_700_000,
            url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.en.bin",
            file_name: "whisper-tiny.en.bin",
            sha256: None,
        },
        ModelInfo {
            id: "whisper-base.en",
            kind: ModelKind::Whisper,
            name: "Whisper Base (English)",
            size_bytes: 147_900_000,
            url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin",
            file_name: "whisper-base.en.bin",
            sha256: None,
        },
        ModelInfo {
            id: "whisper-small.en",
            kind: ModelKind::Whisper,
            name: "Whisper Small (English)",
            size_bytes: 487_600_000,
            url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en.bin",
            file_name: "whisper-small.en.bin",
            sha256: None,
        },
        ModelInfo {
            id: "whisper-large-v3-turbo",
            kind: ModelKind::Whisper,
            name: "Whisper Large v3 Turbo (multilingual)",
            size_bytes: 1_624_000_000,
            url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin",
            file_name: "whisper-large-v3-turbo.bin",
            sha256: None,
        },
        ModelInfo {
            id: "qwen2.5-1.5b-instruct",
            kind: ModelKind::Llm,
            name: "Qwen2.5 1.5B Instruct (Q4_K_M)",
            size_bytes: 1_120_000_000,
            url: "https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct-GGUF/resolve/main/qwen2.5-1.5b-instruct-q4_k_m.gguf",
            file_name: "qwen2.5-1.5b-instruct-q4_k_m.gguf",
            sha256: None,
        },
        ModelInfo {
            id: "llama-3.2-1b-instruct",
            kind: ModelKind::Llm,
            name: "Llama 3.2 1B Instruct (Q4_K_M)",
            size_bytes: 808_000_000,
            url: "https://huggingface.co/bartowski/Llama-3.2-1B-Instruct-GGUF/resolve/main/Llama-3.2-1B-Instruct-Q4_K_M.gguf",
            file_name: "Llama-3.2-1B-Instruct-Q4_K_M.gguf",
            sha256: None,
        },
    ]
}

pub fn find(id: &str) -> Option<&'static ModelInfo> {
    catalog().iter().find(|m| m.id == id)
}

/// `app_data_dir/models`, created if missing.
pub fn models_dir(app: &AppHandle) -> std::io::Result<PathBuf> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| std::io::Error::other(e.to_string()))?
        .join("models");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// On-disk path for a model id (whether or not it's downloaded yet).
pub fn model_path(app: &AppHandle, id: &str) -> Option<PathBuf> {
    let info = find(id)?;
    Some(models_dir(app).ok()?.join(info.file_name))
}

/// A catalog entry plus its runtime status, returned to the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatus {
    pub id: String,
    pub kind: ModelKind,
    pub name: String,
    pub size_bytes: u64,
    pub installed: bool,
    pub downloading: bool,
    /// True when this model is the one selected in Settings for its kind.
    pub active: bool,
}

/// Catalog + per-model installed/active/downloading flags.
pub fn list(app: &AppHandle, state: &AppState) -> Vec<ModelStatus> {
    let (whisper_sel, llm_sel) = {
        let s = state.settings.lock();
        (s.local_whisper_model.clone(), s.local_llm_model.clone())
    };
    let in_flight = state.model_downloads.lock();
    catalog()
        .iter()
        .map(|m| {
            let installed = model_path(app, m.id).map(|p| p.exists()).unwrap_or(false);
            let active = match m.kind {
                ModelKind::Whisper => m.id == whisper_sel,
                ModelKind::Llm => m.id == llm_sel,
            };
            ModelStatus {
                id: m.id.to_string(),
                kind: m.kind,
                name: m.name.to_string(),
                size_bytes: m.size_bytes,
                installed,
                downloading: in_flight.contains_key(m.id),
                active,
            }
        })
        .collect()
}

/// Request cancellation of an in-flight download. The download task observes the
/// flag, deletes its partial file, and emits a `MODEL_ERROR`.
pub fn cancel(state: &AppState, id: &str) {
    if let Some(flag) = state.model_downloads.lock().get(id) {
        flag.store(true, Ordering::SeqCst);
    }
}

/// Delete a downloaded model file. No-op if it isn't present.
pub fn delete(app: &AppHandle, id: &str) -> Result<(), String> {
    if let Some(path) = model_path(app, id) {
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Kick off an async streamed download. Registers a cancel flag, streams to a
/// `.part` file, emits `MODEL_PROGRESS`, verifies the optional SHA-256, then
/// atomically renames into place and emits `MODEL_DONE` (or `MODEL_ERROR`).
/// Returns immediately; progress is reported via events.
pub fn start_download(app: AppHandle, id: String) -> Result<(), String> {
    let info = find(&id).ok_or_else(|| format!("Unknown model id: {id}"))?;
    let dest = model_path(&app, &id).ok_or("Could not resolve model path")?;

    {
        let state = app.state::<AppState>();
        let mut map = state.model_downloads.lock();
        if map.contains_key(info.id) {
            return Err("Already downloading".into());
        }
        map.insert(info.id.to_string(), Arc::new(AtomicBool::new(false)));
    }

    tauri::async_runtime::spawn(async move {
        let result = download_to_file(&app, info, &dest).await;

        // Always clear the in-flight entry.
        {
            let state = app.state::<AppState>();
            state.model_downloads.lock().remove(info.id);
        }

        match result {
            Ok(()) => {
                let _ = app.emit_to(
                    events::MAIN,
                    events::MODEL_DONE,
                    events::ModelStatusPayload {
                        id: id.clone(),
                        message: None,
                    },
                );
            }
            Err(e) => {
                // Clean up any partial file.
                let _ = std::fs::remove_file(dest.with_extension("part"));
                let _ = app.emit_to(
                    events::MAIN,
                    events::MODEL_ERROR,
                    events::ModelStatusPayload {
                        id: id.clone(),
                        message: Some(e),
                    },
                );
            }
        }
    });

    Ok(())
}

/// The streaming download itself. Errors (including cancellation) propagate to
/// the caller, which emits the terminal event.
async fn download_to_file(
    app: &AppHandle,
    info: &'static ModelInfo,
    dest: &std::path::Path,
) -> Result<(), String> {
    use std::io::Write;

    let cancel_flag = {
        let state = app.state::<AppState>();
        let flag = state.model_downloads.lock().get(info.id).cloned();
        flag
    }
    .ok_or("Download was cancelled")?;

    // A finite connect timeout so a dead host fails fast, plus a per-read
    // timeout so a stalled (but not closed) connection can't hang the download
    // forever. We deliberately omit an overall `.timeout` because a multi-GB
    // download legitimately runs for minutes.
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .read_timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| format!("HTTP client error: {e}"))?;
    let resp = client
        .get(info.url)
        .send()
        .await
        .map_err(|e| format!("Connection failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Download failed: HTTP {}", resp.status()));
    }
    let total = resp.content_length().unwrap_or(info.size_bytes);

    let part_path = dest.with_extension("part");
    let mut file =
        std::fs::File::create(&part_path).map_err(|e| format!("Cannot write file: {e}"))?;
    let mut hasher = Sha256::new();
    let mut downloaded: u64 = 0;
    let mut last_emit: u64 = 0;

    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        if cancel_flag.load(Ordering::SeqCst) {
            return Err("Download cancelled".into());
        }
        // A read that stalls past the client's `read_timeout` surfaces here as an
        // error rather than hanging forever.
        let chunk = chunk.map_err(|e| format!("Download interrupted: {e}"))?;
        file.write_all(&chunk).map_err(|e| e.to_string())?;
        if info.sha256.is_some() {
            hasher.update(&chunk);
        }
        downloaded += chunk.len() as u64;

        // Throttle progress events to ~every 1 MB.
        if downloaded - last_emit >= 1_048_576 {
            last_emit = downloaded;
            let _ = app.emit_to(
                events::MAIN,
                events::MODEL_PROGRESS,
                events::ModelProgressPayload {
                    id: info.id.to_string(),
                    downloaded,
                    total,
                },
            );
        }
    }
    file.flush().map_err(|e| e.to_string())?;
    drop(file);

    // Verify checksum when the catalog provides one.
    if let Some(expected) = info.sha256 {
        let actual = format!("{:x}", hasher.finalize());
        if !actual.eq_ignore_ascii_case(expected) {
            return Err("Checksum mismatch — download may be corrupt".into());
        }
    }

    std::fs::rename(&part_path, dest).map_err(|e| format!("Cannot finalize file: {e}"))?;
    Ok(())
}
