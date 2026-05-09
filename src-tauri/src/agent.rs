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
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};
use tokio_tungstenite::tungstenite::{
    client::IntoClientRequest,
    protocol::{frame::coding::CloseCode, CloseFrame},
    Message,
};

pub(crate) mod keys {
    //! Persistent ed25519 keypair for device pairing.
    //!
    //! On first read the keypair is generated via OsRng and stored at
    //! `$APP_DATA/atlas/agent_key` (32 raw bytes, 0600 perms on Unix).
    //! Subsequent reads load the same key, so the device id stays stable
    //! across Atlas restarts — pairing on the mobile side persists.
    //!
    //! The private bytes never leave the host. Only the public key + a
    //! short fingerprint are exposed (via the `agent_pairing_info`
    //! command), which is what the mobile app QR-scans to register.

    use std::fs;
    use std::io::Write;
    use std::path::Path;

    use ed25519_dalek::{SigningKey, VerifyingKey};
    use rand::rngs::OsRng;

    const KEY_FILE: &str = "agent_key";

    pub fn load_or_generate(app_data_dir: &Path) -> anyhow::Result<SigningKey> {
        let path = app_data_dir.join(KEY_FILE);
        if let Ok(bytes) = fs::read(&path) {
            if bytes.len() == 32 {
                let arr: [u8; 32] = bytes.as_slice().try_into().unwrap();
                return Ok(SigningKey::from_bytes(&arr));
            }
            tracing::warn!(
                path = %path.display(),
                len = bytes.len(),
                "agent_key has unexpected size; regenerating"
            );
        }
        fs::create_dir_all(app_data_dir)?;
        let mut csprng = OsRng;
        let key = SigningKey::generate(&mut csprng);

        let mut opts = fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&path)?;
        f.write_all(&key.to_bytes())?;
        tracing::info!(path = %path.display(), "agent keypair generated");
        Ok(key)
    }

    /// Short, displayable fingerprint — the first 8 bytes of the public
    /// key, hex-encoded. Stable across restarts for the same key.
    pub fn fingerprint(verifying: &VerifyingKey) -> String {
        verifying.as_bytes()[..8]
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect()
    }

    pub fn public_key_hex(verifying: &VerifyingKey) -> String {
        verifying
            .as_bytes()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect()
    }
}

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
    /// and reject unknown / unauthorized devices.
    Hello {
        agent_version: &'a str,
        os: &'a str,
        device_id: &'a str,
        public_key: &'a str,
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
        json!({ "state": "connected", "url": url }),
    );

    // Resolve the device identity and announce ourselves. The relay sees
    // public_key, not the bearer token — so a real production relay can
    // verify signed messages from this agent against the pubkey it has
    // on file from the pairing step.
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|e| anyhow::anyhow!("resolve app_data_dir: {e}"))?
        .join("atlas");
    let signing = keys::load_or_generate(&app_data)?;
    let verifying = signing.verifying_key();
    let device_id = keys::fingerprint(&verifying);
    let public_key = keys::public_key_hex(&verifying);

    let hello = OutboundMessage::Hello {
        agent_version: env!("CARGO_PKG_VERSION"),
        os: std::env::consts::OS,
        device_id: &device_id,
        public_key: &public_key,
    };
    let hello_json = serde_json::to_string(&hello)?;
    ws.send(Message::Text(hello_json.into())).await?;

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()?;

    while let Some(msg) = ws.next().await {
        match msg? {
            Message::Text(text) => {
                let s = text.as_str();
                tracing::info!(text = %s, "agent received text message");
                let _ = app.emit("agent:message", json!({ "text": s }));
                if let Some(reply) = handle_text(app, &http, s).await {
                    if let Err(e) = ws.send(Message::Text(reply.into())).await {
                        tracing::warn!(error = %e, "agent: send reply failed");
                        break;
                    }
                }
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

/// Try to decode an inbound text frame as an envelope and produce a reply.
/// Returns `None` for envelopes that don't need a reply (unknown types,
/// malformed JSON, etc — those are already logged via the emit above).
async fn handle_text(app: &AppHandle, http: &reqwest::Client, text: &str) -> Option<String> {
    let envelope: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, "agent: ignoring non-JSON message");
            return None;
        }
    };

    let envelope_type = envelope.get("type").and_then(Value::as_str);
    if envelope_type != Some("rpc") {
        // Future types: "ping", "approval", etc. For now non-rpc envelopes
        // are silently dropped — the UI already saw them via agent:message.
        return None;
    }

    let env_id = envelope.get("id").cloned().unwrap_or(Value::Null);
    let rpc_request = match envelope.get("request") {
        Some(r) => r.clone(),
        None => {
            return Some(error_reply(
                &env_id,
                "envelope missing `request` field (the JSON-RPC payload)",
            ));
        }
    };

    // Resolve the loopback MCP endpoint from settings on every call. Reading
    // it on every message is cheap and means a settings change between
    // requests is picked up immediately.
    let app_data = match app.path().app_data_dir() {
        Ok(p) => p.join("atlas"),
        Err(e) => {
            return Some(error_reply(&env_id, &format!("resolve app_data_dir: {e}")));
        }
    };
    let settings = match crate::storage::settings::load(&app_data).await {
        Ok(s) => s,
        Err(e) => {
            return Some(error_reply(&env_id, &format!("load settings: {e}")));
        }
    };
    let mcp = settings.advanced.mcp;
    if !mcp.enabled || mcp.token.is_empty() {
        return Some(error_reply(
            &env_id,
            "MCP server is not enabled — toggle on in Settings → Advanced and restart Atlas",
        ));
    }

    let url = format!("http://127.0.0.1:{}/mcp", mcp.port);
    let response = match http
        .post(&url)
        .header("Authorization", format!("Bearer {}", mcp.token))
        .json(&rpc_request)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return Some(error_reply(&env_id, &format!("MCP HTTP call failed: {e}"))),
    };

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Some(error_reply(
            &env_id,
            &format!("MCP returned {status}: {body}"),
        ));
    }
    let body: Value = match response.json().await {
        Ok(v) => v,
        Err(e) => {
            return Some(error_reply(
                &env_id,
                &format!("MCP non-JSON response: {e}"),
            ));
        }
    };

    let reply = json!({
        "type": "rpc",
        "id": env_id,
        "response": body,
    });
    serde_json::to_string(&reply).ok()
}

fn error_reply(env_id: &Value, message: &str) -> String {
    serde_json::to_string(&json!({
        "type": "rpc",
        "id": env_id,
        "response": {
            "jsonrpc": "2.0",
            "id": null,
            "error": { "code": -32603, "message": message }
        }
    }))
    .unwrap_or_else(|_| "{}".into())
}
