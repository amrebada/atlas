//! Launch template store - `<app_data>/launch_templates.json`.
//!
//! User-defined templates for starting new Claude Code sessions. Deliberately
//! a standalone app-level JSON file (not `settings.json`, not SQLite) so it
//! avoids the settings TOP_LEVEL_KEYS whitelist and shallow-merge races.

use std::path::{Path, PathBuf};

use tokio::sync::Mutex;

use crate::storage::json::{read_json, write_json};
use crate::storage::types::LaunchTemplate;

/// Filename inside `<app_data>/atlas/` - kept private so callers must go
/// through this module.
const LAUNCH_TEMPLATES_FILE: &str = "launch_templates.json";

/// Serializes the read-modify-write cycles below. `upsert` and `remove`
/// both re-read the whole file before writing it back; without the lock,
/// two concurrent IPC calls could interleave (each writes an array built
/// from a stale snapshot) and one mutation would silently be lost.
static WRITE_LOCK: Mutex<()> = Mutex::const_new(());

/// Resolve the on-disk `launch_templates.json` path for an app-data dir.
pub fn launch_templates_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(LAUNCH_TEMPLATES_FILE)
}

/// All launch templates, in stored order. A missing file is an empty list -
/// the file is only created by the first upsert.
pub async fn list(app_data_dir: &Path) -> anyhow::Result<Vec<LaunchTemplate>> {
    let path = launch_templates_path(app_data_dir);
    Ok(read_json::<Vec<LaunchTemplate>>(&path)?.unwrap_or_default())
}

/// Insert or replace a launch template by id. Ids are client-generated;
/// empty id or label is rejected.
pub async fn upsert(app_data_dir: &Path, template: LaunchTemplate) -> anyhow::Result<()> {
    if template.id.trim().is_empty() {
        anyhow::bail!("launch template id may not be empty");
    }
    if template.label.trim().is_empty() {
        anyhow::bail!("launch template label may not be empty");
    }

    let _guard = WRITE_LOCK.lock().await;
    let mut all = list(app_data_dir).await?;
    if let Some(slot) = all.iter_mut().find(|t| t.id == template.id) {
        *slot = template;
    } else {
        all.push(template);
    }

    write_json(&launch_templates_path(app_data_dir), &all)?;
    Ok(())
}

/// Remove a launch template by id. Unknown ids are a no-op (idempotent).
pub async fn remove(app_data_dir: &Path, id: &str) -> anyhow::Result<()> {
    let _guard = WRITE_LOCK.lock().await;
    let mut all = list(app_data_dir).await?;
    let before = all.len();
    all.retain(|t| t.id != id);
    if all.len() != before {
        write_json(&launch_templates_path(app_data_dir), &all)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::types::LaunchTemplateVar;
    use std::env;

    fn unique_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        env::temp_dir().join(format!(
            "atlas-launch-templates-{tag}-{}-{nanos}",
            std::process::id()
        ))
    }

    fn sample(id: &str, label: &str) -> LaunchTemplate {
        LaunchTemplate {
            id: id.into(),
            label: label.into(),
            hint: "fix a bug end-to-end".into(),
            color: "#3178c6".into(),
            body: "<p>Fix {{issue}} in {{area}}</p>".into(),
            variables: vec![
                LaunchTemplateVar {
                    key: "issue".into(),
                    label: "Issue".into(),
                    default: "GH-1".into(),
                    hint: "issue number".into(),
                    multiline: false,
                    options: vec![],
                    required: true,
                },
                LaunchTemplateVar {
                    key: "area".into(),
                    label: "Area".into(),
                    default: String::new(),
                    hint: String::new(),
                    multiline: true,
                    options: vec!["frontend".into(), "backend".into()],
                    required: false,
                },
            ],
            created_at: "2026-07-21T00:00:00Z".into(),
            updated_at: "2026-07-21T00:00:00Z".into(),
        }
    }

    #[tokio::test]
    async fn list_is_empty_on_fresh_dir() -> anyhow::Result<()> {
        let dir = unique_dir("fresh");
        std::fs::create_dir_all(&dir)?;

        let all = list(&dir).await?;
        assert!(all.is_empty(), "expected empty list, got {all:?}");
        // Listing must not create the file.
        assert!(!launch_templates_path(&dir).exists());

        std::fs::remove_dir_all(&dir).ok();
        Ok(())
    }

    #[tokio::test]
    async fn upsert_then_list_roundtrips_all_fields() -> anyhow::Result<()> {
        let dir = unique_dir("roundtrip");
        std::fs::create_dir_all(&dir)?;

        let t = sample("tpl-1", "Bug fix");
        upsert(&dir, t.clone()).await?;

        let all = list(&dir).await?;
        assert_eq!(all.len(), 1);
        let got = &all[0];
        assert_eq!(got.id, t.id);
        assert_eq!(got.label, t.label);
        assert_eq!(got.hint, t.hint);
        assert_eq!(got.color, t.color);
        assert_eq!(got.body, t.body);
        assert_eq!(got.created_at, t.created_at);
        assert_eq!(got.updated_at, t.updated_at);

        // Variables survive with every field intact.
        assert_eq!(got.variables.len(), 2);
        let issue = &got.variables[0];
        assert_eq!(issue.key, "issue");
        assert_eq!(issue.label, "Issue");
        assert_eq!(issue.default, "GH-1");
        assert_eq!(issue.hint, "issue number");
        assert!(!issue.multiline);
        assert!(issue.options.is_empty());
        assert!(issue.required);
        let area = &got.variables[1];
        assert_eq!(area.key, "area");
        assert!(area.multiline);
        assert_eq!(area.options, vec!["frontend", "backend"]);
        assert!(!area.required);

        std::fs::remove_dir_all(&dir).ok();
        Ok(())
    }

    #[tokio::test]
    async fn upsert_replaces_by_id() -> anyhow::Result<()> {
        let dir = unique_dir("replace");
        std::fs::create_dir_all(&dir)?;

        upsert(&dir, sample("tpl-1", "Bug fix")).await?;
        upsert(&dir, sample("tpl-2", "Refactor")).await?;

        let mut updated = sample("tpl-1", "Bug fix (v2)");
        updated.variables.clear();
        upsert(&dir, updated).await?;

        let all = list(&dir).await?;
        assert_eq!(all.len(), 2, "replace must not append");
        // Order is preserved: tpl-1 stays first.
        assert_eq!(all[0].id, "tpl-1");
        assert_eq!(all[0].label, "Bug fix (v2)");
        assert!(all[0].variables.is_empty());
        assert_eq!(all[1].id, "tpl-2");

        std::fs::remove_dir_all(&dir).ok();
        Ok(())
    }

    #[tokio::test]
    async fn upsert_rejects_empty_id_and_label() -> anyhow::Result<()> {
        let dir = unique_dir("reject");
        std::fs::create_dir_all(&dir)?;

        let err = upsert(&dir, sample("   ", "ok label")).await;
        assert!(err.is_err(), "blank id must be rejected");
        assert!(format!("{}", err.unwrap_err()).contains("id"));

        let err = upsert(&dir, sample("ok-id", "   ")).await;
        assert!(err.is_err(), "blank label must be rejected");
        assert!(format!("{}", err.unwrap_err()).contains("label"));

        // Nothing was persisted.
        assert!(list(&dir).await?.is_empty());

        std::fs::remove_dir_all(&dir).ok();
        Ok(())
    }

    #[tokio::test]
    async fn remove_deletes_by_id() -> anyhow::Result<()> {
        let dir = unique_dir("remove");
        std::fs::create_dir_all(&dir)?;

        upsert(&dir, sample("tpl-1", "Bug fix")).await?;
        upsert(&dir, sample("tpl-2", "Refactor")).await?;

        remove(&dir, "tpl-1").await?;

        let all = list(&dir).await?;
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, "tpl-2");

        std::fs::remove_dir_all(&dir).ok();
        Ok(())
    }

    #[tokio::test]
    async fn remove_unknown_id_is_ok() -> anyhow::Result<()> {
        let dir = unique_dir("remove-unknown");
        std::fs::create_dir_all(&dir)?;

        // On a fresh dir (no file yet).
        remove(&dir, "ghost").await?;

        // And with existing content, which must be left untouched.
        upsert(&dir, sample("tpl-1", "Bug fix")).await?;
        remove(&dir, "ghost").await?;
        assert_eq!(list(&dir).await?.len(), 1);

        std::fs::remove_dir_all(&dir).ok();
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_mutations_do_not_lose_updates() -> anyhow::Result<()> {
        let dir = unique_dir("concurrent");
        std::fs::create_dir_all(&dir)?;

        // Without WRITE_LOCK each task could read a stale snapshot and
        // clobber another task's write; with it, every upsert survives.
        let mut handles = Vec::new();
        for i in 0..16 {
            let dir = dir.clone();
            handles.push(tokio::spawn(async move {
                upsert(&dir, sample(&format!("tpl-{i}"), &format!("T{i}"))).await
            }));
        }
        for h in handles {
            h.await??;
        }
        assert_eq!(list(&dir).await?.len(), 16);

        std::fs::remove_dir_all(&dir).ok();
        Ok(())
    }

    #[tokio::test]
    async fn file_on_disk_is_valid_json_array() -> anyhow::Result<()> {
        let dir = unique_dir("raw-json");
        std::fs::create_dir_all(&dir)?;

        upsert(&dir, sample("tpl-1", "Bug fix")).await?;

        let raw = std::fs::read_to_string(launch_templates_path(&dir))?;
        let value: serde_json::Value = serde_json::from_str(&raw)?;
        let arr = value.as_array().expect("top-level JSON array");
        assert_eq!(arr.len(), 1);
        // camelCase field names on disk (serde rename_all).
        assert_eq!(
            arr[0].get("createdAt").and_then(|v| v.as_str()),
            Some("2026-07-21T00:00:00Z")
        );
        assert!(arr[0].get("variables").and_then(|v| v.as_array()).is_some());

        std::fs::remove_dir_all(&dir).ok();
        Ok(())
    }
}
