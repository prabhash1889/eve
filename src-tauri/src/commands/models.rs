//! Local-model management and status commands.

use tauri::{AppHandle, State};

use crate::models::{self, ModelStatus};
use crate::state::AppState;
use crate::transcription::{TranscriptionBenchmark, WhisperStatus};

// --- Local models ------------------------------------------------------------

/// The local-model catalog with per-model installed/active/downloading flags.
#[tauri::command]
pub fn list_models(app: AppHandle, state: State<AppState>) -> Vec<ModelStatus> {
    models::list(&app, &state)
}

/// Start (or no-op if already running) a streamed download. Progress is reported
/// via `model://progress` / `model://done` / `model://error` events.
#[tauri::command]
pub fn download_model(app: AppHandle, id: String) -> Result<(), String> {
    models::start_download(app, id)
}

/// Request cancellation of an in-flight download.
#[tauri::command]
pub fn cancel_model_download(state: State<AppState>, id: String) -> Result<(), String> {
    models::cancel(&state, &id);
    Ok(())
}

/// Delete a downloaded model file from disk.
#[tauri::command]
pub fn delete_model(app: AppHandle, id: String) -> Result<(), String> {
    models::delete(&app, &id)
}

/// Phase 2: preload the selected local Whisper model so the next dictation skips
/// the cold load. Called from the Local Models page after selecting a model or
/// switching the speech backend to local. Best-effort - a missing model or a
/// build without the feature is not a user-facing error here.
#[tauri::command]
pub async fn prewarm_local_model(state: State<'_, AppState>) -> Result<(), String> {
    let transcriber = state.transcriber.clone();
    let _ = transcriber.prewarm().await;
    Ok(())
}

/// Free any loaded local speech model that isn't the active selection (releasing
/// its VRAM on the CUDA build) and prewarm the one that is, honoring the
/// prewarm-enabled setting. Called from the Local Models page after the speech
/// backend or the selected model changes. Best-effort.
#[tauri::command]
pub async fn reconcile_local_models(state: State<'_, AppState>) -> Result<(), String> {
    let transcriber = state.transcriber.clone();
    let prewarm = state.settings.lock().local_prewarm_enabled;
    transcriber.reconcile(prewarm).await;
    Ok(())
}

/// Phase 2: readiness of the selected local Whisper model (loaded / loading /
/// last load time), for the Local Models status panel. `None` when the build has
/// no local backend.
#[tauri::command]
pub fn get_local_whisper_status(state: State<AppState>) -> Option<WhisperStatus> {
    state.transcriber.whisper_status()
}

#[tauri::command]
pub fn get_local_transcription_benchmark(state: State<AppState>) -> Option<TranscriptionBenchmark> {
    state.last_transcription_benchmark.lock().clone()
}
