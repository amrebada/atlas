//! Claude Code hook bridge.
//!
//! Atlas installs a tiny shell script as a Claude Code hook (in
//! `~/.claude/settings.json`) for two events:
//!
//! * `Notification` — Claude Code needs attention (permission prompt, idle
//!   waiting for input, …). The script drops the payload into an inbox
//!   directory and exits; Atlas raises an OS notification + toast.
//! * `PermissionRequest` — Claude Code is about to show a permission
//!   dialog. The script drops the request into the inbox, then waits for
//!   Atlas to write a decision into the outbox. Atlas shows an Allow /
//!   Deny dialog; the decision is printed to stdout in Claude Code's
//!   `hookSpecificOutput` shape. No decision (timeout, "ask in terminal",
//!   Atlas not running) → the script prints nothing and Claude Code shows
//!   its own prompt as usual.
//!
//! Transport is plain files under `<app_data>/claude-hooks/` so it works
//! for sessions started anywhere (Atlas panes, iTerm, VS Code), needs no
//! network port, and degrades to "nothing happens" when Atlas is closed.
//! A `std` poller thread drains the inbox every [`POLL_INTERVAL`].

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::AppHandle;
use ts_rs::TS;

use crate::events;
use crate::storage::types::ProjectFilter;
use crate::storage::Db;

const POLL_INTERVAL: Duration = Duration::from_millis(400);
/// Inbox files older than this on pickup are discarded (Atlas was closed
/// when they were written; firing them now would just confuse).
const STALE_AFTER: Duration = Duration::from_secs(120);
/// How long the hook script waits for a permission decision. Keep in sync
/// with the loop in [`HOOK_SCRIPT`] (1450 × 0.2s ≈ 290s) and below the
/// hook `timeout` Atlas writes into settings.json.
pub const PERMISSION_WAIT_SECS: u64 = 290;
const HOOK_TIMEOUT_SECS: u64 = 300;
const HOOK_EVENTS: [&str; 2] = ["Notification", "PermissionRequest"];
const SCRIPT_NAME: &str = "atlas-hook.sh";

const HOOK_SCRIPT: &str = r#"#!/bin/bash
# Atlas <-> Claude Code hook bridge. Installed by Atlas (Settings > Providers).
# Safe to delete; Atlas re-creates it when hooks are re-enabled.
#
# Usage (from ~/.claude/settings.json): atlas-hook.sh <HookEventName>
# Reads Claude Code's JSON payload on stdin, drops it into Atlas's inbox and,
# for PermissionRequest, waits for Atlas to answer via the outbox.
DIR="$(cd "$(dirname "$0")" && pwd)"
EVENT="$1"
INBOX="$DIR/inbox"
OUTBOX="$DIR/outbox"
PIDFILE="$DIR/atlas.pid"

# Atlas not running -> do nothing, let Claude Code behave normally.
if [ ! -f "$PIDFILE" ] || ! kill -0 "$(cat "$PIDFILE" 2>/dev/null)" 2>/dev/null; then
  exit 0
fi

mkdir -p "$INBOX" "$OUTBOX"
ID="$(date +%s)-$$-$RANDOM"
PAYLOAD="$(cat)"
[ -z "$PAYLOAD" ] && PAYLOAD="null"
TMP="$INBOX/.$ID.tmp"
printf '{"id":"%s","event":"%s","payload":%s}' "$ID" "$EVENT" "$PAYLOAD" > "$TMP" && mv "$TMP" "$INBOX/$ID.json"

if [ "$EVENT" = "PermissionRequest" ]; then
  i=0
  while [ $i -lt 1450 ]; do
    if [ -f "$OUTBOX/$ID.json" ]; then
      # Empty file = "no decision, ask in the terminal".
      [ -s "$OUTBOX/$ID.json" ] && cat "$OUTBOX/$ID.json"
      rm -f "$OUTBOX/$ID.json"
      exit 0
    fi
    sleep 0.2
    i=$((i+1))
  done
  rm -f "$INBOX/$ID.json"
fi
exit 0
"#;

/// Snapshot of the integration for the settings UI.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct ClaudeHooksStatus {
    /// Both hooks are wired up and the script exists.
    pub installed: bool,
    pub notification_hook: bool,
    pub permission_hook: bool,
    pub script_path: String,
    pub settings_path: String,
}

/// A decision for a pending `PermissionRequest`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "lowercase"
)]
pub enum PermissionDecision {
    Allow,
    Deny,
    /// No decision — let Claude Code show its own prompt.
    Ask,
}

/// Envelope the hook script writes into the inbox.
#[derive(Debug, Deserialize)]
struct Envelope {
    id: String,
    event: String,
    #[serde(default)]
    payload: Value,
}

struct Pending {
    received_at: Instant,
}

pub struct ClaudeHooksBridge {
    root: PathBuf,
    claude_settings: PathBuf,
    app: Option<AppHandle>,
    db: Option<Db>,
    pending: Mutex<HashMap<String, Pending>>,
}

impl ClaudeHooksBridge {
    /// Production constructor: wires the app handle + db and starts the
    /// inbox poller thread.
    pub fn start(app: AppHandle, app_data_dir: &Path, db: Db) -> Arc<Self> {
        let bridge = Arc::new(Self::with_paths(
            app_data_dir.join("claude-hooks"),
            default_claude_settings_path(),
            Some(app),
            Some(db),
        ));
        if let Err(e) = bridge.write_pid_file() {
            tracing::warn!(error = %e, "claude-hooks: pid file");
        }
        let poller = Arc::clone(&bridge);
        if let Err(e) = std::thread::Builder::new()
            .name("atlas-claude-hooks".into())
            .spawn(move || poller.poll_loop())
        {
            tracing::warn!(error = %e, "claude-hooks: spawn poller");
        }
        bridge
    }

    fn with_paths(
        root: PathBuf,
        claude_settings: PathBuf,
        app: Option<AppHandle>,
        db: Option<Db>,
    ) -> Self {
        Self {
            root,
            claude_settings,
            app,
            db,
            pending: Mutex::new(HashMap::new()),
        }
    }

    pub fn script_path(&self) -> PathBuf {
        self.root.join(SCRIPT_NAME)
    }
    fn inbox(&self) -> PathBuf {
        self.root.join("inbox")
    }
    fn outbox(&self) -> PathBuf {
        self.root.join("outbox")
    }

    fn write_pid_file(&self) -> anyhow::Result<()> {
        fs::create_dir_all(&self.root)?;
        fs::write(self.root.join("atlas.pid"), std::process::id().to_string())?;
        Ok(())
    }

    // ---- install / uninstall / status ----------------------------------

    /// Write the hook script and register it for both events in
    /// `~/.claude/settings.json`, preserving every other key and any
    /// hooks the user already has. Idempotent.
    pub fn install(&self) -> anyhow::Result<ClaudeHooksStatus> {
        fs::create_dir_all(self.inbox())?;
        fs::create_dir_all(self.outbox())?;
        let script = self.script_path();
        fs::write(&script, HOOK_SCRIPT)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script, fs::Permissions::from_mode(0o755))?;
        }
        self.write_pid_file()?;

        let mut settings = read_settings_value(&self.claude_settings)?;
        let hooks = settings
            .as_object_mut()
            .expect("settings root is an object")
            .entry("hooks")
            .or_insert_with(|| json!({}));
        if !hooks.is_object() {
            *hooks = json!({});
        }
        let hooks = hooks.as_object_mut().expect("hooks is an object");
        let script_str = script.to_string_lossy().into_owned();
        for event in HOOK_EVENTS {
            let list = hooks.entry(event).or_insert_with(|| json!([]));
            if !list.is_array() {
                *list = json!([]);
            }
            let arr = list.as_array_mut().expect("event list is an array");
            arr.retain(|entry| !entry_is_ours(entry, &script_str));
            arr.push(json!({
                "hooks": [{
                    "type": "command",
                    "command": format!("'{script_str}' {event}"),
                    "timeout": HOOK_TIMEOUT_SECS,
                }]
            }));
        }
        write_settings_value(&self.claude_settings, &settings)?;
        self.status()
    }

    /// Remove Atlas's entries (other hooks are untouched) and the script.
    pub fn uninstall(&self) -> anyhow::Result<ClaudeHooksStatus> {
        let script_str = self.script_path().to_string_lossy().into_owned();
        if self.claude_settings.exists() {
            let mut settings = read_settings_value(&self.claude_settings)?;
            let mut hooks_empty = false;
            if let Some(hooks) = settings.get_mut("hooks").and_then(Value::as_object_mut) {
                for event in HOOK_EVENTS {
                    let drop_key = match hooks.get_mut(event).and_then(Value::as_array_mut) {
                        Some(arr) => {
                            arr.retain(|entry| !entry_is_ours(entry, &script_str));
                            arr.is_empty()
                        }
                        None => false,
                    };
                    if drop_key {
                        hooks.remove(event);
                    }
                }
                hooks_empty = hooks.is_empty();
            }
            if hooks_empty {
                if let Some(obj) = settings.as_object_mut() {
                    obj.remove("hooks");
                }
            }
            write_settings_value(&self.claude_settings, &settings)?;
        }
        let _ = fs::remove_file(self.script_path());
        self.status()
    }

    pub fn status(&self) -> anyhow::Result<ClaudeHooksStatus> {
        let script = self.script_path();
        let script_str = script.to_string_lossy().into_owned();
        let settings = if self.claude_settings.exists() {
            read_settings_value(&self.claude_settings)?
        } else {
            json!({})
        };
        let has = |event: &str| -> bool {
            settings
                .get("hooks")
                .and_then(|h| h.get(event))
                .and_then(Value::as_array)
                .map(|arr| arr.iter().any(|e| entry_is_ours(e, &script_str)))
                .unwrap_or(false)
        };
        let notification_hook = has("Notification");
        let permission_hook = has("PermissionRequest");
        Ok(ClaudeHooksStatus {
            installed: script.exists() && notification_hook && permission_hook,
            notification_hook,
            permission_hook,
            script_path: script_str,
            settings_path: self.claude_settings.to_string_lossy().into_owned(),
        })
    }

    // ---- permission decisions -------------------------------------------

    /// Answer a pending `PermissionRequest`. Writes the outbox file the
    /// hook script is polling for. Unknown ids are still answered (the
    /// script may have outlived an Atlas restart) but logged.
    pub fn resolve_permission(
        &self,
        id: &str,
        decision: PermissionDecision,
        message: Option<&str>,
    ) -> anyhow::Result<()> {
        if !is_safe_id(id) {
            anyhow::bail!("invalid permission request id");
        }
        let known = self.pending.lock().unwrap().remove(id).is_some();
        if !known {
            tracing::debug!(id, "claude-hooks: resolving unknown permission id");
        }
        let body = decision_body(decision, message);
        fs::create_dir_all(self.outbox())?;
        let tmp = self.outbox().join(format!(".{id}.tmp"));
        fs::write(&tmp, body)?;
        fs::rename(&tmp, self.outbox().join(format!("{id}.json")))?;
        Ok(())
    }

    // ---- inbox poller ---------------------------------------------------

    fn poll_loop(self: Arc<Self>) {
        loop {
            self.drain_inbox();
            self.expire_pending();
            std::thread::sleep(POLL_INTERVAL);
        }
    }

    fn drain_inbox(&self) {
        let Ok(entries) = fs::read_dir(self.inbox()) else {
            return;
        };
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.extension().and_then(|e| e.to_str()) == Some("json")
                    && !p
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(|n| n.starts_with('.'))
                        .unwrap_or(true)
            })
            .collect();
        files.sort();
        for path in files {
            let stale = fs::metadata(&path)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| SystemTime::now().duration_since(t).ok())
                .map(|age| age > STALE_AFTER)
                .unwrap_or(false);
            let raw = fs::read_to_string(&path);
            let _ = fs::remove_file(&path);
            if stale {
                continue;
            }
            match raw
                .map_err(anyhow::Error::from)
                .and_then(|s| serde_json::from_str::<Envelope>(&s).map_err(anyhow::Error::from))
            {
                Ok(env) => self.dispatch(env),
                Err(e) => {
                    tracing::warn!(error = %e, path = %path.display(), "claude-hooks: bad inbox file")
                }
            }
        }
    }

    fn expire_pending(&self) {
        let expired: Vec<String> = {
            let mut pending = self.pending.lock().unwrap();
            let now = Instant::now();
            let ids: Vec<String> = pending
                .iter()
                .filter(|(_, p)| now.duration_since(p.received_at).as_secs() > PERMISSION_WAIT_SECS)
                .map(|(id, _)| id.clone())
                .collect();
            for id in &ids {
                pending.remove(id);
            }
            ids
        };
        if let Some(app) = &self.app {
            for id in expired {
                let _ = events::emit_claude_permission_cancelled(app, &id);
            }
        }
    }

    fn dispatch(&self, env: Envelope) {
        let cwd = env.payload["cwd"].as_str().unwrap_or("").to_string();
        let session_id = env.payload["session_id"].as_str().unwrap_or("").to_string();
        let (project_id, project_name) = self.project_for_cwd(&cwd);
        let Some(app) = &self.app else {
            return;
        };
        match env.event.as_str() {
            "Notification" => {
                let payload = events::ClaudeNotificationPayload {
                    id: env.id,
                    project_id,
                    project_name,
                    cwd,
                    session_id,
                    message: env.payload["message"].as_str().unwrap_or("").to_string(),
                    title: env.payload["title"].as_str().map(str::to_string),
                    notification_type: env.payload["notification_type"]
                        .as_str()
                        .map(str::to_string),
                };
                if let Err(e) = events::emit_claude_notification(app, &payload) {
                    tracing::warn!(error = %e, "emit claude:notification");
                }
            }
            "PermissionRequest" => {
                self.pending.lock().unwrap().insert(
                    env.id.clone(),
                    Pending {
                        received_at: Instant::now(),
                    },
                );
                let payload = events::ClaudePermissionPayload {
                    id: env.id,
                    project_id,
                    project_name,
                    cwd,
                    session_id,
                    tool_name: env.payload["tool_name"].as_str().unwrap_or("?").to_string(),
                    tool_input: env
                        .payload
                        .get("tool_input")
                        .cloned()
                        .unwrap_or(Value::Null),
                    permission_mode: env.payload["permission_mode"].as_str().map(str::to_string),
                    expires_in_secs: PERMISSION_WAIT_SECS,
                };
                if let Err(e) = events::emit_claude_permission_request(app, &payload) {
                    tracing::warn!(error = %e, "emit claude:permission:request");
                }
            }
            other => tracing::debug!(event = other, "claude-hooks: ignoring event"),
        }
    }

    /// Longest indexed project path that contains `cwd`.
    fn project_for_cwd(&self, cwd: &str) -> (Option<String>, Option<String>) {
        let Some(db) = &self.db else {
            return (None, None);
        };
        if cwd.is_empty() {
            return (None, None);
        }
        let projects = match tauri::async_runtime::block_on(db.list_projects(ProjectFilter {
            include_archived: true,
            ..ProjectFilter::default()
        })) {
            Ok(p) => p,
            Err(e) => {
                tracing::debug!(error = %e, "claude-hooks: list_projects");
                return (None, None);
            }
        };
        let cwd_path = Path::new(cwd);
        projects
            .iter()
            .filter(|p| cwd_path.starts_with(&p.path))
            .max_by_key(|p| p.path.len())
            .map(|p| (Some(p.id.clone()), Some(p.name.clone())))
            .unwrap_or((None, None))
    }
}

// ---- helpers ---------------------------------------------------------------

fn default_claude_settings_path() -> PathBuf {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".claude").join("settings.json")
}

/// A hook group entry (`{ matcher?, hooks: [...] }`) is ours when any of
/// its commands references our script path.
fn entry_is_ours(entry: &Value, script: &str) -> bool {
    entry
        .get("hooks")
        .and_then(Value::as_array)
        .map(|hooks| {
            hooks.iter().any(|h| {
                h.get("command")
                    .and_then(Value::as_str)
                    .map(|c| c.contains(script))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

fn read_settings_value(path: &Path) -> anyhow::Result<Value> {
    if !path.exists() {
        return Ok(json!({}));
    }
    let raw = fs::read_to_string(path)?;
    if raw.trim().is_empty() {
        return Ok(json!({}));
    }
    let v: Value = serde_json::from_str(&raw)
        .map_err(|e| anyhow::anyhow!("{} is not valid JSON: {e}", path.display()))?;
    if !v.is_object() {
        anyhow::bail!("{} must contain a JSON object", path.display());
    }
    Ok(v)
}

fn write_settings_value(path: &Path, value: &Value) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.atlas-tmp");
    fs::write(&tmp, serde_json::to_string_pretty(value)? + "\n")?;
    fs::rename(&tmp, path)?;
    Ok(())
}

fn is_safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Claude Code's `PermissionRequest` hook output. An empty body means
/// "no decision" and the hook script prints nothing.
fn decision_body(decision: PermissionDecision, message: Option<&str>) -> String {
    match decision {
        PermissionDecision::Ask => String::new(),
        PermissionDecision::Allow => json!({
            "hookSpecificOutput": {
                "hookEventName": "PermissionRequest",
                "decision": { "behavior": "allow" }
            }
        })
        .to_string(),
        PermissionDecision::Deny => json!({
            "hookSpecificOutput": {
                "hookEventName": "PermissionRequest",
                "decision": {
                    "behavior": "deny",
                    "message": message.unwrap_or("Denied by the user in Atlas")
                }
            }
        })
        .to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TmpDir(PathBuf);
    impl TmpDir {
        fn new(tag: &str) -> Self {
            let base = std::env::temp_dir().join(format!(
                "atlas-hooks-test-{}-{}-{}",
                tag,
                std::process::id(),
                chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
            ));
            fs::create_dir_all(&base).unwrap();
            Self(base)
        }
    }
    impl Drop for TmpDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn bridge(tmp: &TmpDir) -> ClaudeHooksBridge {
        ClaudeHooksBridge::with_paths(
            tmp.0.join("claude-hooks"),
            tmp.0.join("dot-claude").join("settings.json"),
            None,
            None,
        )
    }

    #[test]
    fn install_merges_and_uninstall_restores_user_settings() {
        let tmp = TmpDir::new("install");
        let b = bridge(&tmp);
        fs::create_dir_all(b.claude_settings.parent().unwrap()).unwrap();
        fs::write(
            &b.claude_settings,
            r#"{
  "permissions": { "allow": ["Bash(ls:*)"] },
  "hooks": {
    "Notification": [
      { "hooks": [ { "type": "command", "command": "say hi" } ] }
    ]
  }
}"#,
        )
        .unwrap();

        let before = b.status().unwrap();
        assert!(!before.installed);

        let st = b.install().unwrap();
        assert!(st.installed && st.notification_hook && st.permission_hook);
        assert!(b.script_path().exists());
        assert!(b.root.join("atlas.pid").exists());

        let v = read_settings_value(&b.claude_settings).unwrap();
        assert_eq!(
            v["permissions"]["allow"][0], "Bash(ls:*)",
            "other keys kept"
        );
        let notif = v["hooks"]["Notification"].as_array().unwrap();
        assert_eq!(notif.len(), 2, "user hook kept + ours added");
        assert_eq!(notif[0]["hooks"][0]["command"], "say hi");
        let ours = &notif[1]["hooks"][0];
        assert!(ours["command"]
            .as_str()
            .unwrap()
            .ends_with("atlas-hook.sh' Notification"));
        assert_eq!(ours["timeout"], HOOK_TIMEOUT_SECS);
        assert_eq!(v["hooks"]["PermissionRequest"].as_array().unwrap().len(), 1);

        // Idempotent: a second install doesn't duplicate.
        b.install().unwrap();
        let v = read_settings_value(&b.claude_settings).unwrap();
        assert_eq!(v["hooks"]["Notification"].as_array().unwrap().len(), 2);

        let st = b.uninstall().unwrap();
        assert!(!st.installed);
        let v = read_settings_value(&b.claude_settings).unwrap();
        assert_eq!(v["hooks"]["Notification"].as_array().unwrap().len(), 1);
        assert!(v["hooks"].get("PermissionRequest").is_none());
        assert_eq!(v["permissions"]["allow"][0], "Bash(ls:*)");
        assert!(!b.script_path().exists());
    }

    #[test]
    fn install_creates_settings_when_missing_and_uninstall_cleans_hooks_key() {
        let tmp = TmpDir::new("fresh");
        let b = bridge(&tmp);
        b.install().unwrap();
        let v = read_settings_value(&b.claude_settings).unwrap();
        assert!(v["hooks"]["Notification"].is_array());
        b.uninstall().unwrap();
        let v = read_settings_value(&b.claude_settings).unwrap();
        assert!(v.get("hooks").is_none(), "{v}");
    }

    #[test]
    fn resolve_writes_outbox_in_claude_code_shape() {
        let tmp = TmpDir::new("resolve");
        let b = bridge(&tmp);
        b.pending.lock().unwrap().insert(
            "req-1".into(),
            Pending {
                received_at: Instant::now(),
            },
        );
        b.resolve_permission("req-1", PermissionDecision::Allow, None)
            .unwrap();
        let out: Value =
            serde_json::from_str(&fs::read_to_string(b.outbox().join("req-1.json")).unwrap())
                .unwrap();
        assert_eq!(
            out["hookSpecificOutput"]["hookEventName"],
            "PermissionRequest"
        );
        assert_eq!(out["hookSpecificOutput"]["decision"]["behavior"], "allow");
        assert!(b.pending.lock().unwrap().is_empty());

        b.resolve_permission("req-2", PermissionDecision::Deny, Some("nope"))
            .unwrap();
        let out: Value =
            serde_json::from_str(&fs::read_to_string(b.outbox().join("req-2.json")).unwrap())
                .unwrap();
        assert_eq!(out["hookSpecificOutput"]["decision"]["behavior"], "deny");
        assert_eq!(out["hookSpecificOutput"]["decision"]["message"], "nope");

        b.resolve_permission("req-3", PermissionDecision::Ask, None)
            .unwrap();
        assert_eq!(
            fs::metadata(b.outbox().join("req-3.json")).unwrap().len(),
            0,
            "ask = empty file so the script prints nothing"
        );

        assert!(b
            .resolve_permission("../evil", PermissionDecision::Allow, None)
            .is_err());
    }

    #[test]
    fn drain_inbox_parses_and_tracks_permission_requests() {
        let tmp = TmpDir::new("drain");
        let b = bridge(&tmp);
        fs::create_dir_all(b.inbox()).unwrap();
        fs::write(
            b.inbox().join("a.json"),
            r#"{"id":"a","event":"PermissionRequest","payload":{"cwd":"/x","tool_name":"Bash","tool_input":{"command":"ls"}}}"#,
        )
        .unwrap();
        fs::write(b.inbox().join(".b.tmp"), "partial").unwrap();
        fs::write(b.inbox().join("c.json"), "not json").unwrap();

        b.drain_inbox();
        // Without an AppHandle nothing is emitted, but parsing + cleanup
        // still happen; the pending map is only touched inside dispatch
        // (which needs the handle), so assert on the filesystem here.
        assert!(!b.inbox().join("a.json").exists());
        assert!(!b.inbox().join("c.json").exists());
        assert!(
            b.inbox().join(".b.tmp").exists(),
            "in-flight temp files are left alone"
        );
    }

    #[test]
    fn hook_script_is_well_formed() {
        assert!(HOOK_SCRIPT.starts_with("#!/bin/bash"));
        assert!(HOOK_SCRIPT.contains("PermissionRequest"));
        assert!(HOOK_SCRIPT.contains("atlas.pid"));
        // The script's wait (1450 × 0.2s) must stay under the hook timeout.
        let script_wait_secs = 1450.0 * 0.2;
        let (wait, timeout) = (PERMISSION_WAIT_SECS, HOOK_TIMEOUT_SECS);
        assert!(script_wait_secs < timeout as f64);
        assert!(wait < timeout);
    }
}
