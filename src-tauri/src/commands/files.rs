//! File listing IPC for the Inspector → Files tab. Owned by **P3**.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use git2::{ErrorClass, ErrorCode, Repository};
use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};
use tauri::State;
use ts_rs::TS;

use crate::git;
use crate::storage::types::{FileKind, FileNode};
use crate::storage::Db;

/// Match the discovery deny-list; we never want to surface anything below
const DENY_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    ".venv",
    "venv",
    "dist",
    "build",
    ".next",
    ".cache",
];

const DEFAULT_DEPTH: usize = 4;

/// Diff payload used by the Inspector → Files tab when the user clicks a
/// changed file. We hand the renderer two raw strings and let the JS diff
/// viewer compute the visual diff.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct FileDiff {
    /// Path relative to the project root.
    pub path: String,
    /// Status char, mirroring `git::FileStatus::status` (`M`/`+`/`-`).
    pub status: String,
    /// File contents at HEAD. Empty string when the file is newly added.
    pub old_content: String,
    /// File contents in the working tree. Empty string when the file is
    /// deleted.
    pub new_content: String,
    /// True when either side looks binary (NUL byte in the first 8 KB).
    /// The UI uses this to hide the diff viewer and show a fallback note.
    pub is_binary: bool,
}

/// `files.list` - return either the changed-files tree or the full project
#[tauri::command]
pub async fn files_list(
    state: State<'_, Db>,
    project_id: String,
    changed_only: bool,
) -> Result<Vec<FileNode>, String> {
    let project = state
        .get_project(&project_id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())?
        .ok_or_else(|| format!("project not found: {project_id}"))?;
    let project_path = PathBuf::from(&project.path);

    // git2 / ignore::WalkBuilder are blocking. Run on the blocking pool so
    let result = if changed_only {
        let path = project_path.clone();
        tauri::async_runtime::spawn_blocking(move || changed_tree(&path))
            .await
            .map_err(|e| format!("join blocking: {e}"))?
    } else {
        let path = project_path.clone();
        tauri::async_runtime::spawn_blocking(move || full_tree(&path, DEFAULT_DEPTH))
            .await
            .map_err(|e| format!("join blocking: {e}"))?
    };

    result.map_err(|e| e.to_string())
}

/// Build the changed-only tree: every file with a git status, plus all of
fn changed_tree(project_path: &Path) -> anyhow::Result<Vec<FileNode>> {
    let statuses = git::file_statuses(project_path)?;
    if statuses.is_empty() {
        return Ok(Vec::new());
    }

    // Sorted set of every dir that needs to appear so files render under
    let mut dirs: BTreeSet<String> = BTreeSet::new();
    for s in &statuses {
        let path = Path::new(&s.path);
        let mut current = path.parent();
        while let Some(p) = current {
            if p.as_os_str().is_empty() {
                break;
            }
            dirs.insert(p.to_string_lossy().into_owned());
            current = p.parent();
        }
    }

    // Index the file rows by path for quick lookup.
    let mut files_by_path: BTreeMap<String, &git::FileStatus> = BTreeMap::new();
    for s in &statuses {
        files_by_path.insert(s.path.clone(), s);
    }

    // Combine dirs + files into one path-keyed BTreeMap so we get a stable
    let mut all_paths: BTreeSet<String> = BTreeSet::new();
    all_paths.extend(dirs.iter().cloned());
    all_paths.extend(files_by_path.keys().cloned());

    let mut out: Vec<FileNode> = Vec::with_capacity(all_paths.len());
    for path_str in &all_paths {
        let path = Path::new(path_str);
        let depth = path.components().count().saturating_sub(1) as i64;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(path_str)
            .to_string();

        if let Some(status) = files_by_path.get(path_str) {
            let delta = status.delta.map(|(adds, dels)| format_delta(adds, dels));
            out.push(FileNode {
                depth,
                name,
                path: path_str.clone(),
                kind: FileKind::File,
                status: Some(status.status.to_string()),
                delta,
            });
        } else if dirs.contains(path_str) {
            out.push(FileNode {
                depth,
                name,
                path: path_str.clone(),
                kind: FileKind::Dir,
                status: None,
                delta: None,
            });
        }
    }

    // BTreeSet is alphabetic; the React tree expects directory rows BEFORE
    out.sort_by(|a, b| a.path.cmp(&b.path));

    Ok(out)
}

/// Pretty `"+X -Y"` ‑ matches the prototype's delta chip. Skips a side
fn format_delta(adds: u32, dels: u32) -> String {
    match (adds, dels) {
        (0, 0) => String::new(),
        (a, 0) => format!("+{a}"),
        (0, d) => format!("−{d}"),
        (a, d) => format!("+{a} −{d}"),
    }
}

/// Walk the project tree up to `max_depth` levels deep, skipping the
fn full_tree(project_path: &Path, max_depth: usize) -> anyhow::Result<Vec<FileNode>> {
    if !project_path.exists() {
        anyhow::bail!("project path does not exist: {}", project_path.display());
    }

    let mut builder = WalkBuilder::new(project_path);
    builder
        .max_depth(Some(max_depth))
        .hidden(false)
        .git_ignore(false)
        .git_exclude(false)
        .git_global(false)
        .ignore(false)
        .parents(false)
        .follow_links(false)
        .filter_entry(|entry| {
            if let Some(name) = entry.file_name().to_str() {
                if DENY_DIRS.contains(&name) {
                    return false;
                }
            }
            true
        });

    let mut nodes = Vec::new();
    for result in builder.build() {
        let entry = match result {
            Ok(e) => e,
            Err(err) => {
                tracing::trace!(?err, "walk error in files_list");
                continue;
            }
        };

        // Skip the root itself - UI only wants children.
        let Ok(rel) = entry.path().strip_prefix(project_path) else {
            continue;
        };
        if rel.as_os_str().is_empty() {
            continue;
        }

        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let depth = rel.components().count().saturating_sub(1) as i64;
        let name = rel
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        nodes.push(FileNode {
            depth,
            name,
            path: rel.to_string_lossy().into_owned(),
            kind: if is_dir {
                FileKind::Dir
            } else {
                FileKind::File
            },
            status: None,
            delta: None,
        });
    }

    nodes.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(nodes)
}

/// `files.diff` - return the HEAD vs working-tree contents of a single file
/// in the project so the renderer can show a side-by-side / inline diff.
#[tauri::command]
pub async fn files_diff(
    state: State<'_, Db>,
    project_id: String,
    path: String,
) -> Result<FileDiff, String> {
    let project = state
        .get_project(&project_id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())?
        .ok_or_else(|| format!("project not found: {project_id}"))?;
    let project_path = PathBuf::from(&project.path);

    tauri::async_runtime::spawn_blocking(move || read_file_diff(&project_path, &path))
        .await
        .map_err(|e| format!("join blocking: {e}"))?
        .map_err(|e| e.to_string())
}

/// Build a [`FileDiff`] for a single relative path inside the repo. The
/// HEAD blob is read via libgit2; the working-tree side is read straight
/// from disk so untracked files surface naturally.
fn read_file_diff(project_path: &Path, rel_path: &str) -> anyhow::Result<FileDiff> {
    let rel = Path::new(rel_path);
    if rel.is_absolute()
        || rel
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        anyhow::bail!("invalid path: {rel_path}");
    }

    // ---- HEAD content ------------------------------------------------------
    let (old_content, head_existed) = match Repository::open(project_path) {
        Ok(repo) => match read_head_blob(&repo, rel_path) {
            Ok(Some(bytes)) => (bytes, true),
            Ok(None) => (Vec::new(), false),
            Err(_) => (Vec::new(), false),
        },
        Err(e) if e.code() == ErrorCode::NotFound && e.class() == ErrorClass::Repository => {
            (Vec::new(), false)
        }
        Err(_) => (Vec::new(), false),
    };

    // ---- working-tree content ---------------------------------------------
    let abs = project_path.join(rel);
    let (new_content, wt_existed) = match std::fs::read(&abs) {
        Ok(bytes) => (bytes, true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => (Vec::new(), false),
        Err(err) => anyhow::bail!("read {rel_path}: {err}"),
    };

    let status = if head_existed && !wt_existed {
        "-"
    } else if !head_existed && wt_existed {
        "+"
    } else {
        "M"
    };

    let is_binary = looks_binary(&old_content) || looks_binary(&new_content);

    Ok(FileDiff {
        path: rel_path.to_string(),
        status: status.to_string(),
        old_content: if is_binary {
            String::new()
        } else {
            String::from_utf8_lossy(&old_content).into_owned()
        },
        new_content: if is_binary {
            String::new()
        } else {
            String::from_utf8_lossy(&new_content).into_owned()
        },
        is_binary,
    })
}

/// Read the blob bytes for `rel_path` at the current `HEAD` commit. Returns
/// `Ok(None)` when the file does not exist in the HEAD tree (newly added)
/// or the repo is unborn.
fn read_head_blob(repo: &Repository, rel_path: &str) -> anyhow::Result<Option<Vec<u8>>> {
    let head = match repo.head() {
        Ok(h) => h,
        // Unborn branch (fresh repo) → no HEAD blob yet.
        Err(e) if e.code() == ErrorCode::UnbornBranch || e.code() == ErrorCode::NotFound => {
            return Ok(None);
        }
        Err(e) => return Err(e.into()),
    };
    let tree = head.peel_to_commit()?.tree()?;
    let entry = match tree.get_path(Path::new(rel_path)) {
        Ok(e) => e,
        Err(e) if e.code() == ErrorCode::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let object = entry.to_object(repo)?;
    let blob = match object.into_blob() {
        Ok(b) => b,
        Err(_) => return Ok(None),
    };
    Ok(Some(blob.content().to_vec()))
}

/// Heuristic: a file is binary if any of the first 8 KB is a NUL byte.
/// Matches the rule git itself uses for diff fallback messaging.
fn looks_binary(bytes: &[u8]) -> bool {
    let cap = bytes.len().min(8192);
    bytes[..cap].contains(&0u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_delta_collapses_zero_sides() {
        assert_eq!(format_delta(0, 0), "");
        assert_eq!(format_delta(3, 0), "+3");
        assert_eq!(format_delta(0, 5), "−5");
        assert_eq!(format_delta(2, 7), "+2 −7");
    }

    #[test]
    fn looks_binary_detects_nul() {
        assert!(!looks_binary(b"hello world\n"));
        assert!(looks_binary(b"hello\0world"));
        assert!(!looks_binary(b""));
    }
}
