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
    /// Speech→text (whisper.cpp GGML or Parakeet ONNX — anything selectable as
    /// the local speech model).
    Whisper,
    /// Text polish (llama.cpp, GGUF).
    Llm,
}

/// One file of a multi-file model (e.g. Parakeet's decoder/vocab alongside the
/// primary encoder). Same download treatment as the primary `url`/`file_name`.
#[derive(Debug, Clone)]
pub struct ExtraFile {
    pub url: &'static str,
    /// Relative to `models_dir`; may contain a `/` for per-model subdirectories.
    pub file_name: &'static str,
    pub sha256: Option<&'static str>,
}

/// One downloadable model in the catalog.
#[derive(Debug, Clone)]
pub struct ModelInfo {
    /// Stable catalog id, also persisted in Settings and used as the filename stem.
    pub id: &'static str,
    pub kind: ModelKind,
    pub name: &'static str,
    /// Approximate total download size (all files), for display and progress.
    pub size_bytes: u64,
    pub url: &'static str,
    /// On-disk filename (extension matters: `.bin` for GGML, `.gguf` for GGUF).
    /// May contain a `/` for models that live in their own subdirectory.
    pub file_name: &'static str,
    /// Optional lowercase hex SHA-256. When `None`, verification is skipped.
    pub sha256: Option<&'static str>,
    /// Additional files downloaded with this model. Empty for single-file models.
    pub extra_files: &'static [ExtraFile],
}

impl ModelInfo {
    /// All (url, file_name, sha256) tuples: the primary file plus extras.
    fn files(&'static self) -> impl Iterator<Item = (&'static str, &'static str, Option<&'static str>)> {
        std::iter::once((self.url, self.file_name, self.sha256)).chain(
            self.extra_files
                .iter()
                .map(|f| (f.url, f.file_name, f.sha256)),
        )
    }
}

/// The static catalog. Whisper GGML weights come from `ggerganov/whisper.cpp`;
/// LLM GGUF weights from their respective `*-GGUF` repos. Sizes and SHA-256
/// digests come from each repo's Git-LFS pointer file (parity Phase B4) - if a
/// weight file is ever re-uploaded upstream, refresh both from
/// `https://huggingface.co/<repo>/raw/main/<file>`.
pub fn catalog() -> &'static [ModelInfo] {
    &[
        ModelInfo {
            id: "whisper-tiny.en",
            kind: ModelKind::Whisper,
            name: "Whisper Tiny (English)",
            size_bytes: 77_704_715,
            url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.en.bin",
            file_name: "whisper-tiny.en.bin",
            sha256: Some("921e4cf8686fdd993dcd081a5da5b6c365bfde1162e72b08d75ac75289920b1f"),
            extra_files: &[],
        },
        ModelInfo {
            id: "whisper-base.en",
            kind: ModelKind::Whisper,
            name: "Whisper Base (English)",
            size_bytes: 147_964_211,
            url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin",
            file_name: "whisper-base.en.bin",
            sha256: Some("a03779c86df3323075f5e796cb2ce5029f00ec8869eee3fdfb897afe36c6d002"),
            extra_files: &[],
        },
        ModelInfo {
            id: "whisper-small.en",
            kind: ModelKind::Whisper,
            name: "Whisper Small (English)",
            size_bytes: 487_614_201,
            url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en.bin",
            file_name: "whisper-small.en.bin",
            sha256: Some("c6138d6d58ecc8322097e0f987c32f1be8bb0a18532a3f88f734d1bbf9c41e5d"),
            extra_files: &[],
        },
        ModelInfo {
            id: "whisper-large-v3-turbo",
            kind: ModelKind::Whisper,
            name: "Whisper Large v3 Turbo (multilingual)",
            size_bytes: 1_624_555_275,
            url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin",
            file_name: "whisper-large-v3-turbo.bin",
            sha256: Some("1fc70f774d38eb169993ac391eea357ef47c88757ef72ee5943879b7e8e2bc69"),
            extra_files: &[],
        },
        // NVIDIA Parakeet TDT 0.6B v2 (English-only), int8 ONNX export from
        // istupakov/parakeet-tdt-0.6b-v2-onnx, run by `parakeet.rs` behind the
        // `local-parakeet` feature. Multi-file: everything lives in a
        // `parakeet-tdt-0.6b-v2/` subdirectory because parakeet-rs loads a model
        // *directory* by canonical filenames. `size_bytes` is the total across
        // files; sizes/SHA-256 come from the repo's Git-LFS pointer files.
        ModelInfo {
            id: "parakeet-tdt-0.6b-v2",
            kind: ModelKind::Whisper,
            name: "Parakeet TDT 0.6B v2 (English)",
            size_bytes: 661_191_781,
            url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v2-onnx/resolve/main/encoder-model.int8.onnx",
            file_name: "parakeet-tdt-0.6b-v2/encoder-model.int8.onnx",
            sha256: Some("3e0581fda6ab843888b51e56d7ee78b6d5bc3237ec113af1f732d1d5286aa155"),
            extra_files: &[
                ExtraFile {
                    url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v2-onnx/resolve/main/decoder_joint-model.int8.onnx",
                    file_name: "parakeet-tdt-0.6b-v2/decoder_joint-model.int8.onnx",
                    sha256: Some("a449f49acd68979d418651dd2dcb737cc0f1bf0225e009e29ee326354edbf7d3"),
                },
                // Small non-LFS text files — no upstream checksum to pin.
                ExtraFile {
                    url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v2-onnx/resolve/main/vocab.txt",
                    file_name: "parakeet-tdt-0.6b-v2/vocab.txt",
                    sha256: None,
                },
                ExtraFile {
                    url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v2-onnx/resolve/main/config.json",
                    file_name: "parakeet-tdt-0.6b-v2/config.json",
                    sha256: None,
                },
            ],
        },
        ModelInfo {
            id: "qwen2.5-1.5b-instruct",
            kind: ModelKind::Llm,
            name: "Qwen2.5 1.5B Instruct (Q4_K_M)",
            size_bytes: 1_117_320_736,
            url: "https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct-GGUF/resolve/main/qwen2.5-1.5b-instruct-q4_k_m.gguf",
            file_name: "qwen2.5-1.5b-instruct-q4_k_m.gguf",
            sha256: Some("6a1a2eb6d15622bf3c96857206351ba97e1af16c30d7a74ee38970e434e9407e"),
            extra_files: &[],
        },
        ModelInfo {
            id: "llama-3.2-1b-instruct",
            kind: ModelKind::Llm,
            name: "Llama 3.2 1B Instruct (Q4_K_M)",
            size_bytes: 807_694_464,
            url: "https://huggingface.co/bartowski/Llama-3.2-1B-Instruct-GGUF/resolve/main/Llama-3.2-1B-Instruct-Q4_K_M.gguf",
            file_name: "Llama-3.2-1B-Instruct-Q4_K_M.gguf",
            sha256: Some("6f85a640a97cf2bf5b8e764087b1e83da0fdb51d7c9fab7d0fece9385611df83"),
            extra_files: &[],
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
            let installed = models_dir(app)
                .map(|dir| m.files().all(|(_, name, _)| dir.join(name).exists()))
                .unwrap_or(false);
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

/// Delete a downloaded model's files. No-op if they aren't present. For
/// subdirectory models the now-empty directory is pruned best-effort.
pub fn delete(app: &AppHandle, id: &str) -> Result<(), String> {
    let (Some(info), Ok(dir)) = (find(id), models_dir(app)) else {
        return Ok(());
    };
    for (_, name, _) in info.files() {
        let path = dir.join(name);
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| e.to_string())?;
        }
    }
    if info.file_name.contains('/') {
        if let Some(parent) = dir.join(info.file_name).parent() {
            // `remove_dir` only deletes empty directories, so this can't take
            // anything but the model's own leftover folder.
            let _ = std::fs::remove_dir(parent);
        }
    }
    Ok(())
}

/// Kick off an async streamed download of all the model's files. Registers a
/// cancel flag, streams each file to a `.part`, emits cumulative
/// `MODEL_PROGRESS`, verifies each optional SHA-256, then atomically renames
/// into place and emits `MODEL_DONE` (or `MODEL_ERROR`). Returns immediately;
/// progress is reported via events.
pub fn start_download(app: AppHandle, id: String) -> Result<(), String> {
    let info = find(&id).ok_or_else(|| format!("Unknown model id: {id}"))?;
    let dir = models_dir(&app).map_err(|e| format!("Could not resolve model path: {e}"))?;

    {
        let state = app.state::<AppState>();
        let mut map = state.model_downloads.lock();
        if map.contains_key(info.id) {
            return Err("Already downloading".into());
        }
        map.insert(info.id.to_string(), Arc::new(AtomicBool::new(false)));
    }

    tauri::async_runtime::spawn(async move {
        let result = download_all(&app, info, &dir).await;

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
                // Clean up any partial files.
                for (_, name, _) in info.files() {
                    let _ = std::fs::remove_file(dir.join(name).with_extension("part"));
                }
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

/// The streaming download itself: every file of the model in sequence, with
/// progress reported cumulatively against the catalog's total size. Errors
/// (including cancellation) propagate to the caller, which emits the terminal
/// event. Files already fully downloaded (e.g. from a cancelled earlier
/// attempt) are streamed again — simple and always correct.
async fn download_all(
    app: &AppHandle,
    info: &'static ModelInfo,
    dir: &std::path::Path,
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

    let total = info.size_bytes;
    let mut downloaded: u64 = 0;
    let mut last_emit: u64 = 0;

    for (url, name, sha256) in info.files() {
        let dest = dir.join(name);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("Cannot create folder: {e}"))?;
        }

        let resp = client
            .get(url)
            .send()
            .await
            .map_err(|e| format!("Connection failed: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("Download failed: HTTP {}", resp.status()));
        }

        let part_path = dest.with_extension("part");
        let mut file =
            std::fs::File::create(&part_path).map_err(|e| format!("Cannot write file: {e}"))?;
        let mut hasher = Sha256::new();

        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            if cancel_flag.load(Ordering::SeqCst) {
                return Err("Download cancelled".into());
            }
            // A read that stalls past the client's `read_timeout` surfaces here
            // as an error rather than hanging forever.
            let chunk = chunk.map_err(|e| format!("Download interrupted: {e}"))?;
            file.write_all(&chunk).map_err(|e| e.to_string())?;
            if sha256.is_some() {
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
        if let Some(expected) = sha256 {
            let actual = format!("{:x}", hasher.finalize());
            if !actual.eq_ignore_ascii_case(expected) {
                return Err("Checksum mismatch — download may be corrupt".into());
            }
        }

        std::fs::rename(&part_path, &dest).map_err(|e| format!("Cannot finalize file: {e}"))?;
    }
    Ok(())
}
