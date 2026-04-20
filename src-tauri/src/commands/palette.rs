//! query surface; `recents.push` / `recents.list` moved to a dedicated

#![allow(dead_code)]

use tauri::State;

use crate::storage::types::PaletteItem;
use crate::storage::Db;

/// Default palette cap. Matches the prototype's "6 + recents" layout
const DEFAULT_LIMIT: u32 = 20;

/// `palette.query` - merged FTS + recents + action catalog. Empty
#[tauri::command]
pub async fn palette_query(
    state: State<'_, Db>,
    query: String,
    limit: Option<u32>,
) -> Result<Vec<PaletteItem>, String> {
    let lim = limit.unwrap_or(DEFAULT_LIMIT).max(1);
    state
        .palette_source(&query, lim)
        .await
        .map_err(|e: anyhow::Error| e.to_string())
}
