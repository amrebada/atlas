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
//! When more tools land this file will split into `protocol.rs`,
//! `server.rs`, and `tools/`. One file is fine for one tool.

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

const PROTOCOL_VERSION: &str = "2024-11-05";
const SERVER_NAME: &str = "atlas-mcp";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone)]
struct McpState {
    db: Db,
    bearer_token: String,
}

/// Start the MCP server on a background task if the user has opted in via
/// environment variables. Silent no-op otherwise.
///
/// Required: `ATLAS_MCP_ENABLED=1`, `ATLAS_MCP_TOKEN=<non-empty>`.
/// Optional: `ATLAS_MCP_PORT` (default `8765`).
pub fn maybe_spawn(db: Db) {
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
    // localhost / 127.0.0.1 are allowed. Tools speaking direct HTTP from
    // CLIs typically omit Origin, so a missing header is fine.
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

    // Notifications never produce a response body; just ack.
    if is_notification {
        // We accept any notification (e.g. `notifications/initialized`)
        // without dispatching — none of them currently change server state.
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
        "tools/call" => call_tool(&state.db, &req.params).await,
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
    vec![json!({
        "name": "atlas_list_projects",
        "description": "List indexed projects from the Atlas SQLite index. \
                        Returns id, name, path, language, branch, dirty/ahead/behind counts, \
                        and pinned/archived flags. Read-only.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "include_archived": {
                    "type": "boolean",
                    "description": "Include archived projects in the result. Defaults to true.",
                    "default": true
                }
            },
            "additionalProperties": false
        }
    })]
}

async fn call_tool(db: &Db, params: &Value) -> Result<Value, (i32, String)> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or((ERR_INVALID_PARAMS, "missing tool name".into()))?;
    let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

    match name {
        "atlas_list_projects" => list_projects(db, &arguments).await,
        other => Err((ERR_METHOD_NOT_FOUND, format!("unknown tool: {other}"))),
    }
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
    let text = serde_json::to_string_pretty(&projects)
        .map_err(|e| (ERR_INTERNAL, format!("serialize projects: {e}")))?;
    Ok(json!({
        "content": [{ "type": "text", "text": text }],
        "isError": false
    }))
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
