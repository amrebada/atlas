//! Settings store - source of truth at `$APP_DATA/atlas/settings.json`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::storage::json::{read_json, write_json};
use crate::storage::templates::builtin_templates;
use crate::storage::types::{
    AdvancedSettings, CloneDepth, EditorsSettings, FullLiteral, GeneralSettings, GitPollInterval,
    GitSettings, Settings, Template, Theme,
};

/// Filename inside `<app_data>/atlas/` - kept private so callers must go
const SETTINGS_FILE: &str = "settings.json";

/// Whitelist of top-level keys the partial-update patch may touch. Adding
const TOP_LEVEL_KEYS: &[&str] = &[
    "general",
    "editors",
    "git",
    "watchers",
    "templates",
    "shortcuts",
    "advanced",
];

/// Resolve the on-disk `settings.json` path for an app-data dir.
pub fn settings_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(SETTINGS_FILE)
}

/// Default project location - `$HOME/code` if `$HOME` is set, else empty
fn default_project_location() -> String {
    match std::env::var("HOME") {
        Ok(h) if !h.is_empty() => PathBuf::from(h)
            .join("code")
            .to_string_lossy()
            .into_owned(),
        _ => String::new(),
    }
}

/// Keys match the action ids used across the app; values are platform
pub fn default_shortcuts() -> HashMap<String, String> {
    let mut m = HashMap::with_capacity(10);
    m.insert("palette".into(), "Mod+K".into());
    m.insert("new-project".into(), "Mod+N".into());
    m.insert("clone".into(), "Mod+Shift+N".into());
    m.insert("settings".into(), "Mod+,".into());
    m.insert("open-in-editor".into(), "Mod+E".into());
    m.insert("open-terminal".into(), "Ctrl+`".into());
    m.insert("max-terminal".into(), "Ctrl+Mod+F".into());
    m.insert("save".into(), "Mod+S".into());
    m.insert("slash-menu".into(), "/".into());
    m.insert("close".into(), "Escape".into());
    m
}

/// Materialize the default `Settings` value. Built-in templates are
pub fn default_settings() -> Settings {
    Settings {
        general: GeneralSettings {
            launch_at_login: true,
            menu_bar_agent: true,
            default_project_location: default_project_location(),
            theme: Theme::System,
        },
        editors: EditorsSettings {
            detected: Vec::new(),
            default_id: None,
        },
        git: GitSettings {
            poll_interval: GitPollInterval::ThirtySec,
            show_author: false,
            default_clone_depth: CloneDepth::Full(FullLiteral::Full),
            ssh_key: "~/.ssh/id_ed25519".into(),
        },
        watchers: Vec::new(),
        // User-added templates persist here; built-ins are injected by
        templates: Vec::new(),
        shortcuts: default_shortcuts(),
        advanced: AdvancedSettings {
            use_spotlight: false,
            crash_reports: true,
            shell: "/bin/zsh".into(),
            // Opt-in: user must toggle this on in Settings → Advanced
            crash_log: false,
        },
    }
}

/// Load `settings.json`, lazily creating it on first read.
pub async fn load(app_data_dir: &Path) -> anyhow::Result<Settings> {
    let path = settings_path(app_data_dir);

    // Race-free enough: read_json returns None only if the file is
    let user: Settings = match read_json::<Settings>(&path)? {
        Some(s) => s,
        None => {
            let defaults = default_settings();
            write_json(&path, &defaults)?;
            defaults
        }
    };

    Ok(with_builtin_templates(user))
}

/// Persist `settings` to disk, stripping built-in templates first so the
pub async fn save(app_data_dir: &Path, settings: &Settings) -> anyhow::Result<()> {
    let path = settings_path(app_data_dir);
    let stripped = without_builtin_templates(settings);
    write_json(&path, &stripped)?;
    Ok(())
}

/// Apply a shallow-merge patch and persist the result. Returns the newly
pub async fn apply_patch(
    app_data_dir: &Path,
    patch: Value,
) -> anyhow::Result<Settings> {
    let Value::Object(patch_map) = patch else {
        return Err(anyhow::anyhow!(
            "settings patch must be a JSON object, got {}",
            kind_of(&patch)
        ));
    };

    // Reject unknown top-level keys up front.
    for key in patch_map.keys() {
        if !TOP_LEVEL_KEYS.contains(&key.as_str()) {
            return Err(anyhow::anyhow!(
                "unknown settings key: {key} (expected one of {:?})",
                TOP_LEVEL_KEYS
            ));
        }
    }

    // Start from the persisted value (not `load` - load re-injects
    let current = load_stripped(app_data_dir).await?;

    // Round-trip through serde_json so we can apply the merge with the
    let mut as_value = serde_json::to_value(&current)?;
    if let Value::Object(ref mut obj) = as_value {
        for (k, v) in patch_map {
            obj.insert(k, v);
        }
    }

    let merged: Settings = serde_json::from_value(as_value).map_err(|e| {
        anyhow::anyhow!("invalid settings patch: {e}")
    })?;

    // Persist the stripped form; return the view-friendly form.
    save(app_data_dir, &merged).await?;
    Ok(with_builtin_templates(merged))
}

/// Load without re-attaching built-in templates. Used internally by
async fn load_stripped(app_data_dir: &Path) -> anyhow::Result<Settings> {
    let path = settings_path(app_data_dir);
    let user: Settings = match read_json::<Settings>(&path)? {
        Some(s) => s,
        None => {
            let d = default_settings();
            write_json(&path, &d)?;
            d
        }
    };
    Ok(user)
}

/// Return a `Settings` whose `templates` field is `builtins ++ user_only`.
fn with_builtin_templates(mut s: Settings) -> Settings {
    let builtins = builtin_templates();
    let builtin_ids: std::collections::HashSet<&str> =
        builtins.iter().map(|t| t.id.as_str()).collect();
    s.templates.retain(|t| !t.builtin && !builtin_ids.contains(t.id.as_str()));
    let mut all: Vec<Template> = builtins;
    all.extend(s.templates);
    s.templates = all;
    s
}

/// Inverse of `with_builtin_templates` - keep only user-added entries.
fn without_builtin_templates(s: &Settings) -> Settings {
    let mut copy = s.clone();
    copy.templates.retain(|t| !t.builtin);
    copy
}

/// Kind-of helper for the patch-shape error message. `serde_json::Value`
fn kind_of(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Internal helper - convenient in tests to assert "the JSON on disk
#[cfg(test)]
pub(crate) fn read_raw(app_data_dir: &Path) -> anyhow::Result<Option<Value>> {
    let path = settings_path(app_data_dir);
    read_json::<Value>(&path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn unique_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        env::temp_dir().join(format!(
            "atlas-settings-{tag}-{}-{nanos}",
            std::process::id()
        ))
    }

    #[tokio::test]
    async fn load_creates_defaults_on_first_read() -> anyhow::Result<()> {
        let dir = unique_dir("first-read");
        std::fs::create_dir_all(&dir)?;
        assert!(!settings_path(&dir).exists());

        let s = load(&dir).await?;

        // Default shape sanity - spot-check fields we own here.
        assert!(s.general.launch_at_login);
        assert!(s.general.menu_bar_agent);
        assert!(matches!(s.general.theme, Theme::System));
        assert!(matches!(s.git.poll_interval, GitPollInterval::ThirtySec));
        assert!(!s.git.show_author);
        assert_eq!(s.advanced.shell, "/bin/zsh");
        assert!(s.advanced.crash_reports);
        assert!(!s.advanced.use_spotlight);
        assert!(!s.shortcuts.is_empty());
        assert_eq!(s.shortcuts.get("palette").map(String::as_str), Some("Mod+K"));

        // File was created.
        assert!(settings_path(&dir).exists());

        // Built-ins present.
        assert!(s.templates.iter().any(|t| t.builtin && t.id == "node-ts"));

        std::fs::remove_dir_all(&dir).ok();
        Ok(())
    }

    #[tokio::test]
    async fn roundtrip_load_mutate_save_load() -> anyhow::Result<()> {
        let dir = unique_dir("roundtrip");
        std::fs::create_dir_all(&dir)?;

        let mut s = load(&dir).await?;
        s.general.theme = Theme::Dark;
        s.general.launch_at_login = false;
        s.advanced.shell = "/bin/bash".into();
        s.shortcuts.insert("custom".into(), "Mod+Shift+C".into());

        save(&dir, &s).await?;

        let reloaded = load(&dir).await?;
        assert!(matches!(reloaded.general.theme, Theme::Dark));
        assert!(!reloaded.general.launch_at_login);
        assert_eq!(reloaded.advanced.shell, "/bin/bash");
        assert_eq!(
            reloaded.shortcuts.get("custom").map(String::as_str),
            Some("Mod+Shift+C")
        );

        std::fs::remove_dir_all(&dir).ok();
        Ok(())
    }

    #[tokio::test]
    async fn save_does_not_persist_builtin_templates() -> anyhow::Result<()> {
        // `save` should strip built-ins so the on-disk JSON stays focused
        let dir = unique_dir("no-builtins-on-disk");
        std::fs::create_dir_all(&dir)?;

        let s = load(&dir).await?; // injects builtins
        assert!(s.templates.iter().any(|t| t.builtin));

        // save strips them on disk
        save(&dir, &s).await?;
        let raw = read_raw(&dir)?.expect("file present");
        let templates = raw
            .get("templates")
            .and_then(|t| t.as_array())
            .expect("templates array");
        assert!(
            templates.iter().all(|t| {
                !t.get("builtin")
                    .and_then(|b| b.as_bool())
                    .unwrap_or(true)
            }),
            "on-disk templates array should contain zero builtins, got {:?}",
            templates
        );

        // load re-attaches them.
        let s2 = load(&dir).await?;
        assert!(s2.templates.iter().any(|t| t.builtin && t.id == "node-ts"));

        std::fs::remove_dir_all(&dir).ok();
        Ok(())
    }

    #[tokio::test]
    async fn apply_patch_accepts_partial_update() -> anyhow::Result<()> {
        let dir = unique_dir("patch-partial");
        std::fs::create_dir_all(&dir)?;
        let _ = load(&dir).await?; // seed defaults

        let patch = json!({
            "general": {
                "launchAtLogin": false,
                "menuBarAgent": false,
                "defaultProjectLocation": "/tmp/custom",
                "theme": "dark"
            }
        });
        let merged = apply_patch(&dir, patch).await?;
        assert!(matches!(merged.general.theme, Theme::Dark));
        assert_eq!(merged.general.default_project_location, "/tmp/custom");
        assert!(!merged.general.launch_at_login);

        // Untouched branches are preserved.
        assert_eq!(merged.advanced.shell, "/bin/zsh");

        // Persisted.
        let loaded = load(&dir).await?;
        assert!(matches!(loaded.general.theme, Theme::Dark));

        std::fs::remove_dir_all(&dir).ok();
        Ok(())
    }

    #[tokio::test]
    async fn apply_patch_rejects_unknown_keys() -> anyhow::Result<()> {
        let dir = unique_dir("patch-unknown");
        std::fs::create_dir_all(&dir)?;
        let _ = load(&dir).await?;

        let patch = json!({ "nonsense": 1 });
        let err = apply_patch(&dir, patch).await;
        assert!(err.is_err(), "expected rejection for unknown top-level key");
        let msg = format!("{}", err.unwrap_err());
        assert!(msg.contains("unknown settings key"), "{msg}");

        std::fs::remove_dir_all(&dir).ok();
        Ok(())
    }

    #[tokio::test]
    async fn apply_patch_rejects_non_object_patch() -> anyhow::Result<()> {
        let dir = unique_dir("patch-nonobject");
        std::fs::create_dir_all(&dir)?;
        let _ = load(&dir).await?;

        let err = apply_patch(&dir, json!(42)).await;
        assert!(err.is_err());
        let err = apply_patch(&dir, json!("string")).await;
        assert!(err.is_err());
        let err = apply_patch(&dir, json!(null)).await;
        assert!(err.is_err());

        std::fs::remove_dir_all(&dir).ok();
        Ok(())
    }

    #[tokio::test]
    async fn apply_patch_validates_shape() -> anyhow::Result<()> {
        // A structurally-valid key with a garbage inner shape should
        let dir = unique_dir("patch-shape");
        std::fs::create_dir_all(&dir)?;
        let _ = load(&dir).await?;

        let bad = json!({
            "general": { "launchAtLogin": "not a bool" }
        });
        let err = apply_patch(&dir, bad).await;
        assert!(err.is_err(), "expected type-check failure");

        std::fs::remove_dir_all(&dir).ok();
        Ok(())
    }
}
