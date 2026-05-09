//! Embedded Model Context Protocol server.
//!
//! Phase 1 of the remote-control feature: an HTTP+JSON-RPC endpoint on
//! loopback that exposes a small set of read-only Atlas tools to a local
//! AI CLI (Claude Code, Codex, etc.). Off by default — opted in via
//! `ATLAS_MCP_ENABLED=1`. A bearer token (`ATLAS_MCP_TOKEN`) is required
//! so other local processes can't reach it just by guessing the port.
//!
//! Wire format: a single POST `/mcp` handler that dispatches JSON-RPC 2.0
//! messages. Streaming SSE responses aren't needed yet — every method we
//! support returns synchronously.
//!
//! When mutating tools land (Phase 2) this file will split into
//! `protocol.rs`, `server.rs`, and `tools/`.

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
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

const PROTOCOL_VERSION: &str = "2024-11-05";
const SERVER_NAME: &str = "atlas-mcp";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone)]
struct McpState {
    db: Db,
    sessions: Arc<SessionsManager>,
    app_data_dir: PathBuf,
    bearer_token: String,
}

/// Start the MCP server on a background task if the user has opted in via
/// environment variables. Silent no-op otherwise.
///
/// Required: `ATLAS_MCP_ENABLED=1`, `ATLAS_MCP_TOKEN=<non-empty>`.
/// Optional: `ATLAS_MCP_PORT` (default `8765`).
pub fn maybe_spawn(db: Db, sessions: Arc<SessionsManager>, app_data_dir: PathBuf) {
    if std::env::var("ATLAS_MCP_ENABLED").as_deref() != Ok("1") {
        return;
    }
    let port: u16 = std::env::var("ATLAS_MCP_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8765);
    let token = match std::env::var("ATLAS_MCP_TOKEN") {
        Ok(t) if !t.is_empty() => t,
        _ => {
            tracing::warn!(
                "ATLAS_MCP_ENABLED=1 but ATLAS_MCP_TOKEN is missing or empty; \
                 refusing to start MCP server without auth"
            );
            return;
        }
    };

    let state = McpState {
        db,
        sessions,
        app_data_dir,
        bearer_token: token,
    };
    let addr = SocketAddr::from(([127, 0, 0, 1], port));

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
