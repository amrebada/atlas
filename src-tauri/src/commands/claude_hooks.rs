//! Claude Code hook bridge IPC - install/uninstall the hooks and answer
//! permission requests. See `crate::claude_hooks` for the mechanism.

use std::sync::Arc;

use tauri::State;

use crate::claude_hooks::{ClaudeHooksBridge, ClaudeHooksStatus, PermissionDecision};

/// `claude_hooks.status` - are the Notification / PermissionRequest hooks
/// wired into `~/.claude/settings.json`?
#[tauri::command]
pub async fn claude_hooks_status(
    bridge: State<'_, Arc<ClaudeHooksBridge>>,
) -> Result<ClaudeHooksStatus, String> {
    bridge.status().map_err(|e: anyhow::Error| e.to_string())
}

/// `claude_hooks.install` - write the hook script and register it.
#[tauri::command]
pub async fn claude_hooks_install(
    bridge: State<'_, Arc<ClaudeHooksBridge>>,
) -> Result<ClaudeHooksStatus, String> {
    bridge.install().map_err(|e: anyhow::Error| e.to_string())
}

/// `claude_hooks.uninstall` - remove Atlas's hook entries + script.
#[tauri::command]
pub async fn claude_hooks_uninstall(
    bridge: State<'_, Arc<ClaudeHooksBridge>>,
) -> Result<ClaudeHooksStatus, String> {
    bridge.uninstall().map_err(|e: anyhow::Error| e.to_string())
}

/// `claude_permission.resolve` - answer a `claude:permission:request`.
#[tauri::command]
pub async fn claude_permission_resolve(
    bridge: State<'_, Arc<ClaudeHooksBridge>>,
    id: String,
    decision: PermissionDecision,
    message: Option<String>,
) -> Result<(), String> {
    bridge
        .resolve_permission(&id, decision, message.as_deref())
        .map_err(|e: anyhow::Error| e.to_string())
}
