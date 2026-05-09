//! Tauri commands for the atlas-agent — pairing info, signed-envelope
//! generation for QR display, etc.

use serde::Serialize;
use serde_json::json;
use tauri::State;
use ts_rs::TS;

use crate::storage::AppContext;

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct PairingInfo {
    /// Short, displayable fingerprint (first 8 bytes of pubkey, hex).
    pub device_id: String,
    /// Full ed25519 public key, hex-encoded (64 chars).
    pub public_key: String,
    /// Default relay URL — what the QR code suggests to the mobile app.
    pub default_relay_url: String,
}

#[tauri::command]
pub fn agent_pairing_info(ctx: State<'_, AppContext>) -> Result<PairingInfo, String> {
    let key = crate::agent::keys::load_or_generate(&ctx.app_data_dir)
        .map_err(|e| format!("agent keys: {e}"))?;
    let verifying = key.verifying_key();
    Ok(PairingInfo {
        device_id: crate::agent::keys::fingerprint(&verifying),
        public_key: crate::agent::keys::public_key_hex(&verifying),
        default_relay_url: "ws://localhost:9000/agent".into(),
    })
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct PairEnvelope {
    /// Where the mobile should POST the envelope (`<relay_base>/pair`).
    /// Derived from the agent settings — production deployments override
    /// this from Settings → Atlas Agent.
    pub relay_base_url: String,
    /// JSON string of the signed pair envelope. Mobile sends this
    /// verbatim as the request body. Re-encoding would break the
    /// signature (canonical-JSON byte equality matters).
    pub envelope_json: String,
    /// Convenience copy of the device id from inside the envelope, so
    /// the UI can show it without re-parsing.
    pub device_id: String,
}

/// Build a freshly-signed pair envelope. The envelope embeds a fresh
/// nonce and signed_at — both make the resulting QR replay-resistant
/// (relay rejects stale signed_at and remembers used nonces).
///
/// The frontend should call this each time it shows the pair QR; the
/// envelope is good for ~60 seconds (relay's freshness window) before
/// the relay refuses it.
#[tauri::command]
pub fn agent_pair_envelope(ctx: State<'_, AppContext>) -> Result<PairEnvelope, String> {
    let signing = crate::agent::keys::load_or_generate(&ctx.app_data_dir)
        .map_err(|e| format!("agent keys: {e}"))?;
    let verifying = signing.verifying_key();
    let device_id = crate::agent::keys::fingerprint(&verifying);
    let public_key = crate::agent::keys::public_key_hex(&verifying);

    let envelope_json = crate::agent::sign_and_serialize(
        &signing,
        json!({
            "type": "pair",
            "device_id": device_id,
            "public_key": public_key,
        }),
    )
    .map_err(|e| format!("sign pair envelope: {e}"))?;

    // The pair endpoint sits next to /agent on the same relay host.
    // For now we strip /agent off the configured WS URL and swap the
    // scheme to http(s) — the same host serves both. Production
    // deployments may want a separate config for HTTPS pair URL; defer.
    let relay_base_url = derive_pair_base("ws://localhost:9000/agent");

    Ok(PairEnvelope {
        relay_base_url,
        envelope_json,
        device_id,
    })
}

/// Convert the agent's WebSocket URL into the matching HTTP(S) base.
///   ws://host:port/agent   → http://host:port
///   wss://host:port/agent  → https://host:port
fn derive_pair_base(ws_url: &str) -> String {
    let (scheme, rest) = if let Some(r) = ws_url.strip_prefix("wss://") {
        ("https", r)
    } else if let Some(r) = ws_url.strip_prefix("ws://") {
        ("http", r)
    } else {
        return ws_url.to_string();
    };
    let host = rest.split('/').next().unwrap_or(rest);
    format!("{scheme}://{host}")
}
