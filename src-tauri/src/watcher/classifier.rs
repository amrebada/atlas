//! Classifies a single debounced filesystem path into the watcher

use std::path::{Component, Path, PathBuf};

/// The category of work a changed path implies. The watcher manager routes
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventKind {
    /// A path inside `.git/` that affects branch / HEAD / refs. Trigger a
    GitMetadata { repo_root: PathBuf },
    /// A `.atlas/*.json` change - reindex that file and emit
    AtlasJson {
        repo_root: PathBuf,
        file_name: String,
    },
    /// consumer; we emit the classification today and log TODO).
    PackageJson { repo_root: PathBuf },
    /// A generic source-file mutation - coalesce into a dirty-bump emit.
    SourceFile { repo_root: PathBuf },
    /// A newly created directory (not yet classified as a repo). The
    NewDirectory { path: PathBuf },
    /// The event is outside all known watch roots or otherwise uninteresting.
    Ignored,
}

/// Deny-list segments we never cross. Matches `ignore::WalkBuilder`'s
pub const DENY_SEGMENTS: &[&str] = &[
    ".cache",
    ".next",
    ".venv",
    "build",
    "dist",
    "node_modules",
    "target",
];

/// Classify `path` given the set of currently watched roots.
pub fn classify(path: &Path, roots: &[PathBuf], is_dir: bool) -> EventKind {
    // Must live under a watch root; otherwise ignore defensively.
    if !roots.iter().any(|r| path.starts_with(r)) {
        return EventKind::Ignored;
    }

    // Deny-list - never process events inside `node_modules`, `target`, etc.
    if path
        .components()
        .any(|c| matches!(c, Component::Normal(n) if DENY_SEGMENTS.iter().any(|d| *d == n)))
    {
        return EventKind::Ignored;
    }

    // `.git/...` ⇒ git metadata. Walk up to find the directory that
    if let Some(repo_root) = containing_repo_via_dotgit(path) {
        return EventKind::GitMetadata { repo_root };
    }

    // `.atlas/*.json` - project-local state files.
    if let Some((repo_root, fname)) = atlas_json_context(path) {
        return EventKind::AtlasJson {
            repo_root,
            file_name: fname,
        };
    }

    // A `package.json` that sits at the top level of some ancestor repo.
    if path
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s == "package.json")
        .unwrap_or(false)
    {
        if let Some(repo_root) = path.parent() {
            return EventKind::PackageJson {
                repo_root: repo_root.to_path_buf(),
            };
        }
    }

    if is_dir {
        return EventKind::NewDirectory {
            path: path.to_path_buf(),
        };
    }

    // Fallback: a source-file-ish change. Attribute it to the nearest
    if let Some(repo_root) = path.parent() {
        return EventKind::SourceFile {
            repo_root: repo_root.to_path_buf(),
        };
    }

    EventKind::Ignored
}

/// If `path` has a `.git` segment, return the path that contains that
fn containing_repo_via_dotgit(path: &Path) -> Option<PathBuf> {
    let mut acc = PathBuf::new();
    let mut found_root = None;
    for comp in path.components() {
        match comp {
            Component::Normal(n) if n == ".git" => {
                found_root = Some(acc.clone());
                break;
            }
            other => acc.push(other.as_os_str()),
        }
    }
    found_root
}

/// Detect `<repo_root>/.atlas/<file>.json`. Returns `(repo_root, file_name)`.
fn atlas_json_context(path: &Path) -> Option<(PathBuf, String)> {
    let file_name = path.file_name()?.to_str()?.to_string();
    if !file_name.ends_with(".json") {
        return None;
    }
    let parent = path.parent()?;
    if parent.file_name()?.to_str()? != ".atlas" {
        return None;
    }
    let repo_root = parent.parent()?.to_path_buf();
    Some((repo_root, file_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roots() -> Vec<PathBuf> {
        vec![PathBuf::from("/u/code")]
    }

    #[test]
    fn outside_any_root_is_ignored() {
        let k = classify(Path::new("/tmp/foo.rs"), &roots(), false);
        assert_eq!(k, EventKind::Ignored);
    }

    #[test]
    fn node_modules_is_denied() {
        let k = classify(
            Path::new("/u/code/myrepo/node_modules/foo/index.js"),
            &roots(),
            false,
        );
        assert_eq!(k, EventKind::Ignored);
    }

    #[test]
    fn target_is_denied() {
        let k = classify(
            Path::new("/u/code/myrepo/target/debug/atlas"),
            &roots(),
            false,
        );
        assert_eq!(k, EventKind::Ignored);
    }

    #[test]
    fn dot_git_head_routes_to_git_metadata() {
        let k = classify(Path::new("/u/code/myrepo/.git/HEAD"), &roots(), false);
        assert_eq!(
            k,
            EventKind::GitMetadata {
                repo_root: PathBuf::from("/u/code/myrepo")
            }
        );
    }

    #[test]
    fn dot_git_refs_heads_routes_to_git_metadata() {
        let k = classify(
            Path::new("/u/code/myrepo/.git/refs/heads/main"),
            &roots(),
            false,
        );
        assert_eq!(
            k,
            EventKind::GitMetadata {
                repo_root: PathBuf::from("/u/code/myrepo")
            }
        );
    }

    #[test]
    fn atlas_json_routes_with_file_name() {
        let k = classify(
            Path::new("/u/code/myrepo/.atlas/todos.json"),
            &roots(),
            false,
        );
        assert_eq!(
            k,
            EventKind::AtlasJson {
                repo_root: PathBuf::from("/u/code/myrepo"),
                file_name: "todos.json".to_string(),
            }
        );
    }

    #[test]
    fn package_json_routes_to_package_json() {
        let k = classify(
            Path::new("/u/code/myrepo/package.json"),
            &roots(),
            false,
        );
        assert_eq!(
            k,
            EventKind::PackageJson {
                repo_root: PathBuf::from("/u/code/myrepo"),
            }
        );
    }

    #[test]
    fn new_dir_flagged_when_is_dir_true() {
        let k = classify(Path::new("/u/code/newproj"), &roots(), true);
        assert_eq!(
            k,
            EventKind::NewDirectory {
                path: PathBuf::from("/u/code/newproj")
            }
        );
    }

    #[test]
    fn source_file_falls_through() {
        let k = classify(Path::new("/u/code/myrepo/src/main.rs"), &roots(), false);
        assert_eq!(
            k,
            EventKind::SourceFile {
                repo_root: PathBuf::from("/u/code/myrepo/src")
            }
        );
    }
}
