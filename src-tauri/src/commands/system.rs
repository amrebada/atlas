//! System-level commands. Backs the sidebar "home volume" row.

use serde::{Deserialize, Serialize};
use sysinfo::Disks;
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../src/types/rust.ts", rename_all = "camelCase")]
pub struct SystemDiskUsage {
    #[ts(type = "number")]
    pub total_bytes: u64,
    #[ts(type = "number")]
    pub free_bytes: u64,
    #[ts(type = "number")]
    pub used_bytes: u64,
    pub total: String,
    pub free: String,
    pub used: String,
    pub mount_point: String,
    #[ts(type = "number")]
    pub pct_used: f32,
}

/// Report free/used/total on the volume that hosts the user's home dir.
#[tauri::command]
pub async fn system_disk_usage() -> Result<SystemDiskUsage, String> {
    let home = dirs_home();
    let disks = Disks::new_with_refreshed_list();

    let chosen = disks
        .iter()
        .filter(|d| home.as_deref().is_none_or(|h| h.starts_with(d.mount_point())))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .or_else(|| disks.iter().next());

    let Some(d) = chosen else {
        return Err("no disks reported by sysinfo".into());
    };

    let total = d.total_space();
    let free = d.available_space();
    let used = total.saturating_sub(free);
    // Fraction 0..1 - the sidebar `DiskBar` scales this for the progress
    let pct_used = if total == 0 {
        0.0
    } else {
        (used as f64 / total as f64) as f32
    };

    Ok(SystemDiskUsage {
        total_bytes: total,
        free_bytes: free,
        used_bytes: used,
        total: crate::util::format_bytes(total),
        free: crate::util::format_bytes(free),
        used: crate::util::format_bytes(used),
        mount_point: d.mount_point().to_string_lossy().into_owned(),
        pct_used,
    })
}

fn dirs_home() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(std::path::PathBuf::from)
}
