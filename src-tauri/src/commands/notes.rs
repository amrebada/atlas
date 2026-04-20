//! Note IPC commands - owned by **D4**.

// P4 wires these into `tauri::generate_handler![...]` in `lib.rs`. Until
#![allow(dead_code)]

use tauri::State;

use crate::storage::types::Note;
use crate::storage::Db;

/// `notes.list` - every `<project>/.atlas/notes/*.json`, pinned first
#[tauri::command]
pub async fn notes_list(state: State<'_, Db>, project_id: String) -> Result<Vec<Note>, String> {
    state
        .notes_list(&project_id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}

/// `notes.get` - fetch a single note by id. `None` if missing.
#[tauri::command]
pub async fn notes_get(
    state: State<'_, Db>,
    project_id: String,
    note_id: String,
) -> Result<Option<Note>, String> {
    state
        .notes_get(&project_id, &note_id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}

/// `notes.upsert` - atomic JSON write + rebuild the `notes_fts` row +
#[tauri::command]
pub async fn notes_upsert(
    state: State<'_, Db>,
    project_id: String,
    note: Note,
) -> Result<(), String> {
    state
        .notes_upsert(&project_id, &note)
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}

/// `notes.delete` - remove the JSON file + FTS row + refresh count.
#[tauri::command]
pub async fn notes_delete(
    state: State<'_, Db>,
    project_id: String,
    note_id: String,
) -> Result<(), String> {
    state
        .notes_delete(&project_id, &note_id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}

/// `notes.pin` - toggle the pinned flag + bump `updatedAt`. Errors if
#[tauri::command]
pub async fn notes_pin(
    state: State<'_, Db>,
    project_id: String,
    note_id: String,
    pinned: bool,
) -> Result<(), String> {
    state
        .notes_pin(&project_id, &note_id, pinned)
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}

/// `notes.search` - FTS5 match against `title + body_plain`, hydrated
#[tauri::command]
pub async fn notes_search(
    state: State<'_, Db>,
    project_id: String,
    query: String,
) -> Result<Vec<Note>, String> {
    state
        .notes_search(&project_id, &query)
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}
