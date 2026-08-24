//! File-transcription queue commands.

use tauri::AppHandle;

/// Enqueue audio files for transcription. Files are read in place (never copied),
/// decoded, and processed serially in the background; finished items land in
/// History. Returns the queued items (id + file name) for the Dashboard card.
#[tauri::command]
pub fn transcribe_files(app: AppHandle, paths: Vec<String>) -> Vec<crate::file_transcribe::QueueItem> {
    crate::file_transcribe::enqueue(&app, paths)
}

/// Cancel a queued (or in-flight) file transcription by id.
#[tauri::command]
pub fn cancel_queue_item(app: AppHandle, id: u64) {
    crate::file_transcribe::cancel(&app, id);
}
