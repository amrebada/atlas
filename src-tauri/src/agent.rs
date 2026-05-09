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

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ed25519_dalek::{Signer, SigningKey};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, EventId, Listener, Manager};
use tokio_tungstenite::tungstenite::{
    client::IntoClientRequest,
    protocol::{frame::coding::CloseCode, CloseFrame},
    Message,
};
use uuid::Uuid;

/// Atlas events the agent forwards to the relay as ambient notifications.
/// `project:updated` and `git:status` are coalesced upstream by the
/// watcher (debounce + 2s coalesce window per repo) so we don't have to
/// rate-limit here. Discovery is one-shot per repo.
///
/// Excluded: `terminal:data:*` (too high frequency, not user-actionable),
/// `discovery:progress` (internal), `toast` (already shown in-app).
const WATCHER_BRIDGE_EVENTS: &[&str] = &[
    "project:updated",
    "project:discovered",
    "project:removed",
    "git:status",
];

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

/// Add `nonce`, `signed_at`, and `sig` fields to a JSON object envelope.
///
/// The signature covers the canonical JSON serialization of the envelope
/// after `nonce` and `signed_at` are added but before `sig` itself —
/// `serde_json::Map` is a BTreeMap by default, so keys are alphabetical
/// and the bytes are reproducible across platforms.
///
/// Phase 4.1: this is the foundation for the production relay verifying
/// every agent → relay message against the device's registered public
/// key (no more static bearer trust). The relay stub doesn't verify yet,
/// but the wire format is now in its final shape.
fn sign_and_serialize(signing: &SigningKey, mut value: Value) -> anyhow::Result<String> {
    let map = match &mut value {
        Value::Object(m) => m,
        _ => return Err(anyhow::anyhow!("envelope must be a JSON object")),
    };
    let nonce = Uuid::new_v4().to_string();
    let signed_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    map.insert("nonce".into(), json!(nonce));
    map.insert("signed_at".into(), json!(signed_at));

    let canonical = serde_json::to_vec(&value)?;
    let sig = signing.sign(&canonical);
    let sig_hex: String = sig
        .to_bytes()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect();
    if let Value::Object(map) = &mut value {
        map.insert("sig".into(), json!(sig_hex));
    }
    Ok(serde_json::to_string(&value)?)
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

    let hello_json = sign_and_serialize(
        &signing,
        json!({
            "type": "hello",
            "agent_version": env!("CARGO_PKG_VERSION"),
            "os": std::env::consts::OS,
            "device_id": device_id,
            "public_key": public_key,
        }),
    )?;
    ws.send(Message::Text(hello_json.into())).await?;

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()?;

    // Phase 8: bridge Atlas's Tauri event bus to the relay. Listeners
    // run on Tauri's event thread and push (event_name, payload_json)
    // tuples through an unbounded channel — no awaiting in the handler
    // means we never block the event loop. The async task below
    // `tokio::select!`s the channel against inbound WS frames.
    let (events_tx, mut events_rx) =
        tokio::sync::mpsc::unbounded_channel::<(String, String)>();
    let listener_ids: Vec<EventId> = WATCHER_BRIDGE_EVENTS
        .iter()
        .map(|name| {
            let tx = events_tx.clone();
            let event_name = name.to_string();
            app.listen(*name, move |event| {
                let _ = tx.send((event_name.clone(), event.payload().to_string()));
            })
        })
        .collect();

    let result = run_session(app, &mut ws, &http, &signing, &mut events_rx).await;

    // Always clean up listeners — without this they accumulate across
    // reconnects and we'd leak a slot per disconnect.
    for id in listener_ids {
        app.unlisten(id);
    }
    result
}

/// The connected-session loop. Multiplexes inbound RPC frames against
/// outbound watcher events, both signed via the device key on the way
/// out.
async fn run_session(
    app: &AppHandle,
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    http: &reqwest::Client,
    signing: &SigningKey,
    events_rx: &mut tokio::sync::mpsc::UnboundedReceiver<(String, String)>,
) -> anyhow::Result<()> {
    loop {
        tokio::select! {
            msg = ws.next() => {
                let Some(msg) = msg else { return Ok(()); };
                match msg? {
                    Message::Text(text) => {
                        let s = text.as_str();
                        tracing::info!(text = %s, "agent received text message");
                        let _ = app.emit("agent:message", json!({ "text": s }));
                        if let Some(reply) = handle_text(app, http, signing, s).await {
                            if let Err(e) = ws.send(Message::Text(reply.into())).await {
                                tracing::warn!(error = %e, "agent: send reply failed");
                                return Ok(());
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
                        return Ok(());
                    }
                    Message::Frame(_) => {}
                }
            }
            Some((event_name, payload_json)) = events_rx.recv() => {
                // Tauri emits payloads pre-serialized. Re-parse so the
                // relay sees a structured object instead of a string-of-JSON;
                // fall back to raw string if parse fails (defensive — every
                // emitter we forward uses serde_json so this should never trip).
                let payload: Value = serde_json::from_str(&payload_json)
                    .unwrap_or(Value::String(payload_json));
                let env_json = match sign_and_serialize(
                    signing,
                    json!({
                        "type": "event",
                        "event": event_name,
                        "payload": payload,
                    }),
                ) {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!(error = %e, "agent: sign event envelope failed");
                        continue;
                    }
                };
                if let Err(e) = ws.send(Message::Text(env_json.into())).await {
                    tracing::warn!(error = %e, "agent: forward event failed");
                    return Ok(());
                }
            }
        }
    }
}

/// Try to decode an inbound text frame as an envelope and produce a reply.
/// All replies go through `sign_and_serialize` so they carry nonce +
/// signed_at + sig before leaving the agent.
async fn handle_text(
    app: &AppHandle,
    http: &reqwest::Client,
    signing: &SigningKey,
    text: &str,
) -> Option<String> {
    let envelope: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, "agent: ignoring non-JSON message");
            return None;
        }
    };

    let envelope_type = envelope.get("type").and_then(Value::as_str);
    if envelope_type != Some("rpc") {
        return None;
    }

    let env_id = envelope.get("id").cloned().unwrap_or(Value::Null);
    let rpc_request = match envelope.get("request") {
        Some(r) => r.clone(),
        None => {
            return Some(error_reply(
                signing,
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
            return Some(error_reply(
                signing,
                &env_id,
                &format!("resolve app_data_dir: {e}"),
            ));
        }
    };
    let settings = match crate::storage::settings::load(&app_data).await {
        Ok(s) => s,
        Err(e) => {
            return Some(error_reply(signing, &env_id, &format!("load settings: {e}")));
        }
    };
    let mcp = settings.advanced.mcp;
    if !mcp.enabled || mcp.token.is_empty() {
        return Some(error_reply(
            signing,
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
        Err(e) => {
            return Some(error_reply(
                signing,
                &env_id,
                &format!("MCP HTTP call failed: {e}"),
            ));
        }
    };

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Some(error_reply(
            signing,
            &env_id,
            &format!("MCP returned {status}: {body}"),
        ));
    }
    let body: Value = match response.json().await {
        Ok(v) => v,
        Err(e) => {
            return Some(error_reply(
                signing,
                &env_id,
                &format!("MCP non-JSON response: {e}"),
            ));
        }
    };

    sign_and_serialize(
        signing,
        json!({
            "type": "rpc",
            "id": env_id,
            "response": body,
        }),
    )
    .ok()
}

fn error_reply(signing: &SigningKey, env_id: &Value, message: &str) -> String {
    sign_and_serialize(
        signing,
        json!({
            "type": "rpc",
            "id": env_id,
            "response": {
                "jsonrpc": "2.0",
                "id": null,
                "error": { "code": -32603, "message": message }
            }
        }),
    )
    .unwrap_or_else(|_| "{}".into())
}
