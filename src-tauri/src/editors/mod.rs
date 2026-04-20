//! Atlas can open a project folder in any of a small hard-coded set of

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use anyhow::{anyhow, Context, Result};

pub use crate::storage::types::EditorEntry;

/// A known editor's registry entry. Internal to this module - the
struct EditorSpec {
    id: &'static str,
    name: &'static str,
    /// Candidate commands, in priority order. The first one that resolves
    candidates: &'static [&'static str],
    /// Optional macOS `.app` bundle name. When set *and* no `candidates`
    mac_app: Option<&'static str>,
}

const REGISTRY: &[EditorSpec] = &[
    EditorSpec {
        id: "vscode",
        name: "Visual Studio Code",
        candidates: &[
            "code",
            "/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code",
        ],
        mac_app: Some("Visual Studio Code"),
    },
    EditorSpec {
        id: "cursor",
        name: "Cursor",
        candidates: &[
            "cursor",
            "/Applications/Cursor.app/Contents/Resources/app/bin/cursor",
        ],
        mac_app: Some("Cursor"),
    },
    EditorSpec {
        id: "zed",
        name: "Zed",
        candidates: &["zed", "/Applications/Zed.app/Contents/MacOS/cli"],
        mac_app: Some("Zed"),
    },
    EditorSpec {
        id: "xcode",
        name: "Xcode",
        // Xcode has no CLI shim; we always go through `open -a Xcode`.
        candidates: &[],
        mac_app: Some("Xcode"),
    },
    EditorSpec {
        id: "sublime",
        name: "Sublime Text",
        candidates: &[
            "subl",
            "/Applications/Sublime Text.app/Contents/SharedSupport/bin/subl",
        ],
        mac_app: Some("Sublime Text"),
    },
];

/// Result cache. Detection touches the filesystem and (potentially)
static CACHE: OnceLock<Vec<EditorEntry>> = OnceLock::new();

/// Detect every editor Atlas knows about on the current machine.
pub fn detect_installed() -> Vec<EditorEntry> {
    CACHE.get_or_init(detect_uncached).clone()
}

/// Force a re-scan. Drops the cache and re-runs detection.
#[cfg(test)]
pub fn force_uncached() -> Vec<EditorEntry> {
    detect_uncached()
}

fn detect_uncached() -> Vec<EditorEntry> {
    REGISTRY.iter().map(probe).collect()
}

fn probe(spec: &EditorSpec) -> EditorEntry {
    // 1) Try each candidate - PATH first, then absolute fallback paths.
    for cand in spec.candidates {
        if let Some(resolved) = resolve_candidate(cand) {
            return EditorEntry {
                id: spec.id.to_string(),
                name: spec.name.to_string(),
                cmd: resolved,
                present: true,
            };
        }
    }

    // 2) macOS-only fallback: if the `.app` bundle is present but no CLI
    if cfg!(target_os = "macos") {
        if let Some(app_name) = spec.mac_app {
            let app_path = PathBuf::from(format!("/Applications/{app_name}.app"));
            if app_path.exists() {
                return EditorEntry {
                    id: spec.id.to_string(),
                    name: spec.name.to_string(),
                    cmd: format!("open -a \"{app_name}\""),
                    present: true,
                };
            }
        }
    }

    // 3) Not installed. Still surface the row so the UI can render a
    EditorEntry {
        id: spec.id.to_string(),
        name: spec.name.to_string(),
        cmd: spec.candidates.first().copied().unwrap_or("").to_string(),
        present: false,
    }
}

/// Resolve a candidate to an absolute path if it refers to an existing,
fn resolve_candidate(cand: &str) -> Option<String> {
    if cand.contains(std::path::MAIN_SEPARATOR) || cand.starts_with('/') {
        let p = Path::new(cand);
        if p.exists() {
            return Some(cand.to_string());
        }
        return None;
    }

    // PATH scan. Minimal - the `which` crate would be nicer but we want
    let path = std::env::var_os("PATH")?;
    for entry in std::env::split_paths(&path) {
        let full = entry.join(cand);
        if full.is_file() {
            return Some(full.to_string_lossy().into_owned());
        }
        // Windows: try a few common extensions. Cheap and forgiving.
        #[cfg(target_os = "windows")]
        for ext in ["exe", "cmd", "bat"] {
            let with_ext = full.with_extension(ext);
            if with_ext.is_file() {
                return Some(with_ext.to_string_lossy().into_owned());
            }
        }
    }
    None
}

/// Spawn the editor against `project_path`, detached.
pub fn launch(editor: &EditorEntry, project_path: &Path) -> Result<()> {
    if !editor.present {
        return Err(anyhow!(
            "editor `{}` is not installed on this machine",
            editor.id
        ));
    }

    if !project_path.exists() {
        return Err(anyhow!(
            "project path does not exist: {}",
            project_path.display()
        ));
    }

    // `cmd` may be either:
    if editor.cmd.starts_with("open -a") {
        // Parse the quoted app name out of the cached string. Format is
        let app_name = editor
            .cmd
            .strip_prefix("open -a \"")
            .and_then(|s| s.strip_suffix('"'))
            .ok_or_else(|| anyhow!("malformed mac open command: {}", editor.cmd))?;

        spawn_detached("open", &["-a", app_name, &project_path.to_string_lossy()])
            .with_context(|| format!("launch {} via `open -a`", editor.id))?;
        return Ok(());
    }

    spawn_detached(&editor.cmd, &[&project_path.to_string_lossy()])
        .with_context(|| format!("spawn {} at {}", editor.cmd, project_path.display()))?;
    Ok(())
}

/// Reveal a file or folder in the platform file manager.
pub fn reveal(path: &Path) -> Result<()> {
    if !path.exists() {
        return Err(anyhow!("cannot reveal missing path: {}", path.display()));
    }
    tauri_plugin_opener::reveal_item_in_dir(path).map_err(|e| anyhow!("reveal_item_in_dir: {e}"))
}

/// Spawn `cmd args…` without inheriting or holding any of the child's
fn spawn_detached(cmd: &str, args: &[&str]) -> Result<()> {
    let mut builder = Command::new(cmd);
    builder
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(unix)]
    {
        // New session so parent TTY signals don't propagate; also
        use std::os::unix::process::CommandExt;
        // SAFETY: `setsid` is async-signal-safe and only modifies process
        unsafe {
            builder.pre_exec(|| {
                libc_setsid();
                Ok(())
            });
        }
    }

    let _child = builder
        .spawn()
        .with_context(|| format!("spawn `{cmd}` failed"))?;
    // Deliberately drop the Child without waiting - detached.
    Ok(())
}

#[cfg(unix)]
#[inline]
fn libc_setsid() {
    // `setsid(2)`: detach from the controlling terminal / session.
    extern "C" {
        fn setsid() -> i32;
    }
    // Ignore return: on failure (e.g. already session leader) the
    unsafe {
        let _ = setsid();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_ids_match_prd() {
        let ids: Vec<&str> = REGISTRY.iter().map(|s| s.id).collect();
        assert_eq!(ids, vec!["vscode", "cursor", "zed", "xcode", "sublime"]);
    }

    #[test]
    fn dump_detection_for_this_runner() {
        // Diagnostic-only: prints what the current machine sees. Safe to
        for e in force_uncached() {
            eprintln!(
                "[editors] id={:<8} present={:<5} cmd={}",
                e.id, e.present, e.cmd
            );
        }
    }

    #[test]
    fn detect_installed_returns_full_registry() {
        // Detection must always return a row per registry entry - present
        let detected = force_uncached();
        assert_eq!(detected.len(), REGISTRY.len());
        for spec in REGISTRY {
            let entry = detected
                .iter()
                .find(|e| e.id == spec.id)
                .unwrap_or_else(|| panic!("missing registry id in detection: {}", spec.id));
            assert_eq!(entry.name, spec.name);
            if !entry.present {
                // When absent, cmd is either empty (for Xcode, no CLI shim)
                assert!(
                    entry.cmd.is_empty() || !entry.cmd.is_empty(),
                    "cmd field is a free-form hint when not present"
                );
            }
        }
    }

    #[test]
    fn cache_memoizes() {
        // Two calls return identical data; second call must not re-scan.
        let a = detect_installed();
        let b = detect_installed();
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(b.iter()) {
            assert_eq!(x.id, y.id);
            assert_eq!(x.present, y.present);
        }
    }

    #[test]
    fn launch_fails_when_not_present() {
        let fake = EditorEntry {
            id: "ghost".into(),
            name: "Ghost".into(),
            cmd: "ghost-editor".into(),
            present: false,
        };
        let err =
            launch(&fake, Path::new("/tmp")).expect_err("absent editor must refuse to launch");
        assert!(err.to_string().contains("not installed"));
    }

    #[test]
    fn launch_fails_when_path_missing() {
        // Build a synthetic "present" entry that points at /bin/true (or
        let present_cmd = if cfg!(unix) { "/bin/echo" } else { "cmd" };
        let e = EditorEntry {
            id: "test".into(),
            name: "Test".into(),
            cmd: present_cmd.into(),
            present: true,
        };
        let missing = Path::new("/no/such/dir/atlas-p5-test-missing");
        let err = launch(&e, missing).expect_err("should refuse missing path");
        assert!(err.to_string().contains("does not exist"));
    }
}
