//! Atlas Agent — outbound WebSocket client that pipes commands from a
//! relay backend (the eventual mobile control plane) into Atlas's
//! embedded MCP server.
//!
//! Phase 4.0a (this commit): connection + handshake + reconnect loop
//! only. Inbound messages are logged and emitted as `agent:message`
//! Tauri events; nothing routes them to the MCP server yet — that's
//! Phase 4.0b. Outbound responses come in 4.0c.
//!
//! Off by default. Configured via env vars during the prototype phase
//! (Settings UI later, alongside the MCP toggle):
//!   ATLAS_AGENT_ENABLED=1
//!   ATLAS_AGENT_URL=ws://localhost:9000/agent   (default if unset)
//!   ATLAS_AGENT_TOKEN=<bearer>                  (required)
//!
//! Wire format: JSON envelopes over text frames. The agent sends a `hello`
//! on connect and otherwise just echoes inbound messages into the Tauri
//! event bus. Real envelope shape will be defined alongside the relay
//! design — Phase 4.0c.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio_tungstenite::tungstenite::{
    client::IntoClientRequest,
    protocol::{frame::coding::CloseCode, CloseFrame},
    Message,
};

const DEFAULT_RELAY_URL: &str = "ws://localhost:9000/agent";
const RECONNECT_MIN: Duration = Duration::from_secs(1);
const RECONNECT_MAX: Duration = Duration::from_secs(30);

/// Spawn the agent if `ATLAS_AGENT_ENABLED=1` and a token is set.
/// Silent no-op otherwise.
pub fn maybe_spawn(app: AppHandle) {
    if std::env::var("ATLAS_AGENT_ENABLED").as_deref() != Ok("1") {
        return;
    }
    let url = std::env::var("ATLAS_AGENT_URL").unwrap_or_else(|_| DEFAULT_RELAY_URL.into());
    let token = match std::env::var("ATLAS_AGENT_TOKEN") {
        Ok(t) if !t.is_empty() => t,
        _ => {
            tracing::warn!(
                "ATLAS_AGENT_ENABLED=1 but ATLAS_AGENT_TOKEN missing or empty; agent will not start"
            );
            return;
        }
    };

    tauri::async_runtime::spawn(async move {
        connect_loop(app, url, token).await;
    });
}

/// Reconnecting loop. On error or clean disconnect, backs off and
/// retries. The exponential backoff caps at 30s so a flapping relay
/// doesn't lead to tight reconnect spins.
async fn connect_loop(app: AppHandle, url: String, token: String) {
    let mut backoff = RECONNECT_MIN;
    loop {
        let _ = app.emit(
            "agent:status",
            serde_json::json!({ "state": "connecting", "url": url }),
        );
        match connect_once(&app, &url, &token).await {
            Ok(()) => {
                tracing::info!("agent disconnected cleanly");
                backoff = RECONNECT_MIN;
            }
            Err(e) => {
                tracing::warn!(error = %e, "agent connection failed; will retry");
            }
        }
        let _ = app.emit(
            "agent:status",
            serde_json::json!({
                "state": "reconnecting",
                "delaySeconds": backoff.as_secs()
            }),
        );
        tokio::time::sleep(backoff).await;
        // Exponential backoff up to RECONNECT_MAX.
        backoff = (backoff.saturating_mul(2)).min(RECONNECT_MAX);
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum OutboundMessage<'a> {
    /// Sent immediately on connect so the relay can register this agent
    /// (and reject duplicates / unknown devices).
    Hello {
        agent_version: &'a str,
        os: &'a str,
        // Future: device_id (ed25519 public key fingerprint), capabilities.
    },
}

async fn connect_once(app: &AppHandle, url: &str, token: &str) -> anyhow::Result<()> {
    let mut req = url
        .into_client_request()
        .map_err(|e| anyhow::anyhow!("parse url {url}: {e}"))?;
    req.headers_mut().insert(
        "authorization",
        format!("Bearer {token}").parse()
            .map_err(|e| anyhow::anyhow!("invalid bearer token: {e}"))?,
    );
    req.headers_mut().insert(
        "user-agent",
        format!("atlas-agent/{}", env!("CARGO_PKG_VERSION"))
            .parse()
            .unwrap(),
    );

    let (mut ws, response) = tokio_tungstenite::connect_async(req).await?;
    tracing::info!(status = %response.status(), "agent connected to relay");
    let _ = app.emit(
        "agent:status",
        serde_json::json!({ "state": "connected", "url": url }),
    );

    let hello = OutboundMessage::Hello {
        agent_version: env!("CARGO_PKG_VERSION"),
        os: std::env::consts::OS,
    };
    let hello_json = serde_json::to_string(&hello)?;
    ws.send(Message::Text(hello_json.into())).await?;

    while let Some(msg) = ws.next().await {
        match msg? {
            Message::Text(text) => {
                tracing::info!(text = %text, "agent received text message");
                let _ = app.emit(
                    "agent:message",
                    serde_json::json!({ "text": text.as_str() }),
                );
            }
            Message::Binary(bytes) => {
                tracing::info!(bytes = bytes.len(), "agent received binary frame");
            }
            Message::Ping(payload) => {
                ws.send(Message::Pong(payload)).await?;
            }
            Message::Pong(_) => {}
            Message::Close(frame) => {
                tracing::info!(?frame, "relay sent close");
                let _ = ws
                    .send(Message::Close(Some(CloseFrame {
                        code: CloseCode::Normal,
                        reason: "agent ack close".into(),
                    })))
                    .await;
                break;
            }
            Message::Frame(_) => {}
        }
    }
    Ok(())
}
