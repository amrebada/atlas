//! Tauri commands that bridge the in-app MCP server with the React UI.
//!
//! Currently just the approval-resolve hook the modal calls when the user
//! clicks Approve or Reject. More commands (e.g. token regenerate, list
//! pending approvals) will join this file as the feature grows.

use std::sync::Arc;

use tauri::State;

use crate::mcp::ApprovalRegistry;

/// Resolve a pending approval request. `id` is the UUID the server sent in
/// the `mcp:approval:request` event; `approve` is `true` for Approve and
/// `false` for Reject.
#[tauri::command]
pub fn mcp_approval_resolve(
    registry: State<'_, Arc<ApprovalRegistry>>,
    id: String,
    approve: bool,
) -> Result<(), String> {
    let parsed = uuid::Uuid::parse_str(&id).map_err(|e| format!("invalid id: {e}"))?;
    registry.resolve(parsed, approve)
}
