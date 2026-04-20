//! Template registry - built-in + user-added.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::storage::discovery::{classify, DiscoveredRepo};
use crate::storage::settings::{apply_patch, load};
use crate::storage::types::{ProjectId, Settings, Template};
use crate::storage::AppContext;

/// Immutable built-in template catalog. Matches the brief exactly so the
pub fn builtin_templates() -> Vec<Template> {
    vec![
        Template {
            id: "node-ts".into(),
            label: "Node + TypeScript".into(),
            color: "#3178c6".into(),
            hint: "pnpm · vitest · tsup".into(),
            path: String::new(),
            builtin: true,
        },
        Template {
            id: "rust-cli".into(),
            label: "Rust CLI".into(),
            color: "#dea584".into(),
            hint: "cargo · clap".into(),
            path: String::new(),
            builtin: true,
        },
        Template {
            id: "python-uv".into(),
            label: "Python (uv)".into(),
            color: "#3572a5".into(),
            hint: "uv · pytest".into(),
            path: String::new(),
            builtin: true,
        },
        Template {
            id: "go-service".into(),
            label: "Go service".into(),
            color: "#00ADD8".into(),
            hint: "go modules".into(),
            path: String::new(),
            builtin: true,
        },
        Template {
            id: "empty".into(),
            label: "Empty folder".into(),
            color: "#9e9e9e".into(),
            hint: "just a folder".into(),
            path: String::new(),
            builtin: true,
        },
    ]
}

/// Ids of built-in templates. Cheap because the list is small and
fn is_builtin(id: &str) -> bool {
    builtin_templates().iter().any(|t| t.id == id)
}

/// Full template list (built-ins + user). Built-ins always come first so
pub async fn list_all(settings: &Settings) -> Vec<Template> {
    let builtins = builtin_templates();
    let builtin_ids: std::collections::HashSet<&str> =
        builtins.iter().map(|t| t.id.as_str()).collect();

    // Start with builtins, append user-added (not in the builtin set).
    let mut out = builtins.clone();
    for t in &settings.templates {
        if !t.builtin && !builtin_ids.contains(t.id.as_str()) {
            out.push(t.clone());
        }
    }
    out
}

/// Upsert a user template. Rejected for built-in ids with a clear error.
pub async fn upsert_user(app_data_dir: &Path, t: Template) -> anyhow::Result<()> {
    if t.builtin {
        return Err(anyhow::anyhow!("cannot modify built-in template"));
    }
    if is_builtin(&t.id) {
        return Err(anyhow::anyhow!(
            "cannot modify built-in template: id '{}' is reserved",
            t.id
        ));
    }
    if t.id.trim().is_empty() {
        return Err(anyhow::anyhow!("template id may not be empty"));
    }

    // Load the persisted (stripped) settings, rewrite the `templates`
    let settings = load(app_data_dir).await?;
    let mut user_only: Vec<Template> = settings
        .templates
        .into_iter()
        .filter(|x| !x.builtin)
        .collect();

    if let Some(slot) = user_only.iter_mut().find(|x| x.id == t.id) {
        *slot = t;
    } else {
        user_only.push(t);
    }

    let patch = serde_json::json!({ "templates": user_only });
    apply_patch(app_data_dir, patch).await?;
    Ok(())
}

// ---------------------------------------------------------------------------

/// Directories skipped during template copy. Matches the deny list used
const TEMPLATE_COPY_DENY: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    ".cache",
    ".venv",
    "venv",
];

/// Request body for creating a new project from a template.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct CreateProjectParams {
    /// Display name for the new project. Also used as the directory
    pub name: String,
    /// Absolute path to the parent directory that will hold the new
    pub parent: String,
    /// Template id; matched against built-ins first, then user templates.
    pub template_id: String,
    /// If true, run `git init` inside the new folder.
    pub init_git: bool,
    /// If true, create an empty `.env` with a single comment line.
    pub create_env: bool,
    /// Optional editor id to open the new project in. Resolution + launch
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_in_editor: Option<String>,
}

/// Canonicalize `path` and reject anything that escapes `$HOME`. Falls
fn canonicalize_within_home(path: &Path) -> anyhow::Result<PathBuf> {
    let canonical = std::fs::canonicalize(path)
        .map_err(|e| anyhow::anyhow!("canonicalize {}: {e}", path.display()))?;

    if let Some(home) = std::env::var_os("HOME") {
        let home_path = PathBuf::from(home);
        if let Ok(home_canonical) = std::fs::canonicalize(&home_path) {
            if !canonical.starts_with(&home_canonical) {
                // Allow tmp dirs so tests can run under /tmp /
                let allowed_tmp_prefixes: [&str; 4] = [
                    "/tmp",
                    "/private/tmp",
                    "/var/folders",
                    "/private/var/folders",
                ];
                let is_tmp = allowed_tmp_prefixes
                    .iter()
                    .any(|p| canonical.starts_with(p));
                if !is_tmp {
                    return Err(anyhow::anyhow!(
                        "parent {} escapes $HOME",
                        canonical.display()
                    ));
                }
            }
        }
    }
    Ok(canonical)
}

/// Copy `src` into `dest` recursively, skipping anything in
fn copy_template_tree(src: &Path, dest: &Path) -> anyhow::Result<u64> {
    if !src.is_dir() {
        return Err(anyhow::anyhow!(
            "template source {} is not a directory",
            src.display()
        ));
    }

    let mut copied: u64 = 0;
    // Hand-rolled BFS rather than recursion so a deep template can't
    let mut stack: Vec<(PathBuf, PathBuf)> = vec![(src.to_path_buf(), dest.to_path_buf())];

    while let Some((s, d)) = stack.pop() {
        // Create the destination directory if missing. `read_dir` below
        std::fs::create_dir_all(&d).map_err(|e| anyhow::anyhow!("mkdir {}: {e}", d.display()))?;

        let entries =
            std::fs::read_dir(&s).map_err(|e| anyhow::anyhow!("read_dir {}: {e}", s.display()))?;

        for entry in entries {
            let entry = entry.map_err(|e| anyhow::anyhow!("read entry in {}: {e}", s.display()))?;
            let name = entry.file_name();
            if let Some(name_str) = name.to_str() {
                if TEMPLATE_COPY_DENY.contains(&name_str) {
                    continue;
                }
            }

            let s_child = entry.path();
            let d_child = d.join(&name);
            let file_type = entry
                .file_type()
                .map_err(|e| anyhow::anyhow!("file_type {}: {e}", s_child.display()))?;

            if file_type.is_dir() {
                stack.push((s_child, d_child));
            } else if file_type.is_file() {
                std::fs::copy(&s_child, &d_child).map_err(|e| {
                    anyhow::anyhow!("copy {} -> {}: {e}", s_child.display(), d_child.display())
                })?;
                copied += 1;
            }
            // Symlinks are intentionally skipped - templates shouldn't
        }
    }

    Ok(copied)
}

/// Create a new project folder from a template.
pub async fn create_project(
    ctx: &AppContext,
    params: CreateProjectParams,
) -> anyhow::Result<ProjectId> {
    let name = params.name.trim();
    if name.is_empty() {
        return Err(anyhow::anyhow!("project name may not be empty"));
    }
    // Reject path separators in the name so `../../escape` can't pivot
    if name.contains('/') || name.contains('\\') {
        return Err(anyhow::anyhow!(
            "project name may not contain path separators"
        ));
    }

    let parent_path = PathBuf::from(&params.parent);
    let parent_canonical = canonicalize_within_home(&parent_path)?;
    if !parent_canonical.is_dir() {
        return Err(anyhow::anyhow!(
            "parent {} is not a directory",
            parent_canonical.display()
        ));
    }

    let dest = parent_canonical.join(name);
    if dest.exists() {
        return Err(anyhow::anyhow!(
            "destination already exists: {}",
            dest.display()
        ));
    }

    // Resolve the template. User templates may carry a real source path;
    let settings = load(&ctx.app_data_dir).await?;
    let all = list_all(&settings).await;
    let template = all
        .iter()
        .find(|t| t.id == params.template_id)
        .ok_or_else(|| anyhow::anyhow!("unknown template id: {}", params.template_id))?;

    // Create the destination up-front so every later step can assume it.
    std::fs::create_dir_all(&dest)
        .map_err(|e| anyhow::anyhow!("create {}: {e}", dest.display()))?;

    // Copy template contents if applicable.
    if !template.path.trim().is_empty() {
        let src = PathBuf::from(&template.path);
        if !src.exists() {
            return Err(anyhow::anyhow!(
                "template source missing on disk: {}",
                src.display()
            ));
        }
        copy_template_tree(&src, &dest)?;
    }

    // Optional `.env` with a single comment line. The comment makes the
    if params.create_env {
        let env_path = dest.join(".env");
        std::fs::write(
            &env_path,
            b"# atlas: fill in project secrets here, then remove this comment\n",
        )
        .map_err(|e| anyhow::anyhow!("write {}: {e}", env_path.display()))?;
    }

    // Optional `git init`. `git2::Repository::init` creates `.git/` in
    if params.init_git {
        git2::Repository::init(&dest)
            .map_err(|e| anyhow::anyhow!("git init {}: {e}", dest.display()))?;
    }

    // Register with the DB as a discovered repo. `classify` infers the
    let canonical_dest = std::fs::canonicalize(&dest)
        .map_err(|e| anyhow::anyhow!("canonicalize {}: {e}", dest.display()))?;
    let mut repo: DiscoveredRepo = classify(&canonical_dest);
    // `classify` sets `name` from the directory basename. Keep it as-is
    repo.path = canonical_dest;
    let id = ctx.db.upsert_discovered(&repo).await?;
    Ok(id)
}

/// Remove a user template by id. Built-in ids are rejected. Unknown ids
pub async fn remove_user(app_data_dir: &Path, id: &str) -> anyhow::Result<()> {
    if is_builtin(id) {
        return Err(anyhow::anyhow!("cannot modify built-in template"));
    }

    let settings = load(app_data_dir).await?;
    let filtered: Vec<Template> = settings
        .templates
        .into_iter()
        .filter(|t| !t.builtin && t.id != id)
        .collect();

    let patch = serde_json::json!({ "templates": filtered });
    apply_patch(app_data_dir, patch).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::path::PathBuf;

    fn unique_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        env::temp_dir().join(format!(
            "atlas-templates-{tag}-{}-{nanos}",
            std::process::id()
        ))
    }

    #[tokio::test]
    async fn builtin_list_matches_brief() {
        let b = builtin_templates();
        let ids: Vec<&str> = b.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["node-ts", "rust-cli", "python-uv", "go-service", "empty"]
        );
        assert!(b.iter().all(|t| t.builtin));
        assert!(b.iter().all(|t| t.path.is_empty()));
    }

    #[tokio::test]
    async fn list_all_surfaces_builtins_by_default() -> anyhow::Result<()> {
        let dir = unique_dir("list-default");
        std::fs::create_dir_all(&dir)?;
        let settings = load(&dir).await?;
        let all = list_all(&settings).await;
        assert!(all.iter().any(|t| t.id == "node-ts" && t.builtin));
        assert!(all.iter().any(|t| t.id == "empty" && t.builtin));
        assert_eq!(
            all.iter().filter(|t| t.builtin).count(),
            5,
            "expected 5 built-in templates"
        );
        std::fs::remove_dir_all(&dir).ok();
        Ok(())
    }

    #[tokio::test]
    async fn upsert_and_remove_user_template() -> anyhow::Result<()> {
        let dir = unique_dir("user-crud");
        std::fs::create_dir_all(&dir)?;
        let _ = load(&dir).await?;

        let mine = Template {
            id: "my-starter".into(),
            label: "My starter".into(),
            color: "#ff00aa".into(),
            hint: "bespoke".into(),
            path: "/tmp/my-starter".into(),
            builtin: false,
        };
        upsert_user(&dir, mine.clone()).await?;

        let settings = load(&dir).await?;
        let all = list_all(&settings).await;
        assert!(all.iter().any(|t| t.id == "my-starter" && !t.builtin));

        // Update in-place.
        let updated = Template {
            label: "My starter (v2)".into(),
            ..mine.clone()
        };
        upsert_user(&dir, updated).await?;
        let settings = load(&dir).await?;
        let all = list_all(&settings).await;
        let got = all.iter().find(|t| t.id == "my-starter").unwrap();
        assert_eq!(got.label, "My starter (v2)");

        // Remove.
        remove_user(&dir, "my-starter").await?;
        let settings = load(&dir).await?;
        let all = list_all(&settings).await;
        assert!(!all.iter().any(|t| t.id == "my-starter"));

        // Remove unknown is idempotent.
        remove_user(&dir, "ghost").await?;

        std::fs::remove_dir_all(&dir).ok();
        Ok(())
    }

    #[tokio::test]
    async fn cannot_modify_builtin_templates() -> anyhow::Result<()> {
        let dir = unique_dir("builtin-guard");
        std::fs::create_dir_all(&dir)?;
        let _ = load(&dir).await?;

        // Upsert with a builtin id → error.
        let fake = Template {
            id: "node-ts".into(),
            label: "hijack".into(),
            color: "#000".into(),
            hint: "".into(),
            path: "".into(),
            builtin: false,
        };
        let err = upsert_user(&dir, fake).await;
        assert!(err.is_err());
        assert!(format!("{}", err.unwrap_err()).contains("built-in"));

        // Upsert with builtin:true → error (even for a new id).
        let bad = Template {
            id: "something-new".into(),
            label: "bad".into(),
            color: "#000".into(),
            hint: "".into(),
            path: "".into(),
            builtin: true,
        };
        let err = upsert_user(&dir, bad).await;
        assert!(err.is_err());

        // Remove builtin id → error.
        let err = remove_user(&dir, "rust-cli").await;
        assert!(err.is_err());

        std::fs::remove_dir_all(&dir).ok();
        Ok(())
    }

    #[tokio::test]
    async fn upsert_rejects_empty_id() -> anyhow::Result<()> {
        let dir = unique_dir("empty-id");
        std::fs::create_dir_all(&dir)?;
        let _ = load(&dir).await?;

        let bad = Template {
            id: "   ".into(),
            label: "x".into(),
            color: "#000".into(),
            hint: "".into(),
            path: "".into(),
            builtin: false,
        };
        let err = upsert_user(&dir, bad).await;
        assert!(err.is_err());

        std::fs::remove_dir_all(&dir).ok();
        Ok(())
    }

    use crate::storage::Db;

    async fn ctx_with_tempdir(tag: &str) -> anyhow::Result<(AppContext, PathBuf)> {
        let dir = unique_dir(tag);
        std::fs::create_dir_all(&dir)?;
        let db = Db::open_in_memory().await?;
        let ctx = AppContext {
            app_data_dir: dir.clone(),
            db,
        };
        // Prime settings.json so subsequent loads don't have to race.
        let _ = load(&ctx.app_data_dir).await?;
        Ok((ctx, dir))
    }

    #[tokio::test]
    async fn create_project_from_builtin_empty_inits_git() -> anyhow::Result<()> {
        let (ctx, app_dir) = ctx_with_tempdir("create-empty").await?;
        let workspace = unique_dir("create-empty-ws");
        std::fs::create_dir_all(&workspace)?;

        let id = create_project(
            &ctx,
            CreateProjectParams {
                name: "alpha".into(),
                parent: workspace.to_string_lossy().to_string(),
                template_id: "empty".into(),
                init_git: true,
                create_env: false,
                open_in_editor: None,
            },
        )
        .await?;

        // The destination dir exists with a `.git`.
        let dest = workspace.join("alpha");
        assert!(dest.is_dir(), "dest missing: {}", dest.display());
        assert!(
            dest.join(".git").exists(),
            "expected .git under {}",
            dest.display()
        );

        // The project made it into the index with the same id.
        let p = ctx.db.get_project(&id).await?.expect("project registered");
        assert_eq!(p.name, "alpha");

        std::fs::remove_dir_all(&workspace).ok();
        std::fs::remove_dir_all(&app_dir).ok();
        Ok(())
    }

    #[tokio::test]
    async fn create_project_copies_user_template_and_skips_deny_list() -> anyhow::Result<()> {
        let (ctx, app_dir) = ctx_with_tempdir("create-copy").await?;

        // Build a fake user template on disk with one real file and a
        let template_src = unique_dir("create-copy-template");
        std::fs::create_dir_all(template_src.join("src"))?;
        std::fs::write(template_src.join("src/index.ts"), b"export const x = 1;\n")?;
        std::fs::write(template_src.join("package.json"), b"{\"name\":\"tmpl\"}\n")?;
        std::fs::create_dir_all(template_src.join("node_modules/react"))?;
        std::fs::write(
            template_src.join("node_modules/react/package.json"),
            b"{\"name\":\"react\"}\n",
        )?;
        std::fs::create_dir_all(template_src.join(".git"))?;
        std::fs::write(template_src.join(".git/HEAD"), b"ref: refs/heads/main\n")?;
        std::fs::create_dir_all(template_src.join("dist"))?;
        std::fs::write(template_src.join("dist/bundle.js"), b"// garbage\n")?;

        // Register as a user template.
        upsert_user(
            &ctx.app_data_dir,
            Template {
                id: "my-user-template".into(),
                label: "Mine".into(),
                color: "#123456".into(),
                hint: "custom".into(),
                path: template_src.to_string_lossy().to_string(),
                builtin: false,
            },
        )
        .await?;

        let workspace = unique_dir("create-copy-ws");
        std::fs::create_dir_all(&workspace)?;

        let id = create_project(
            &ctx,
            CreateProjectParams {
                name: "beta".into(),
                parent: workspace.to_string_lossy().to_string(),
                template_id: "my-user-template".into(),
                init_git: false,
                create_env: true,
                open_in_editor: None,
            },
        )
        .await?;

        let dest = workspace.join("beta");
        assert!(dest.join("src/index.ts").is_file(), "src/index.ts missing");
        assert!(dest.join("package.json").is_file(), "package.json missing");
        assert!(
            dest.join(".env").is_file(),
            "expected .env from create_env=true"
        );

        // Deny-listed directories must NOT have been copied.
        assert!(
            !dest.join("node_modules").exists(),
            "node_modules should have been skipped"
        );
        assert!(!dest.join(".git").exists(), ".git should have been skipped");
        assert!(!dest.join("dist").exists(), "dist should have been skipped");

        // Project registered.
        assert!(ctx.db.get_project(&id).await?.is_some());

        std::fs::remove_dir_all(&template_src).ok();
        std::fs::remove_dir_all(&workspace).ok();
        std::fs::remove_dir_all(&app_dir).ok();
        Ok(())
    }

    #[tokio::test]
    async fn create_project_rejects_existing_destination() -> anyhow::Result<()> {
        let (ctx, app_dir) = ctx_with_tempdir("create-exists").await?;
        let workspace = unique_dir("create-exists-ws");
        std::fs::create_dir_all(workspace.join("taken"))?;

        let err = create_project(
            &ctx,
            CreateProjectParams {
                name: "taken".into(),
                parent: workspace.to_string_lossy().to_string(),
                template_id: "empty".into(),
                init_git: false,
                create_env: false,
                open_in_editor: None,
            },
        )
        .await;
        assert!(err.is_err(), "existing destination must be rejected");
        assert!(format!("{}", err.unwrap_err()).contains("already exists"));

        std::fs::remove_dir_all(&workspace).ok();
        std::fs::remove_dir_all(&app_dir).ok();
        Ok(())
    }

    #[tokio::test]
    async fn create_project_rejects_path_separators_in_name() -> anyhow::Result<()> {
        let (ctx, app_dir) = ctx_with_tempdir("create-sep").await?;
        let workspace = unique_dir("create-sep-ws");
        std::fs::create_dir_all(&workspace)?;

        let err = create_project(
            &ctx,
            CreateProjectParams {
                name: "../escape".into(),
                parent: workspace.to_string_lossy().to_string(),
                template_id: "empty".into(),
                init_git: false,
                create_env: false,
                open_in_editor: None,
            },
        )
        .await;
        assert!(err.is_err(), "path separators in name must be rejected");

        std::fs::remove_dir_all(&workspace).ok();
        std::fs::remove_dir_all(&app_dir).ok();
        Ok(())
    }

    #[tokio::test]
    async fn create_project_rejects_unknown_template() -> anyhow::Result<()> {
        let (ctx, app_dir) = ctx_with_tempdir("create-unk-tmpl").await?;
        let workspace = unique_dir("create-unk-tmpl-ws");
        std::fs::create_dir_all(&workspace)?;

        let err = create_project(
            &ctx,
            CreateProjectParams {
                name: "new".into(),
                parent: workspace.to_string_lossy().to_string(),
                template_id: "never-existed".into(),
                init_git: false,
                create_env: false,
                open_in_editor: None,
            },
        )
        .await;
        assert!(err.is_err(), "unknown template must be rejected");

        std::fs::remove_dir_all(&workspace).ok();
        std::fs::remove_dir_all(&app_dir).ok();
        Ok(())
    }
}
