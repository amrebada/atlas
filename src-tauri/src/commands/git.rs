//! ```text

use std::path::{Path, PathBuf};
use std::process::Stdio;

use git2::{BranchType, DiffOptions, Repository, StatusOptions};
use serde::{Deserialize, Serialize};
use tauri::State;
use tokio::process::Command;
use ts_rs::TS;

use crate::storage::Db;

/// A single row returned by `git_branch_list`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct BranchInfo {
    /// Short branch name, e.g. `main` or `origin/feat/x`.
    pub name: String,
    /// True when this branch is the current HEAD (only meaningful for local
    pub is_head: bool,
    /// True for remote-tracking branches (`refs/remotes/...`).
    pub is_remote: bool,
}

/// Read-only result of a checkout preview.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct GitCheckoutPreview {
    /// The branch that was previewed (echoed from the request).
    pub branch: String,
    /// Count of files whose blob differs between `HEAD` and the target
    pub files_would_change: u32,
    /// True when the working tree has uncommitted changes.
    pub is_dirty: bool,
    /// Human-readable warning shown in the confirmation dialog. `Some`
    pub warning: Option<String>,
}

/// `git.branch_list` - enumerate local + remote branches for the repo at
#[tauri::command]
pub async fn git_branch_list(
    state: State<'_, Db>,
    project_id: String,
) -> Result<Vec<BranchInfo>, String> {
    let project = state
        .get_project(&project_id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())?
        .ok_or_else(|| format!("project not found: {project_id}"))?;
    let project_path = PathBuf::from(&project.path);

    tauri::async_runtime::spawn_blocking(move || list_branches(&project_path))
        .await
        .map_err(|e| format!("join blocking: {e}"))?
        .map_err(|e| e.to_string())
}

/// Result of a mutating git action - surfaces the exit code and the raw
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct GitActionResult {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

/// `git.commit` - stage every tracked + untracked change and commit with
#[tauri::command]
pub async fn git_commit(
    state: State<'_, Db>,
    project_id: String,
    message: String,
) -> Result<GitActionResult, String> {
    let cwd = resolve_project_path(&state, &project_id).await?;
    let trimmed = message.trim().to_string();
    if trimmed.is_empty() {
        return Err("commit message is empty".into());
    }
    let add = run_git(&cwd, &["add", "-A"]).await?;
    if !add.ok {
        return Ok(add);
    }
    run_git(&cwd, &["commit", "-m", &trimmed]).await
}

/// `git.stash` - stash every working-tree change (including untracked
#[tauri::command]
pub async fn git_stash(
    state: State<'_, Db>,
    project_id: String,
    message: String,
) -> Result<GitActionResult, String> {
    let cwd = resolve_project_path(&state, &project_id).await?;
    let trimmed = message.trim().to_string();
    let mut args: Vec<&str> = vec!["stash", "push", "-u"];
    if !trimmed.is_empty() {
        args.push("-m");
        args.push(&trimmed);
    }
    run_git(&cwd, &args).await
}

/// `git.push` - push the current branch to its tracked upstream. If no
#[tauri::command]
pub async fn git_push(state: State<'_, Db>, project_id: String) -> Result<GitActionResult, String> {
    let cwd = resolve_project_path(&state, &project_id).await?;
    // First attempt: plain `git push` (uses configured upstream).
    let first = run_git(&cwd, &["push"]).await?;
    if first.ok {
        return Ok(first);
    }
    // Common case on a freshly-created branch: no upstream is set. Detect
    let needs_upstream = first.stderr.contains("no upstream branch")
        || first.stderr.contains("has no upstream branch")
        || first.stderr.contains("--set-upstream");
    if !needs_upstream {
        return Ok(first);
    }
    let branch = match current_branch_name(&cwd) {
        Some(b) => b,
        None => return Ok(first),
    };
    run_git(&cwd, &["push", "-u", "origin", &branch]).await
}

async fn resolve_project_path(state: &Db, project_id: &str) -> Result<PathBuf, String> {
    let project = state
        .get_project(project_id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())?
        .ok_or_else(|| format!("project not found: {project_id}"))?;
    Ok(PathBuf::from(&project.path))
}

async fn run_git(cwd: &Path, args: &[&str]) -> Result<GitActionResult, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| format!("spawn git: {e}"))?;
    Ok(GitActionResult {
        ok: out.status.success(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    })
}

fn current_branch_name(cwd: &Path) -> Option<String> {
    let repo = Repository::open(cwd).ok()?;
    let head = repo.head().ok()?;
    if !head.is_branch() {
        return None;
    }
    head.shorthand().map(|s| s.to_string())
}

/// `git.checkout` - read-only preview. Returns a [`GitCheckoutPreview`]
#[tauri::command]
pub async fn git_checkout(
    state: State<'_, Db>,
    project_id: String,
    branch: String,
) -> Result<GitCheckoutPreview, String> {
    let project = state
        .get_project(&project_id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())?
        .ok_or_else(|| format!("project not found: {project_id}"))?;
    let project_path = PathBuf::from(&project.path);

    let branch_for_call = branch.clone();
    tauri::async_runtime::spawn_blocking(move || preview_checkout(&project_path, &branch_for_call))
        .await
        .map_err(|e| format!("join blocking: {e}"))?
        .map_err(|e| e.to_string())
}

// ---- implementation ------------------------------------------------------

fn list_branches(project_path: &Path) -> anyhow::Result<Vec<BranchInfo>> {
    let repo = Repository::open(project_path)?;

    let mut out: Vec<BranchInfo> = Vec::new();
    // `None` → iterate over both local and remote. Each iteration yields
    let branches = repo.branches(None)?;
    for item in branches {
        let (branch, btype) = item?;
        let name = match branch.name() {
            Ok(Some(n)) => n.to_string(),
            // Non-UTF-8 branch names are rare and not useful to surface;
            Ok(None) => continue,
            Err(_) => continue,
        };

        // Skip the `HEAD` symref (commonly `origin/HEAD`). It's an alias,
        if name == "HEAD" || name.ends_with("/HEAD") {
            continue;
        }

        let is_remote = matches!(btype, BranchType::Remote);
        // `is_head` is only meaningful for local branches; libgit2's
        let is_head = branch.is_head();

        out.push(BranchInfo {
            name,
            is_head,
            is_remote,
        });
    }

    // Stable order: locals first, then remotes, each alphabetical.
    out.sort_by(|a, b| {
        a.is_remote
            .cmp(&b.is_remote)
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(out)
}

fn preview_checkout(project_path: &Path, branch: &str) -> anyhow::Result<GitCheckoutPreview> {
    let repo = Repository::open(project_path)?;

    // ---- dirty check --------------------------------------------------
    let mut status_opts = StatusOptions::new();
    status_opts
        .include_untracked(true)
        .include_ignored(false)
        .include_unmodified(false)
        .exclude_submodules(true)
        .recurse_untracked_dirs(false);
    let statuses = repo.statuses(Some(&mut status_opts))?;
    let is_dirty = statuses.iter().any(|e| !e.status().is_empty());

    // ---- target tree resolution --------------------------------------
    let target_branch = repo
        .find_branch(branch, BranchType::Local)
        .or_else(|_| repo.find_branch(branch, BranchType::Remote))
        .map_err(|e| anyhow::anyhow!("branch not found: {branch} ({e})"))?;
    let target_commit = target_branch.get().peel_to_commit()?;
    let target_tree = target_commit.tree()?;

    // HEAD tree; fall back to an empty diff if HEAD is unborn (fresh repo).
    let head_tree = match repo.head() {
        Ok(head_ref) => Some(head_ref.peel_to_commit()?.tree()?),
        Err(_) => None,
    };

    let mut diff_opts = DiffOptions::new();
    diff_opts.ignore_submodules(true);
    let diff =
        repo.diff_tree_to_tree(head_tree.as_ref(), Some(&target_tree), Some(&mut diff_opts))?;

    let files_would_change: u32 = diff.deltas().len().try_into().unwrap_or(u32::MAX);

    let warning = if is_dirty {
        Some("working tree has uncommitted changes — checkout would require stash".to_string())
    } else {
        None
    };

    Ok(GitCheckoutPreview {
        branch: branch.to_string(),
        files_would_change,
        is_dirty,
        warning,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Throwaway temp dir that cleans up on drop. Mirrors the helper in
    struct TmpDir(std::path::PathBuf);
    impl TmpDir {
        fn new(tag: &str) -> anyhow::Result<Self> {
            let base = std::env::temp_dir().join(format!(
                "atlas-git-cmd-test-{}-{}-{}",
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

    /// Init a repo, configure an identity, and return the `Repository`.
    fn init_repo(path: &Path) -> anyhow::Result<Repository> {
        let repo = Repository::init(path)?;
        repo.config()?.set_str("user.email", "test@example.com")?;
        repo.config()?.set_str("user.name", "Atlas Test")?;
        Ok(repo)
    }

    /// Stage every file in the index and commit on the current branch.
    fn commit_all(repo: &Repository, message: &str) -> anyhow::Result<git2::Oid> {
        let mut idx = repo.index()?;
        idx.add_all(["."].iter(), git2::IndexAddOption::DEFAULT, None)?;
        idx.write()?;
        let tree_id = idx.write_tree()?;
        let tree = repo.find_tree(tree_id)?;
        let sig = repo.signature()?;
        let parent = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
        let parents: Vec<&git2::Commit> = parent.iter().collect();
        let oid = repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parents)?;
        Ok(oid)
    }

    #[test]
    fn list_branches_returns_all_locals_and_skips_head_symref() -> anyhow::Result<()> {
        let tmp = TmpDir::new("branch-list")?;
        let repo = init_repo(&tmp.0)?;

        // First commit on the default branch.
        fs::write(tmp.0.join("a.txt"), b"a")?;
        let first = commit_all(&repo, "init")?;

        // Create two more branches pointing at the initial commit.
        let commit = repo.find_commit(first)?;
        repo.branch("feat/one", &commit, false)?;
        repo.branch("bugfix/two", &commit, false)?;

        let rows = list_branches(&tmp.0)?;
        // Exactly three local branches; no remotes configured.
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert!(names.contains(&"feat/one"), "{names:?}");
        assert!(names.contains(&"bugfix/two"), "{names:?}");
        assert_eq!(
            rows.iter().filter(|r| r.is_head).count(),
            1,
            "exactly one local branch should be HEAD"
        );
        // No row equals or ends with "/HEAD".
        assert!(rows
            .iter()
            .all(|r| r.name != "HEAD" && !r.name.ends_with("/HEAD")));
        Ok(())
    }

    #[test]
    fn preview_checkout_reports_diff_count_and_clean_worktree() -> anyhow::Result<()> {
        let tmp = TmpDir::new("preview-diff")?;
        let repo = init_repo(&tmp.0)?;

        // Commit A on main.
        fs::write(tmp.0.join("a.txt"), b"a1")?;
        let first = commit_all(&repo, "a")?;

        // Create `feature` from first commit, switch to it, modify a file,
        let first_commit = repo.find_commit(first)?;
        repo.branch("feature", &first_commit, false)?;
        repo.set_head("refs/heads/feature")?;
        repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))?;
        fs::write(tmp.0.join("a.txt"), b"a2")?;
        fs::write(tmp.0.join("b.txt"), b"b")?;
        commit_all(&repo, "feature commit")?;

        // Switch back to the original branch (commonly `main` or `master`).
        let default_branch_name = {
            let local = repo.branches(Some(BranchType::Local))?;
            let mut name: Option<String> = None;
            for b in local {
                let (br, _) = b?;
                if let Ok(Some(n)) = br.name() {
                    if n != "feature" {
                        name = Some(n.to_string());
                        break;
                    }
                }
            }
            name.ok_or_else(|| anyhow::anyhow!("default branch missing"))?
        };
        repo.set_head(&format!("refs/heads/{default_branch_name}"))?;
        repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))?;

        let preview = preview_checkout(&tmp.0, "feature")?;
        assert_eq!(preview.branch, "feature");
        // a.txt modified + b.txt added = 2 files would change.
        assert_eq!(preview.files_would_change, 2);
        assert!(!preview.is_dirty, "worktree should be clean post-checkout");
        assert!(preview.warning.is_none());
        Ok(())
    }

    #[test]
    fn preview_checkout_flags_dirty_worktree() -> anyhow::Result<()> {
        let tmp = TmpDir::new("preview-dirty")?;
        let repo = init_repo(&tmp.0)?;

        fs::write(tmp.0.join("a.txt"), b"a1")?;
        let first = commit_all(&repo, "a")?;
        let commit = repo.find_commit(first)?;
        repo.branch("feature", &commit, false)?;

        // Introduce an untracked file so the worktree is dirty.
        fs::write(tmp.0.join("scratch.txt"), b"x")?;

        let preview = preview_checkout(&tmp.0, "feature")?;
        assert!(preview.is_dirty);
        assert!(preview
            .warning
            .as_deref()
            .unwrap_or("")
            .contains("uncommitted"));
        Ok(())
    }
}
