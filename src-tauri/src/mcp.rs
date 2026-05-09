//! Embedded Model Context Protocol server.
//!
//! Phase 1 of the remote-control feature: an HTTP+JSON-RPC endpoint on
//! loopback that exposes a small set of read-only Atlas tools to a local
//! AI CLI (Claude Code, Codex, etc.). Off by default — the user opts in
//! from Settings → Advanced. A bearer token is required, so other local
//! processes can't reach it just by guessing the port.
//!
//! Config resolution order (each field individually):
//! 1. Environment variable (`ATLAS_MCP_ENABLED`, `ATLAS_MCP_PORT`,
//!    `ATLAS_MCP_TOKEN`) — convenient for `pnpm tauri dev`.
//! 2. `settings.advanced.mcp` from `settings.json` — production path.
//!
//! Wire format: a single POST `/mcp` handler that dispatches JSON-RPC 2.0
//! messages. Streaming SSE responses aren't needed yet — every method we
//! support returns synchronously.
//!
//! Approval model (Phase 2): mutating tools require a `scoped_token` issued
//! by `approval_request_plan`. The server emits a `mcp:approval:request`
//! Tauri event and waits up to 120s for the user to click Approve/Reject in
//! the in-app dialog. Once approved, the token is valid for any number of
//! mutating calls within a 60s window — matches the user's preference for
//! batched approvals over per-action prompts.

use crate::sessions::SessionsManager;
use crate::storage::{types::ProjectFilter, Db};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use tokio::sync::oneshot;
use uuid::Uuid;

const PROTOCOL_VERSION: &str = "2024-11-05";
const SERVER_NAME: &str = "atlas-mcp";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone)]
struct McpState {
    db: Db,
    sessions: Arc<SessionsManager>,
    app_data_dir: PathBuf,
    bearer_token: String,
    approvals: Arc<ApprovalRegistry>,
    /// Used by mutating tools to emit the same `project:updated` /
    /// `git:status` events the in-app commands emit, so the React cache
    /// stays in sync without an app restart.
    app: AppHandle,
}

/// Start the MCP server on a background task if the user has opted in.
/// Silent no-op when disabled. Reads `settings.advanced.mcp` and lets env
/// vars override individual fields (see module docs).
pub fn maybe_spawn(
    db: Db,
    sessions: Arc<SessionsManager>,
    app_data_dir: PathBuf,
    approvals: Arc<ApprovalRegistry>,
    app: AppHandle,
) {
    let resolved = resolve_config(&app_data_dir);
    let Some(cfg) = resolved else {
        return;
    };

    let state = McpState {
        db,
        sessions,
        app_data_dir,
        bearer_token: cfg.token,
        approvals,
        app,
    };
    let addr = SocketAddr::from(([127, 0, 0, 1], cfg.port));

    tauri::async_runtime::spawn(async move {
        let app = Router::new().route("/mcp", post(handle)).with_state(state);
        match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => {
                tracing::info!(%addr, "atlas-mcp listening");
                if let Err(e) = axum::serve(listener, app).await {
                    tracing::error!(error = %e, "atlas-mcp serve loop ended with error");
                }
            }
            Err(e) => {
                tracing::error!(error = %e, %addr, "atlas-mcp bind failed");
            }
        }
    });
}

struct ResolvedConfig {
    port: u16,
    token: String,
}

fn resolve_config(app_data_dir: &PathBuf) -> Option<ResolvedConfig> {
    // Settings come from disk; env vars layered on top for dev convenience.
    let settings = tauri::async_runtime::block_on(crate::storage::settings::load(app_data_dir))
        .ok()
        .map(|s| s.advanced.mcp)
        .unwrap_or_default();

    let env_enabled = std::env::var("ATLAS_MCP_ENABLED").as_deref() == Ok("1");
    let enabled = env_enabled || settings.enabled;
    if !enabled {
        return None;
    }

    let port: u16 = std::env::var("ATLAS_MCP_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(if settings.port == 0 { 8765 } else { settings.port });

    let token = std::env::var("ATLAS_MCP_TOKEN")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or(settings.token);

    if token.is_empty() {
        tracing::warn!(
            "MCP server is enabled but no bearer token is configured; \
             refusing to start without auth. Set one in Settings → Advanced \
             or via ATLAS_MCP_TOKEN."
        );
        return None;
    }

    Some(ResolvedConfig { port, token })
}

// ---------- Approval registry ----------

const APPROVAL_REQUEST_TIMEOUT_SECS: u64 = 120;
const SCOPED_TOKEN_TTL_SECS: u64 = 60;

/// Bridges the MCP server (which runs in tokio tasks under axum) and the
/// Tauri command layer (which runs the user's Approve/Reject click). The
/// MCP server inserts a oneshot sender keyed by request id, emits a
/// `mcp:approval:request` event, and awaits the receiver. The Tauri
/// command resolves the matching entry.
pub struct ApprovalRegistry {
    pending: Mutex<HashMap<Uuid, oneshot::Sender<bool>>>,
    approved: Mutex<HashMap<String, ApprovedToken>>,
    app: AppHandle,
}

#[derive(Debug, Clone, Copy)]
struct ApprovedToken {
    expires_at: Instant,
}

impl ApprovalRegistry {
    pub fn new(app: AppHandle) -> Arc<Self> {
        Arc::new(Self {
            pending: Mutex::new(HashMap::new()),
            approved: Mutex::new(HashMap::new()),
            app,
        })
    }

    /// Open a new approval request and await the user's decision. Returns
    /// the issued scoped token on approve, or an error string on reject /
    /// timeout / cancellation. The summary is sent verbatim to the UI.
    async fn request(&self, summary: &str) -> Result<String, String> {
        let id = Uuid::new_v4();
        let (tx, rx) = oneshot::channel::<bool>();
        self.pending.lock().unwrap().insert(id, tx);

        let _ = self.app.emit(
            "mcp:approval:request",
            serde_json::json!({
                "id": id.to_string(),
                "summary": summary,
                "ttlSeconds": APPROVAL_REQUEST_TIMEOUT_SECS,
            }),
        );

        let outcome =
            tokio::time::timeout(Duration::from_secs(APPROVAL_REQUEST_TIMEOUT_SECS), rx).await;

        // Always remove the pending entry — by this point the receiver is
        // either complete, cancelled, or its sender has been dropped.
        self.pending.lock().unwrap().remove(&id);

        let approved = match outcome {
            Ok(Ok(v)) => v,
            Ok(Err(_)) => return Err("approval request was cancelled".into()),
            Err(_) => {
                let _ = self.app.emit(
                    "mcp:approval:cancelled",
                    serde_json::json!({ "id": id.to_string() }),
                );
                return Err("approval timed out".into());
            }
        };

        if !approved {
            return Err("rejected by user".into());
        }

        let token = format!("appr_{}", Uuid::new_v4().simple());
        self.approved.lock().unwrap().insert(
            token.clone(),
            ApprovedToken {
                expires_at: Instant::now() + Duration::from_secs(SCOPED_TOKEN_TTL_SECS),
            },
        );
        Ok(token)
    }

    /// Called from the Tauri `mcp_approval_resolve` command. Returns an
    /// error if the request id doesn't match a pending entry (e.g. it
    /// already timed out, or the user double-clicked).
    pub fn resolve(&self, id: Uuid, approve: bool) -> Result<(), String> {
        let tx = self
            .pending
            .lock()
            .unwrap()
            .remove(&id)
            .ok_or_else(|| "no such pending approval".to_string())?;
        // If send fails the receiver is gone (already cleaned up). Either
        // way the outcome is the same: no waiter to notify.
        let _ = tx.send(approve);
        Ok(())
    }

    /// Validate that `token` is approved and unexpired. Used by mutating
    /// tools as a precondition. Multi-use within the TTL window — Phase
    /// 2.1 will tighten this with per-step signatures.
    fn validate(&self, token: &str) -> Result<(), String> {
        let mut approved = self.approved.lock().unwrap();
        // Drop expired entries opportunistically.
        let now = Instant::now();
        approved.retain(|_, t| t.expires_at > now);

        let entry = approved
            .get(token)
            .ok_or_else(|| "invalid or expired scoped_token".to_string())?;
        if now > entry.expires_at {
            return Err("scoped_token expired".into());
        }
        Ok(())
    }
}

// ---------- JSON-RPC envelope ----------

#[derive(Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Serialize)]
struct JsonRpcSuccess {
    jsonrpc: &'static str,
    id: Value,
    result: Value,
}

#[derive(Serialize)]
struct JsonRpcErrorEnvelope {
    jsonrpc: &'static str,
    id: Value,
    error: JsonRpcErrorBody,
}

#[derive(Serialize)]
struct JsonRpcErrorBody {
    code: i32,
    message: String,
}

// JSON-RPC error codes per spec.
const ERR_INVALID_REQUEST: i32 = -32600;
const ERR_METHOD_NOT_FOUND: i32 = -32601;
const ERR_INVALID_PARAMS: i32 = -32602;
const ERR_INTERNAL: i32 = -32603;

// ---------- HTTP handler ----------

async fn handle(
    State(state): State<McpState>,
    headers: HeaderMap,
    Json(req): Json<JsonRpcRequest>,
) -> axum::response::Response {
    // Bearer auth — required even on loopback so unrelated local processes
    // (browsers, other apps) can't drive the server by guessing the port.
    let auth_ok = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(|t| constant_time_eq(t.as_bytes(), state.bearer_token.as_bytes()))
        .unwrap_or(false);
    if !auth_ok {
        return (StatusCode::UNAUTHORIZED, "missing or invalid bearer token").into_response();
    }

    // DNS-rebinding protection: when an Origin header is present, only
    // localhost / 127.0.0.1 are allowed. CLIs speaking direct HTTP usually
    // omit Origin, so a missing header is fine.
    if let Some(origin) = headers.get("origin").and_then(|v| v.to_str().ok()) {
        let ok = origin.starts_with("http://localhost")
            || origin.starts_with("https://localhost")
            || origin.starts_with("http://127.0.0.1")
            || origin.starts_with("https://127.0.0.1");
        if !ok {
            return (StatusCode::FORBIDDEN, "origin not allowed").into_response();
        }
    }

    if req.jsonrpc != "2.0" {
        return rpc_error(Value::Null, ERR_INVALID_REQUEST, "expected jsonrpc 2.0");
    }

    let is_notification = req.id.is_none();
    let id = req.id.clone().unwrap_or(Value::Null);

    if is_notification {
        // Any client notification (e.g. `notifications/initialized`) is
        // accepted without dispatching — none currently change server state.
        return StatusCode::ACCEPTED.into_response();
    }

    let outcome = match req.method.as_str() {
        "initialize" => Ok(json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {
                "tools": { "listChanged": false }
            },
            "serverInfo": { "name": SERVER_NAME, "version": SERVER_VERSION }
        })),
        "tools/list" => Ok(json!({ "tools": tool_descriptors() })),
        "tools/call" => call_tool(&state, &req.params).await,
        other => Err((ERR_METHOD_NOT_FOUND, format!("method not found: {other}"))),
    };

    match outcome {
        Ok(result) => Json(JsonRpcSuccess {
            jsonrpc: "2.0",
            id,
            result,
        })
        .into_response(),
        Err((code, message)) => rpc_error(id, code, &message),
    }
}

fn rpc_error(id: Value, code: i32, message: &str) -> axum::response::Response {
    Json(JsonRpcErrorEnvelope {
        jsonrpc: "2.0",
        id,
        error: JsonRpcErrorBody {
            code,
            message: message.to_string(),
        },
    })
    .into_response()
}

// ---------- Tools ----------

fn tool_descriptors() -> Vec<Value> {
    vec![
        json!({
            "name": "atlas_list_projects",
            "description": "List indexed projects from the Atlas SQLite index. \
                            Returns id, name, path, language, branch, dirty/ahead/behind, \
                            and pinned/archived flags. Read-only.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "include_archived": {
                        "type": "boolean",
                        "description": "Include archived projects. Defaults to true.",
                        "default": true
                    }
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "atlas_get_project",
            "description": "Look up one project by id. Returns the same shape as \
                            atlas_list_projects entries, or null if no project matches.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Project id." }
                },
                "required": ["id"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "atlas_search_projects",
            "description": "Ranked FTS search across project name, path, and tags. \
                            Useful for resolving a fuzzy user reference (e.g. \"the notes app\") \
                            into a concrete project id before calling other tools.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Free-text search term." }
                },
                "required": ["query"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "atlas_list_recents",
            "description": "Most recently opened projects, newest first.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of recents to return.",
                        "minimum": 1,
                        "maximum": 100,
                        "default": 10
                    }
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "atlas_list_sessions",
            "description": "List AI CLI sessions discovered for a given project across all \
                            enabled providers (Claude Code, Codex, OpenCode). Newest first. \
                            Optionally filter to a single provider id.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project_id": { "type": "string", "description": "Project id." },
                    "provider": {
                        "type": "string",
                        "description": "Optional provider id filter (e.g. \"claude\", \"codex\")."
                    }
                },
                "required": ["project_id"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "atlas_list_scripts",
            "description": "Scripts (Taskfile entries, npm-style runs) registered for a project.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project_id": { "type": "string", "description": "Project id." }
                },
                "required": ["project_id"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "approval_request_plan",
            "description": "Ask the user to approve a batch of upcoming mutating actions. \
                            Pass a clear human-readable summary describing every step (e.g. \
                            \"Pin 'atlas' and 'notaty', archive 'old-experiment', then run \
                            'lint' on each\"). Returns a scoped_token valid for 60 seconds; \
                            pass it as the `scoped_token` argument to mutating tools. \
                            On reject or 120s timeout this tool errors — re-plan and ask again.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "summary": {
                        "type": "string",
                        "description": "Plain-English summary of what will happen. Shown verbatim to the user."
                    }
                },
                "required": ["summary"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "atlas_pin_project",
            "description": "Pin or unpin a project in the sidebar. \
                            MUTATING — requires a scoped_token from approval_request_plan.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Project id." },
                    "pinned": { "type": "boolean", "description": "True to pin, false to unpin." },
                    "scoped_token": {
                        "type": "string",
                        "description": "Approval token from approval_request_plan."
                    }
                },
                "required": ["id", "pinned", "scoped_token"],
                "additionalProperties": false
            }
        }),
    ]
}

async fn call_tool(state: &McpState, params: &Value) -> Result<Value, (i32, String)> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or((ERR_INVALID_PARAMS, "missing tool name".into()))?;
    let args = params.get("arguments").cloned().unwrap_or(json!({}));

    match name {
        "atlas_list_projects" => list_projects(&state.db, &args).await,
        "atlas_get_project" => get_project(&state.db, &args).await,
        "atlas_search_projects" => search_projects(&state.db, &args).await,
        "atlas_list_recents" => list_recents(&state.db, &args).await,
        "atlas_list_sessions" => list_sessions(state, &args).await,
        "atlas_list_scripts" => list_scripts(&state.db, &args).await,
        "approval_request_plan" => request_plan(state, &args).await,
        "atlas_pin_project" => pin_project(state, &args).await,
        other => Err((ERR_METHOD_NOT_FOUND, format!("unknown tool: {other}"))),
    }
}

// Each tool that returns plain JSON-encoded data goes through this helper
// so the wire format (`content: [{type: text, text: "<json>"}]`) lives in
// one place.
fn ok_text<T: Serialize>(value: &T) -> Result<Value, (i32, String)> {
    let text = serde_json::to_string_pretty(value)
        .map_err(|e| (ERR_INTERNAL, format!("serialize tool result: {e}")))?;
    Ok(json!({
        "content": [{ "type": "text", "text": text }],
        "isError": false
    }))
}

fn require_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, (i32, String)> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or((ERR_INVALID_PARAMS, format!("missing {key}")))
}

async fn list_projects(db: &Db, args: &Value) -> Result<Value, (i32, String)> {
    let include_archived = args
        .get("include_archived")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let filter = ProjectFilter {
        include_archived,
        ..ProjectFilter::default()
    };
    let projects = db
        .list_projects(filter)
        .await
        .map_err(|e| (ERR_INTERNAL, format!("list_projects failed: {e}")))?;
    ok_text(&projects)
}

async fn get_project(db: &Db, args: &Value) -> Result<Value, (i32, String)> {
    let id = require_str(args, "id")?;
    let project = db
        .get_project(id)
        .await
        .map_err(|e| (ERR_INTERNAL, format!("get_project failed: {e}")))?;
    ok_text(&project)
}

async fn search_projects(db: &Db, args: &Value) -> Result<Value, (i32, String)> {
    let query = require_str(args, "query")?;
    let projects = db
        .search_projects(query)
        .await
        .map_err(|e| (ERR_INTERNAL, format!("search_projects failed: {e}")))?;
    ok_text(&projects)
}

async fn list_recents(db: &Db, args: &Value) -> Result<Value, (i32, String)> {
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(10)
        .clamp(1, 100) as u32;
    let projects = db
        .recents_list(limit)
        .await
        .map_err(|e| (ERR_INTERNAL, format!("recents_list failed: {e}")))?;
    ok_text(&projects)
}

async fn list_sessions(state: &McpState, args: &Value) -> Result<Value, (i32, String)> {
    let project_id = require_str(args, "project_id")?;
    let provider_filter = args
        .get("provider")
        .and_then(Value::as_str)
        .map(String::from);

    let project = state
        .db
        .get_project(project_id)
        .await
        .map_err(|e| (ERR_INTERNAL, format!("get_project failed: {e}")))?
        .ok_or((
            ERR_INVALID_PARAMS,
            format!("project not found: {project_id}"),
        ))?;

    let providers_settings = crate::storage::settings::load(&state.app_data_dir)
        .await
        .map(|s| s.providers)
        .unwrap_or_default();

    let path = PathBuf::from(project.path);
    let mgr = Arc::clone(&state.sessions);
    // SessionsManager::list_for_project is synchronous filesystem I/O across
    // multiple provider databases — push it to a blocking thread so it can't
    // stall the server's runtime.
    let sessions = tauri::async_runtime::spawn_blocking(move || {
        mgr.list_for_project(&path, &providers_settings, provider_filter.as_deref())
    })
    .await
    .map_err(|e| (ERR_INTERNAL, format!("join blocking: {e}")))?
    .map_err(|e| (ERR_INTERNAL, format!("list_for_project failed: {e}")))?;
    ok_text(&sessions)
}

async fn list_scripts(db: &Db, args: &Value) -> Result<Value, (i32, String)> {
    let project_id = require_str(args, "project_id")?;
    let scripts = db
        .scripts_list(project_id)
        .await
        .map_err(|e| (ERR_INTERNAL, format!("scripts_list failed: {e}")))?;
    ok_text(&scripts)
}

async fn request_plan(state: &McpState, args: &Value) -> Result<Value, (i32, String)> {
    let summary = require_str(args, "summary")?;
    let token = state
        .approvals
        .request(summary)
        .await
        .map_err(|e| (ERR_INTERNAL, e))?;
    Ok(json!({
        "content": [{
            "type": "text",
            "text": format!(
                "APPROVED. Use scoped_token={token} on mutating tools. \
                 Valid for {SCOPED_TOKEN_TTL_SECS} seconds."
            )
        }],
        "structuredContent": {
            "scopedToken": token,
            "ttlSeconds": SCOPED_TOKEN_TTL_SECS
        },
        "isError": false
    }))
}

async fn pin_project(state: &McpState, args: &Value) -> Result<Value, (i32, String)> {
    let scoped_token = require_str(args, "scoped_token")?;
    state
        .approvals
        .validate(scoped_token)
        .map_err(|e| (ERR_INVALID_PARAMS, e))?;

    let id = require_str(args, "id")?;
    let pinned = args
        .get("pinned")
        .and_then(Value::as_bool)
        .ok_or((ERR_INVALID_PARAMS, "missing pinned (boolean)".into()))?;

    state
        .db
        .pin_project(id, pinned)
        .await
        .map_err(|e| (ERR_INTERNAL, format!("pin_project failed: {e}")))?;

    // Fire the same event the in-app `projects_pin` command would have, so
    // the React cache merges the change without waiting for a restart.
    let _ = crate::events::emit_project_updated(&state.app, id, json!({ "pinned": pinned }));

    ok_text(&json!({ "id": id, "pinned": pinned }))
}

/// Constant-time byte comparison so an attacker on the loopback can't
/// distinguish wrong-from-the-first-byte tokens from wrong-at-the-end ones.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}
