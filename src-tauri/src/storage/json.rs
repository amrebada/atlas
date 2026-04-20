//! Atomic per-project JSON file API.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{de::DeserializeOwned, Serialize};

/// Atomically write `data` to `path` via a sibling temp file + rename.
pub fn write_atomic(path: &Path, data: &[u8]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }

    // Sibling tempfile in the same dir → same filesystem → atomic rename.
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow::anyhow!("destination path missing a file name"))?;
    let tmp_name = format!(".{}.tmp", file_name);
    let tmp_path = path
        .parent()
        .map(|p| p.join(&tmp_name))
        .unwrap_or_else(|| Path::new(&tmp_name).to_path_buf());

    // Scope the file so the fd closes before rename on Windows.
    let write_result: anyhow::Result<()> = (|| {
        let mut f = fs::File::create(&tmp_path)?;
        f.write_all(data)?;
        f.sync_all()?;
        Ok(())
    })();

    if let Err(e) = write_result {
        // Best-effort cleanup.
        let _ = fs::remove_file(&tmp_path);
        return Err(e);
    }

    fs::rename(&tmp_path, path)?;
    Ok(())
}

/// Serialize `value` and write it to `path` atomically.
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> anyhow::Result<()> {
    let mut data = serde_json::to_vec_pretty(value)?;
    // Trailing newline keeps `git diff` stable and matches POSIX text
    if !data.ends_with(b"\n") {
        data.push(b'\n');
    }
    write_atomic(path, &data)
}

/// Read and parse JSON at `path`.
pub fn read_json<T: DeserializeOwned>(path: &Path) -> anyhow::Result<Option<T>> {
    match fs::read(path) {
        Ok(bytes) => {
            let value = serde_json::from_slice(&bytes)
                .map_err(|e| anyhow::anyhow!("parse {}: {e}", path.display()))?;
            Ok(Some(value))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(anyhow::anyhow!("read {}: {e}", path.display())),
    }
}

/// Compute the canonical `<project>/.atlas/<name>.json` path.
pub fn atlas_file(project_path: &Path, name: &str) -> PathBuf {
    project_path.join(".atlas").join(format!("{name}.json"))
}

/// Compute the canonical `<project>/.atlas/notes/<id>.json` path used by
pub fn atlas_note_file(project_path: &Path, note_id: &str) -> PathBuf {
    project_path
        .join(".atlas")
        .join("notes")
        .join(format!("{note_id}.json"))
}

/// Ensure `<project>/.atlas/` exists. Idempotent.
pub fn ensure_atlas_dir(project_path: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(project_path.join(".atlas"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use std::env;

    fn unique_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        env::temp_dir().join(format!("atlas-json-{tag}-{}-{nanos}", std::process::id()))
    }

    #[test]
    fn write_atomic_roundtrip() -> anyhow::Result<()> {
        let dir = unique_dir("atomic");
        let target = dir.join("sub").join("file.json");
        write_atomic(&target, br#"{"ok":true}"#)?;
        let got = fs::read_to_string(&target)?;
        assert_eq!(got, r#"{"ok":true}"#);
        fs::remove_dir_all(&dir).ok();
        Ok(())
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct Sample {
        name: String,
        n: i64,
        opts: Vec<String>,
    }

    #[test]
    fn write_then_read_json_roundtrip() -> anyhow::Result<()> {
        let dir = unique_dir("rw");
        let path = dir.join(".atlas").join("sample.json");
        let value = Sample {
            name: "atlas".into(),
            n: 42,
            opts: vec!["a".into(), "b".into()],
        };
        write_json(&path, &value)?;

        // File on disk is pretty + trailing newline.
        let raw = fs::read_to_string(&path)?;
        assert!(
            raw.ends_with('\n'),
            "expected trailing newline, got {raw:?}"
        );
        assert!(raw.contains("\n  "), "expected pretty-print indent");

        let parsed: Sample = read_json(&path)?.expect("file exists");
        assert_eq!(parsed, value);
        fs::remove_dir_all(&dir).ok();
        Ok(())
    }

    #[test]
    fn read_json_returns_none_for_missing_file() -> anyhow::Result<()> {
        let dir = unique_dir("missing");
        fs::create_dir_all(&dir)?;
        let path = dir.join("nope.json");
        let got: Option<Sample> = read_json(&path)?;
        assert!(got.is_none());
        fs::remove_dir_all(&dir).ok();
        Ok(())
    }

    #[test]
    fn atomic_rename_leaves_no_tmp_file_on_success() -> anyhow::Result<()> {
        // A successful write must clean up its temp sibling - i.e. the
        let dir = unique_dir("notmp");
        let path = dir.join("foo.json");
        write_json(
            &path,
            &Sample {
                name: "ok".into(),
                n: 1,
                opts: vec![],
            },
        )?;

        // No `.foo.json.tmp` left behind.
        let tmp = dir.join(".foo.json.tmp");
        assert!(!tmp.exists(), "stale temp file at {}", tmp.display());
        assert!(path.exists(), "destination missing at {}", path.display());

        // Sanity: the file is parseable JSON, not half-written.
        let _: Sample = read_json(&path)?.expect("present");
        fs::remove_dir_all(&dir).ok();
        Ok(())
    }

    #[test]
    fn write_json_overwrites_existing() -> anyhow::Result<()> {
        let dir = unique_dir("overwrite");
        let path = dir.join("a.json");
        write_json(
            &path,
            &Sample {
                name: "v1".into(),
                n: 1,
                opts: vec![],
            },
        )?;
        write_json(
            &path,
            &Sample {
                name: "v2".into(),
                n: 2,
                opts: vec![],
            },
        )?;
        let got: Sample = read_json(&path)?.expect("present");
        assert_eq!(got.name, "v2");
        assert_eq!(got.n, 2);
        fs::remove_dir_all(&dir).ok();
        Ok(())
    }

    #[test]
    fn atlas_file_paths_compose_correctly() {
        let project = Path::new("/tmp/proj");
        assert_eq!(
            atlas_file(project, "todos"),
            Path::new("/tmp/proj/.atlas/todos.json")
        );
        assert_eq!(
            atlas_file(project, "scripts"),
            Path::new("/tmp/proj/.atlas/scripts.json")
        );
        assert_eq!(
            atlas_note_file(project, "abc-123"),
            Path::new("/tmp/proj/.atlas/notes/abc-123.json")
        );
    }

    #[test]
    fn ensure_atlas_dir_is_idempotent() -> anyhow::Result<()> {
        let dir = unique_dir("ensure");
        fs::create_dir_all(&dir)?;
        ensure_atlas_dir(&dir)?;
        // Calling twice doesn't error.
        ensure_atlas_dir(&dir)?;
        assert!(dir.join(".atlas").is_dir());
        fs::remove_dir_all(&dir).ok();
        Ok(())
    }
}
