//! Tauri commands for the atlas-agent — pairing info, etc.

use serde::Serialize;
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
