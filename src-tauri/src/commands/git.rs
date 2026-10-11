//! ```text

use std::path::{Path, PathBuf};
use std::process::Stdio;

use git2::{BranchType, DiffOptions, Repository, StatusOptions};
use serde::{Deserialize, Serialize};
use tauri::State;
use tokio::process::Command;
use ts_rs::TS;

use crate::storage::Db;
use crate::watcher::WatcherManager;

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
pub async fn git_push(
    state: State<'_, Db>,
    watcher: State<'_, WatcherManager>,
    project_id: String,
) -> Result<GitActionResult, String> {
    let cwd = resolve_project_path(&state, &project_id).await?;
    let out = push_current(&cwd).await?;
    watcher.refresh_git_status_for(cwd);
    Ok(out)
}

/// `git push`, falling back to `git push -u origin <branch>` when the
/// current branch has no upstream yet (fresh local branch).
async fn push_current(cwd: &Path) -> Result<GitActionResult, String> {
    // First attempt: plain `git push` (uses configured upstream).
    let first = run_git(cwd, &["push"]).await?;
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
    let branch = match current_branch_name(cwd) {
        Some(b) => b,
        None => return Ok(first),
    };
    run_git(cwd, &["push", "-u", "origin", &branch]).await
}

/// One shell step of a `git_sync` run, in execution order.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct GitSyncStep {
    /// `status` | `stash` | `pull` | `rebase-abort` | `push` | `stash-pop`.
    pub name: String,
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Outcome of `git_sync`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct GitSyncResult {
    /// True only when every step that ran succeeded.
    pub ok: bool,
    /// Local changes were stashed before the rebase.
    pub stashed: bool,
    /// The stash was popped back onto the working tree. When `stashed` is
    /// true and this is false, the changes are still in `git stash list`.
    pub stash_restored: bool,
    /// One-line human summary for the toast.
    pub summary: String,
    pub steps: Vec<GitSyncStep>,
}

/// `git.sync` - bring the current branch in line with its upstream and
/// publish local commits in one go:
///
/// 1. `git stash push -u` when the working tree is dirty,
/// 2. `git pull --rebase` (aborting the rebase on conflict so the tree is
///    left exactly as it was),
/// 3. `git push` (setting upstream if needed),
/// 4. `git stash pop` to re-apply step 1.
#[tauri::command]
pub async fn git_sync(
    state: State<'_, Db>,
    watcher: State<'_, WatcherManager>,
    project_id: String,
) -> Result<GitSyncResult, String> {
    let cwd = resolve_project_path(&state, &project_id).await?;
    let result = sync_repo(&cwd).await?;
    watcher.refresh_git_status_for(cwd);
    Ok(result)
}

const SYNC_STASH_MESSAGE: &str = "atlas: sync autostash";

pub(crate) async fn sync_repo(cwd: &Path) -> Result<GitSyncResult, String> {
    let mut steps: Vec<GitSyncStep> = Vec::new();

    // ---- 1. dirty check -------------------------------------------------
    let status = run_git(cwd, &["status", "--porcelain", "--untracked-files=all"]).await?;
    let status_ok = status.ok;
    let dirty = status_ok && !status.stdout.trim().is_empty();
    steps.push(step("status", &status));
    if !status_ok {
        return Ok(finish(
            false,
            false,
            false,
            format!("git status failed: {}", tail(&status)),
            steps,
        ));
    }

    let mut stashed = false;
    if dirty {
        let stash = run_git(cwd, &["stash", "push", "-u", "-m", SYNC_STASH_MESSAGE]).await?;
        let stash_ok = stash.ok;
        // `git stash push` exits 0 with "No local changes to save" when the
        // only dirt is something it won't stash; don't pop in that case.
        stashed = stash_ok && !stash.stdout.contains("No local changes to save");
        steps.push(step("stash", &stash));
        if !stash_ok {
            return Ok(finish(
                false,
                false,
                false,
                format!("couldn't stash local changes: {}", tail(&stash)),
                steps,
            ));
        }
    }

    // ---- 2. pull --rebase -----------------------------------------------
    let mut pull = run_git(cwd, &["pull", "--rebase"]).await?;
    if !pull.ok && pull.stderr.contains("no tracking information") {
        if let Some(branch) = current_branch_name(cwd) {
            pull = run_git(cwd, &["pull", "--rebase", "origin", &branch]).await?;
        }
    }
    let pull_ok = pull.ok;
    let pull_tail = tail(&pull);
    steps.push(step("pull", &pull));
    if !pull_ok {
        if rebase_in_progress(cwd).await {
            let abort = run_git(cwd, &["rebase", "--abort"]).await?;
            steps.push(step("rebase-abort", &abort));
        }
        let restored = if stashed {
            pop_stash(cwd, &mut steps).await?
        } else {
            false
        };
        let mut summary = format!("pull --rebase failed: {pull_tail}");
        if stashed && !restored {
            summary.push_str(" · your changes are kept in `git stash list`");
        }
        return Ok(finish(false, stashed, restored, summary, steps));
    }

    // ---- 3. push --------------------------------------------------------
    let push = push_current(cwd).await?;
    let push_ok = push.ok;
    let push_tail = tail(&push);
    steps.push(step("push", &push));

    // ---- 4. restore stash -----------------------------------------------
    let restored = if stashed {
        pop_stash(cwd, &mut steps).await?
    } else {
        false
    };

    let ok = push_ok && (!stashed || restored);
    let mut summary = if push_ok {
        "Synced: pulled with rebase and pushed".to_string()
    } else {
        format!("pulled with rebase, but push failed: {push_tail}")
    };
    if stashed {
        summary.push_str(if restored {
            " · local changes re-applied"
        } else {
            " · stash pop conflicted; your changes are in `git stash list`"
        });
    }
    Ok(finish(ok, stashed, restored, summary, steps))
}

async fn pop_stash(cwd: &Path, steps: &mut Vec<GitSyncStep>) -> Result<bool, String> {
    let pop = run_git(cwd, &["stash", "pop"]).await?;
    let ok = pop.ok;
    steps.push(step("stash-pop", &pop));
    Ok(ok)
}

/// True when `.git/rebase-merge` or `.git/rebase-apply` exists (resolved
/// through `git rev-parse --git-path` so linked worktrees work too).
async fn rebase_in_progress(cwd: &Path) -> bool {
    for dir in ["rebase-merge", "rebase-apply"] {
        if let Ok(out) = run_git(cwd, &["rev-parse", "--git-path", dir]).await {
            let rel = out.stdout.trim();
            if !rel.is_empty() {
                let p = PathBuf::from(rel);
                let abs = if p.is_absolute() { p } else { cwd.join(p) };
                if abs.exists() {
                    return true;
                }
            }
        }
    }
    false
}

fn step(name: &str, r: &GitActionResult) -> GitSyncStep {
    GitSyncStep {
        name: name.to_string(),
        ok: r.ok,
        stdout: r.stdout.clone(),
        stderr: r.stderr.clone(),
    }
}

fn finish(
    ok: bool,
    stashed: bool,
    stash_restored: bool,
    summary: String,
    steps: Vec<GitSyncStep>,
) -> GitSyncResult {
    GitSyncResult {
        ok,
        stashed,
        stash_restored,
        summary,
        steps,
    }
}

/// Last non-empty line of stderr (falling back to stdout) for toasts.
fn tail(r: &GitActionResult) -> String {
    let pick = |s: &str| {
        s.lines()
            .map(str::trim)
            .rfind(|l| !l.is_empty())
            .map(str::to_string)
    };
    pick(&r.stderr)
        .or_else(|| pick(&r.stdout))
        .unwrap_or_else(|| "unknown error".to_string())
}

/// Snapshot of the Atlas-managed local-exclude state for a project. Backs
/// the Inspector → Ignores tab.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct LocalExcludes {
    /// False when the project directory is not a git repository.
    pub is_git_repo: bool,
    /// Patterns inside the Atlas-managed block of `.git/info/exclude`.
    pub patterns: Vec<String>,
    /// True when git ignores `.atlas/` from any rule source (managed block,
    /// `.gitignore`, global excludes).
    pub atlas_ignored: bool,
    /// True when `.atlas/` files are committed to the index. Ignore rules
    /// only affect untracked files, so the UI shows a de-index warning.
    pub atlas_tracked: bool,
}

/// `git.local_excludes_get` - read the Atlas-managed block of
/// `.git/info/exclude` plus the `.atlas/` ignored/tracked flags.
#[tauri::command]
pub async fn local_excludes_get(
    state: State<'_, Db>,
    project_id: String,
) -> Result<LocalExcludes, String> {
    let cwd = resolve_project_path(&state, &project_id).await?;
    tauri::async_runtime::spawn_blocking(move || read_excludes_state(&cwd))
        .await
        .map_err(|e| format!("join blocking: {e}"))?
        .map_err(|e| e.to_string())
}

/// `git.local_excludes_set` - replace the Atlas-managed block with
/// `patterns` (empty list removes the block) and return the fresh state.
#[tauri::command]
pub async fn local_excludes_set(
    state: State<'_, Db>,
    project_id: String,
    patterns: Vec<String>,
) -> Result<LocalExcludes, String> {
    let cwd = resolve_project_path(&state, &project_id).await?;
    tauri::async_runtime::spawn_blocking(move || {
        crate::git::write_local_excludes(&cwd, &patterns)?;
        read_excludes_state(&cwd)
    })
    .await
    .map_err(|e| format!("join blocking: {e}"))?
    .map_err(|e| e.to_string())
}

fn read_excludes_state(project_path: &Path) -> anyhow::Result<LocalExcludes> {
    if !crate::git::is_git_repo(project_path) {
        return Ok(LocalExcludes {
            is_git_repo: false,
            patterns: Vec::new(),
            atlas_ignored: false,
            atlas_tracked: false,
        });
    }
    Ok(LocalExcludes {
        is_git_repo: true,
        patterns: crate::git::read_local_excludes(project_path)?,
        atlas_ignored: crate::git::atlas_dir_ignored(project_path)?,
        atlas_tracked: crate::git::atlas_dir_tracked(project_path)?,
    })
}

async fn resolve_project_path(state: &Db, project_id: &str) -> Result<PathBuf, String> {
    let project = state
        .get_project(project_id)
        .await
        .map_err(|e: anyhow::Error| e.to_string())?
        .ok_or_else(|| format!("project not found: {project_id}"))?;
    Ok(PathBuf::from(&project.path))
}

pub(crate) async fn run_git(cwd: &Path, args: &[&str]) -> Result<GitActionResult, String> {
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

    // ---- git_sync -------------------------------------------------------

    /// Run `git` with the given args in `cwd`, panicking on failure.
    fn sh(cwd: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(args)
            .env("GIT_AUTHOR_NAME", "Atlas Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "Atlas Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.com")
            .output()
            .expect("spawn git");
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// Bare remote + two clones (`a` publishes, `b` is the repo under test)
    /// sharing one commit on `main`.
    fn sync_fixture(tag: &str) -> anyhow::Result<(TmpDir, PathBuf, PathBuf, PathBuf)> {
        let tmp = TmpDir::new(tag)?;
        let remote = tmp.0.join("remote.git");
        let a = tmp.0.join("a");
        let b = tmp.0.join("b");
        sh(&tmp.0, &["init", "--bare", "-q", remote.to_str().unwrap()]);
        sh(
            &tmp.0,
            &["clone", "-q", remote.to_str().unwrap(), a.to_str().unwrap()],
        );
        sh(&a, &["checkout", "-q", "-b", "main"]);
        fs::write(a.join("shared.txt"), "v1\n")?;
        sh(&a, &["add", "-A"]);
        sh(&a, &["commit", "-q", "-m", "init"]);
        sh(&a, &["push", "-q", "-u", "origin", "main"]);
        sh(
            &tmp.0,
            &["clone", "-q", remote.to_str().unwrap(), b.to_str().unwrap()],
        );
        Ok((tmp, remote, a, b))
    }

    #[tokio::test]
    async fn sync_stashes_rebases_pushes_and_restores() -> anyhow::Result<()> {
        let (_tmp, remote, a, b) = sync_fixture("sync-ok")?;

        // Upstream moves ahead.
        fs::write(a.join("upstream.txt"), "from a\n")?;
        sh(&a, &["add", "-A"]);
        sh(&a, &["commit", "-q", "-m", "upstream"]);
        sh(&a, &["push", "-q"]);

        // Local commit + dirty tracked file + untracked file in `b`.
        fs::write(b.join("local.txt"), "from b\n")?;
        sh(&b, &["add", "-A"]);
        sh(&b, &["commit", "-q", "-m", "local"]);
        fs::write(b.join("shared.txt"), "v1\nwip\n")?;
        fs::write(b.join("scratch.txt"), "untracked\n")?;

        let res = sync_repo(&b).await.map_err(|e| anyhow::anyhow!(e))?;
        assert!(res.ok, "{res:?}");
        assert!(res.stashed);
        assert!(res.stash_restored);

        // Remote has both commits, local is in sync with it.
        let remote_log = sh(&remote, &["log", "--oneline", "main"]);
        assert_eq!(remote_log.lines().count(), 3, "{remote_log}");
        let counts = sh(
            &b,
            &["rev-list", "--left-right", "--count", "HEAD...origin/main"],
        );
        assert_eq!(counts.trim(), "0\t0", "{counts}");

        // Working-tree changes survived.
        assert_eq!(fs::read_to_string(b.join("shared.txt"))?, "v1\nwip\n");
        assert!(b.join("scratch.txt").exists());
        assert!(b.join("upstream.txt").exists());
        let stash = sh(&b, &["stash", "list"]);
        assert!(stash.trim().is_empty(), "stash should be popped: {stash}");
        Ok(())
    }

    #[tokio::test]
    async fn sync_aborts_rebase_on_conflict_and_restores_stash() -> anyhow::Result<()> {
        let (_tmp, _remote, a, b) = sync_fixture("sync-conflict")?;

        fs::write(a.join("shared.txt"), "v2 from a\n")?;
        sh(&a, &["commit", "-q", "-am", "a edits shared"]);
        sh(&a, &["push", "-q"]);

        fs::write(b.join("shared.txt"), "v2 from b\n")?;
        sh(&b, &["commit", "-q", "-am", "b edits shared"]);
        let local_head = sh(&b, &["rev-parse", "HEAD"]);
        fs::write(b.join("notes.txt"), "dirty\n")?;

        let res = sync_repo(&b).await.map_err(|e| anyhow::anyhow!(e))?;
        assert!(!res.ok);
        assert!(res.stashed);
        assert!(res.stash_restored, "{res:?}");
        assert!(
            res.summary.contains("pull --rebase failed"),
            "{}",
            res.summary
        );
        assert!(res.steps.iter().any(|s| s.name == "rebase-abort" && s.ok));
        assert!(!res.steps.iter().any(|s| s.name == "push"));

        // Tree is exactly as before: same HEAD, no rebase in flight, dirt back.
        assert_eq!(sh(&b, &["rev-parse", "HEAD"]), local_head);
        assert!(!rebase_in_progress(&b).await);
        assert!(b.join("notes.txt").exists());
        assert_eq!(fs::read_to_string(b.join("shared.txt"))?, "v2 from b\n");
        Ok(())
    }

    #[tokio::test]
    async fn sync_clean_tree_skips_stash() -> anyhow::Result<()> {
        let (_tmp, _remote, _a, b) = sync_fixture("sync-clean")?;
        let res = sync_repo(&b).await.map_err(|e| anyhow::anyhow!(e))?;
        assert!(res.ok, "{res:?}");
        assert!(!res.stashed);
        assert!(!res.steps.iter().any(|s| s.name == "stash"));
        assert!(res.steps.iter().any(|s| s.name == "push" && s.ok));
        Ok(())
    }
}
