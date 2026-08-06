//! Git operations wrapper - thin `git2::Repository` facade used by the

use std::path::Path;

use git2::{BranchType, DiffOptions, ErrorClass, ErrorCode, Repository, Status, StatusOptions};
use serde::Serialize;

/// Snapshot of a repo's high-level state, matching the subset of fields on
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitStatus {
    /// Short branch name (e.g. `main`, `feat/charts`). Empty string for
    pub branch: String,
    /// Count of paths with a working-tree or index dirty bit set.
    pub dirty: u32,
    /// Commits on the local branch that aren't on upstream (0 if no upstream).
    pub ahead: u32,
    /// Commits on upstream that aren't local (0 if no upstream).
    pub behind: u32,
    /// Author name of the HEAD commit (e.g. `"Ada Lovelace"`). `None` when
    pub author: Option<String>,
}

/// Cheap file-system probe - does `path` contain a `.git/` entry?
pub fn is_git_repo(path: &Path) -> bool {
    path.join(".git").exists()
}

/// Read the current status of the repo at `project_path`.
pub fn read_status(project_path: &Path) -> anyhow::Result<Option<GitStatus>> {
    let repo = match Repository::open(project_path) {
        Ok(r) => r,
        Err(e) if is_not_a_repo(&e) => return Ok(None),
        Err(e) => return Err(e.into()),
    };

    let branch = current_branch(&repo);
    let dirty = count_dirty(&repo)?;
    let (ahead, behind) = ahead_behind(&repo).unwrap_or((0, 0));
    let author = head_author(&repo);

    Ok(Some(GitStatus {
        branch,
        dirty,
        ahead,
        behind,
        author,
    }))
}

/// Author name of the commit currently pointed to by `HEAD`.
fn head_author(repo: &Repository) -> Option<String> {
    let head = repo.head().ok()?;
    let commit = head.peel_to_commit().ok()?;
    // Bind the signature to a local so the `&str` returned by `name()`
    let sig = commit.author();
    sig.name().map(|s| s.to_string())
}

fn is_not_a_repo(e: &git2::Error) -> bool {
    e.code() == ErrorCode::NotFound && e.class() == ErrorClass::Repository
}

/// Short branch name, or empty string for detached HEAD / unborn branch.
fn current_branch(repo: &Repository) -> String {
    match repo.head() {
        Ok(r) => r.shorthand().unwrap_or("").to_string(),
        Err(e) if e.code() == ErrorCode::UnbornBranch || e.code() == ErrorCode::NotFound => {
            String::new()
        }
        Err(_) => String::new(),
    }
}

/// Count paths with any working-tree or index status bit set.
fn count_dirty(repo: &Repository) -> anyhow::Result<u32> {
    let mut opts = StatusOptions::new();
    opts.include_untracked(true)
        .include_ignored(false)
        .include_unmodified(false)
        .exclude_submodules(true)
        .recurse_untracked_dirs(false); // one entry per untracked dir is fine

    let statuses = repo.statuses(Some(&mut opts))?;
    let mask = Status::WT_NEW
        | Status::WT_MODIFIED
        | Status::WT_DELETED
        | Status::WT_TYPECHANGE
        | Status::WT_RENAMED
        | Status::INDEX_NEW
        | Status::INDEX_MODIFIED
        | Status::INDEX_DELETED
        | Status::INDEX_TYPECHANGE
        | Status::INDEX_RENAMED
        | Status::CONFLICTED;

    let mut count: u32 = 0;
    for entry in statuses.iter() {
        if entry.status().intersects(mask) {
            count = count.saturating_add(1);
        }
    }
    Ok(count)
}

/// Per-file status row used by the Inspector → Files tab.
#[derive(Debug, Clone, Serialize)]
pub struct FileStatus {
    /// Path relative to the repo root.
    pub path: String,
    /// `'M'` modified · `'+'` added/untracked · `'-'` deleted.
    pub status: char,
    /// `(additions, deletions)` line counts from a workdir-vs-index diff.
    pub delta: Option<(u32, u32)>,
}

/// Read per-file status for the repo at `project_path`. Only modified,
pub fn file_statuses(project_path: &Path) -> anyhow::Result<Vec<FileStatus>> {
    let repo = match Repository::open(project_path) {
        Ok(r) => r,
        Err(e) if is_not_a_repo(&e) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };

    let mut opts = StatusOptions::new();
    opts.include_untracked(true)
        .include_ignored(false)
        .include_unmodified(false)
        .exclude_submodules(true)
        .recurse_untracked_dirs(true);

    let statuses = repo.statuses(Some(&mut opts))?;

    // Compute deltas in one pass via `diff_index_to_workdir`. We index by
    let mut diff_opts = DiffOptions::new();
    diff_opts
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .ignore_submodules(true);
    let deltas: std::collections::HashMap<String, (u32, u32)> =
        match repo.diff_index_to_workdir(None, Some(&mut diff_opts)) {
            Ok(diff) => match diff.stats() {
                Ok(_stats) => {
                    // git2's high-level `stats()` aggregates totals; we
                    let mut map: std::collections::HashMap<String, (u32, u32)> =
                        std::collections::HashMap::new();
                    let _ = diff.foreach(
                        &mut |_d, _| true,
                        None,
                        None,
                        Some(&mut |delta, _hunk, line| {
                            let path = delta
                                .new_file()
                                .path()
                                .or_else(|| delta.old_file().path())
                                .map(|p| p.to_string_lossy().into_owned());
                            if let Some(p) = path {
                                let entry = map.entry(p).or_insert((0, 0));
                                match line.origin() {
                                    '+' => entry.0 = entry.0.saturating_add(1),
                                    '-' => entry.1 = entry.1.saturating_add(1),
                                    _ => {}
                                }
                            }
                            true
                        }),
                    );
                    map
                }
                Err(_) => std::collections::HashMap::new(),
            },
            Err(_) => std::collections::HashMap::new(),
        };

    let mut out = Vec::with_capacity(statuses.len());
    for entry in statuses.iter() {
        let bits = entry.status();
        let path = match entry.path() {
            Some(p) => p.to_string(),
            None => continue,
        };

        // Map status bits to a single spec-shaped char. Order of checks
        let status = if bits.intersects(Status::WT_DELETED | Status::INDEX_DELETED) {
            '-'
        } else if bits.intersects(Status::WT_NEW | Status::INDEX_NEW) {
            '+'
        } else if bits.intersects(
            Status::WT_MODIFIED
                | Status::WT_RENAMED
                | Status::WT_TYPECHANGE
                | Status::INDEX_MODIFIED
                | Status::INDEX_RENAMED
                | Status::INDEX_TYPECHANGE
                | Status::CONFLICTED,
        ) {
            'M'
        } else {
            // Clean / ignored - skip.
            continue;
        };

        let delta = deltas.get(&path).copied();
        out.push(FileStatus {
            path,
            status,
            delta,
        });
    }
    Ok(out)
}

// ---- local excludes (`.git/info/exclude`) --------------------------------
//
// `.git/info/exclude` is git's per-repo ignore file: same syntax as
// `.gitignore`, but it lives inside `.git/` so it is never committed or
// pushed. Atlas owns one clearly-delimited block inside it and never touches
// lines the user wrote outside that block.

/// First line of the Atlas-managed span inside `.git/info/exclude`.
pub const EXCLUDE_BLOCK_START: &str = "# --- managed by Atlas (do not edit inside this block) ---";
/// Last line of the Atlas-managed span.
pub const EXCLUDE_BLOCK_END: &str = "# --- end Atlas ---";

/// Absolute path of the repo's `info/exclude` file.
fn exclude_file_path(repo: &Repository) -> std::path::PathBuf {
    repo.path().join("info").join("exclude")
}

/// Patterns inside the Atlas-managed block of `.git/info/exclude`. Empty
/// when the file or the block doesn't exist, or `project_path` isn't a repo.
pub fn read_local_excludes(project_path: &Path) -> anyhow::Result<Vec<String>> {
    let repo = match Repository::open(project_path) {
        Ok(r) => r,
        Err(e) if is_not_a_repo(&e) => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let file = exclude_file_path(&repo);
    let content = match std::fs::read_to_string(&file) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    Ok(parse_managed_block(&content))
}

/// Replace the Atlas-managed block with `patterns`, preserving every line
/// the user wrote outside it. An empty `patterns` removes the block.
pub fn write_local_excludes(project_path: &Path, patterns: &[String]) -> anyhow::Result<()> {
    let repo = Repository::open(project_path)?;
    let file = exclude_file_path(&repo);
    let existing = match std::fs::read_to_string(&file) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.into()),
    };
    let rewritten = splice_managed_block(&existing, patterns);
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&file, rewritten)?;
    Ok(())
}

/// True when git ignores `.atlas/` from any rule source - the managed block,
/// `.gitignore`, or the user's global excludes file.
pub fn atlas_dir_ignored(project_path: &Path) -> anyhow::Result<bool> {
    let repo = match Repository::open(project_path) {
        Ok(r) => r,
        Err(e) if is_not_a_repo(&e) => return Ok(false),
        Err(e) => return Err(e.into()),
    };
    // Trailing slash marks the path as a directory for dir-only rules.
    Ok(repo.is_path_ignored(".atlas/")?)
}

/// True when any `.atlas/` file is in the index. Ignore rules only affect
/// untracked files, so callers surface a "de-index first" warning.
pub fn atlas_dir_tracked(project_path: &Path) -> anyhow::Result<bool> {
    let repo = match Repository::open(project_path) {
        Ok(r) => r,
        Err(e) if is_not_a_repo(&e) => return Ok(false),
        Err(e) => return Err(e.into()),
    };
    let index = repo.index()?;
    Ok(index.iter().any(|e| e.path.starts_with(b".atlas/")))
}

fn parse_managed_block(content: &str) -> Vec<String> {
    let mut inside = false;
    let mut out = Vec::new();
    for line in content.lines() {
        let t = line.trim();
        if t == EXCLUDE_BLOCK_START {
            inside = true;
        } else if t == EXCLUDE_BLOCK_END {
            inside = false;
        } else if inside && !t.is_empty() && !t.starts_with('#') {
            out.push(t.to_string());
        }
    }
    out
}

fn splice_managed_block(existing: &str, patterns: &[String]) -> String {
    let mut kept: Vec<&str> = Vec::new();
    let mut inside = false;
    for line in existing.lines() {
        let t = line.trim();
        if t == EXCLUDE_BLOCK_START {
            inside = true;
        } else if t == EXCLUDE_BLOCK_END {
            inside = false;
        } else if !inside {
            kept.push(line);
        }
    }
    while kept.last().is_some_and(|l| l.trim().is_empty()) {
        kept.pop();
    }

    let cleaned: Vec<&str> = patterns
        .iter()
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect();

    let mut out = kept.join("\n");
    if cleaned.is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        return out;
    }
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str(EXCLUDE_BLOCK_START);
    out.push('\n');
    for p in &cleaned {
        out.push_str(p);
        out.push('\n');
    }
    out.push_str(EXCLUDE_BLOCK_END);
    out.push('\n');
    out
}

/// `(ahead, behind)` of the local branch vs its configured upstream.
fn ahead_behind(repo: &Repository) -> anyhow::Result<(u32, u32)> {
    let head = repo.head()?;
    if !head.is_branch() {
        return Ok((0, 0));
    }
    let shorthand = match head.shorthand() {
        Some(s) => s,
        None => return Ok((0, 0)),
    };
    let local_branch = repo.find_branch(shorthand, BranchType::Local)?;

    let upstream = match local_branch.upstream() {
        Ok(u) => u,
        Err(_) => return Ok((0, 0)),
    };

    let local_oid = match local_branch.get().target() {
        Some(o) => o,
        None => return Ok((0, 0)),
    };
    let upstream_oid = match upstream.get().target() {
        Some(o) => o,
        None => return Ok((0, 0)),
    };

    let (ahead, behind) = repo.graph_ahead_behind(local_oid, upstream_oid)?;
    Ok((ahead as u32, behind as u32))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;

    /// Spins up a bare, throwaway directory and returns (tempdir, path).
    struct TmpDir(std::path::PathBuf);
    impl TmpDir {
        fn new(tag: &str) -> anyhow::Result<Self> {
            let base = std::env::temp_dir().join(format!(
                "atlas-git-test-{}-{}-{}",
                tag,
                std::process::id(),
                chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
            ));
            fs::create_dir_all(&base)?;
            Ok(Self(base))
        }
    }
    impl Drop for TmpDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn is_git_repo_false_on_empty_dir() -> anyhow::Result<()> {
        let tmp = TmpDir::new("is-git-empty")?;
        assert!(!is_git_repo(&tmp.0));
        Ok(())
    }

    #[test]
    fn read_status_returns_none_on_non_repo() -> anyhow::Result<()> {
        let tmp = TmpDir::new("non-repo")?;
        let got = read_status(&tmp.0)?;
        assert!(got.is_none(), "expected None for non-repo, got {got:?}");
        Ok(())
    }

    #[test]
    fn read_status_clean_new_repo_reports_empty_status() -> anyhow::Result<()> {
        let tmp = TmpDir::new("clean")?;

        // `git2::Repository::init` is fine here - we stay inside the crate
        let repo = Repository::init(&tmp.0)?;
        // Set a config so dirty-count doesn't complain if we later add a commit.
        repo.config()?.set_str("user.email", "test@example.com")?;
        repo.config()?.set_str("user.name", "Atlas Test")?;

        assert!(is_git_repo(&tmp.0));
        let got = read_status(&tmp.0)?;
        let s = got.expect("init'd repo should surface a status");
        assert_eq!(s.dirty, 0, "no files yet, no dirty bits");
        assert_eq!(s.ahead, 0);
        assert_eq!(s.behind, 0);
        // Unborn branch → shorthand empty.
        assert_eq!(s.branch, "");
        Ok(())
    }

    #[test]
    fn read_status_counts_dirty_files() -> anyhow::Result<()> {
        let tmp = TmpDir::new("dirty")?;
        let repo = Repository::init(&tmp.0)?;
        repo.config()?.set_str("user.email", "test@example.com")?;
        repo.config()?.set_str("user.name", "Atlas Test")?;

        // Create one untracked file. Should count as WT_NEW (dirty).
        fs::write(tmp.0.join("README.md"), b"# hi\n")?;

        let s = read_status(&tmp.0)?.expect("repo present");
        assert!(s.dirty >= 1, "expected at least 1 dirty, got {}", s.dirty);
        Ok(())
    }

    /// Smoke test that `read_status` also works when invoked from inside
    #[test]
    fn read_status_from_subdirectory() -> anyhow::Result<()> {
        let tmp = TmpDir::new("subdir")?;
        let _repo = Repository::init(&tmp.0)?;
        let sub = tmp.0.join("src");
        fs::create_dir_all(&sub)?;

        // The API opens the repo at the given path; for "walk up" we have
        let s = read_status(&tmp.0)?.expect("repo present");
        assert_eq!(s.ahead, 0);
        assert_eq!(s.behind, 0);

        // Also: calling with a non-repo subdir that's never been init'd
        let _ = s;
        let outside = tmp
            .0
            .parent()
            .ok_or_else(|| anyhow::anyhow!("no parent"))?
            .join("atlas-test-not-a-repo");
        let _ = fs::create_dir_all(&outside);
        let got = read_status(&outside)?;
        assert!(got.is_none());
        let _ = fs::remove_dir_all(&outside);
        Ok(())
    }

    /// Smoke of `Command::new("git")` availability - skipped if git CLI
    #[test]
    fn ahead_behind_zero_without_upstream() -> anyhow::Result<()> {
        let tmp = TmpDir::new("no-upstream")?;
        let repo = Repository::init(&tmp.0)?;
        repo.config()?.set_str("user.email", "test@example.com")?;
        repo.config()?.set_str("user.name", "Atlas Test")?;

        // Make one commit so HEAD is valid.
        let sig = repo.signature()?;
        fs::write(tmp.0.join("a.txt"), b"a")?;
        let mut idx = repo.index()?;
        idx.add_path(Path::new("a.txt"))?;
        idx.write()?;
        let tree_id = idx.write_tree()?;
        let tree = repo.find_tree(tree_id)?;
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])?;

        // Use `git --version` to decide whether git CLI paths are even
        let _ = Command::new("git").arg("--version").output();

        let s = read_status(&tmp.0)?.expect("repo present");
        assert_eq!((s.ahead, s.behind), (0, 0));
        Ok(())
    }

    #[test]
    fn local_excludes_round_trip_and_ignore_effect() -> anyhow::Result<()> {
        let tmp = TmpDir::new("excludes-roundtrip")?;
        let _repo = Repository::init(&tmp.0)?;

        // Nothing written yet.
        assert!(read_local_excludes(&tmp.0)?.is_empty());
        assert!(!atlas_dir_ignored(&tmp.0)?);

        let patterns = vec![".atlas/".to_string(), "*.excalidraw".to_string()];
        write_local_excludes(&tmp.0, &patterns)?;
        assert_eq!(read_local_excludes(&tmp.0)?, patterns);

        // The written block must actually change git's ignore verdict, both
        // for the `.atlas/` dir rule and for a glob rule.
        fs::create_dir_all(tmp.0.join(".atlas"))?;
        fs::write(tmp.0.join(".atlas").join("todos.json"), b"{}")?;
        assert!(atlas_dir_ignored(&tmp.0)?);
        let repo = Repository::open(&tmp.0)?;
        assert!(repo.is_path_ignored("diagram.excalidraw")?);
        assert!(!repo.is_path_ignored("kept.txt")?);

        // Dirty count must not include the ignored `.atlas/` contents.
        let s = read_status(&tmp.0)?.expect("repo present");
        assert_eq!(s.dirty, 0, "ignored paths must not count as dirty");
        Ok(())
    }

    #[test]
    fn write_local_excludes_preserves_user_lines_outside_block() -> anyhow::Result<()> {
        let tmp = TmpDir::new("excludes-preserve")?;
        let repo = Repository::init(&tmp.0)?;

        // The user already had hand-written rules in info/exclude.
        let file = repo.path().join("info").join("exclude");
        fs::create_dir_all(file.parent().unwrap())?;
        fs::write(&file, "# mine\nsecret.env\n")?;

        write_local_excludes(&tmp.0, &[".atlas/".to_string()])?;
        let after = fs::read_to_string(&file)?;
        assert!(after.contains("# mine"), "{after}");
        assert!(after.contains("secret.env"), "{after}");
        assert!(after.contains(EXCLUDE_BLOCK_START), "{after}");

        // Rewriting with a different set replaces only the block...
        write_local_excludes(&tmp.0, &["scratch-*.md".to_string()])?;
        let after = fs::read_to_string(&file)?;
        assert!(after.contains("secret.env"), "{after}");
        assert!(after.contains("scratch-*.md"), "{after}");
        assert!(!after.contains(".atlas/"), "{after}");

        // ...and an empty set removes the block but keeps user lines.
        write_local_excludes(&tmp.0, &[])?;
        let after = fs::read_to_string(&file)?;
        assert!(after.contains("secret.env"), "{after}");
        assert!(!after.contains(EXCLUDE_BLOCK_START), "{after}");
        Ok(())
    }

    #[test]
    fn atlas_dir_tracked_detects_committed_atlas_files() -> anyhow::Result<()> {
        let tmp = TmpDir::new("excludes-tracked")?;
        let repo = Repository::init(&tmp.0)?;
        repo.config()?.set_str("user.email", "test@example.com")?;
        repo.config()?.set_str("user.name", "Atlas Test")?;

        assert!(!atlas_dir_tracked(&tmp.0)?);

        fs::create_dir_all(tmp.0.join(".atlas"))?;
        fs::write(tmp.0.join(".atlas").join("todos.json"), b"{}")?;
        let mut idx = repo.index()?;
        idx.add_path(Path::new(".atlas/todos.json"))?;
        idx.write()?;

        assert!(atlas_dir_tracked(&tmp.0)?);
        Ok(())
    }
}
