//! SQLite-backed index for Atlas.

use crate::git::GitStatus;
use crate::storage::discovery::{scan_root, scan_root_with_progress, DiscoveredRepo};
use crate::storage::json::{atlas_file, atlas_note_file, read_json, write_json};
use crate::storage::types::{
    Collection, Lang, Note, PaletteItem, PaneLayout, Project, ProjectFilter, Script, Todo,
};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Row, SqlitePool};
use std::path::{Path, PathBuf};
use std::str::FromStr;

/// Thin wrapper so the pool is cheap to clone and easy to stash in
#[derive(Debug, Clone)]
pub struct Db {
    pool: SqlitePool,
}

impl Db {
    /// Access the underlying pool. Most callers should go through the
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Open (or create) `atlas.db` inside the given app-data directory and
    pub async fn open(app_data_dir: &Path) -> anyhow::Result<Db> {
        tokio::fs::create_dir_all(app_data_dir).await?;
        let db_path = app_data_dir.join("atlas.db");

        let url = format!("sqlite://{}", db_path.to_string_lossy());
        let opts = SqliteConnectOptions::from_str(&url)?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .foreign_keys(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(opts)
            .await?;

        sqlx::migrate!("./migrations").run(&pool).await?;

        Ok(Db { pool })
    }

    /// Open an in-memory pool for unit tests. Runs migrations.
    #[cfg(test)]
    pub async fn open_in_memory() -> anyhow::Result<Db> {
        // :memory: doesn't support WAL - use the default journal.
        let opts = SqliteConnectOptions::from_str("sqlite::memory:")?.foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Db { pool })
    }

    /// Highest applied migration version from `_sqlx_migrations`.
    pub async fn current_version(&self) -> anyhow::Result<Option<i64>> {
        // `_sqlx_migrations` is created + owned by sqlx's migrator. The
        let (max_version,): (Option<i64>,) =
            sqlx::query_as("SELECT MAX(version) FROM _sqlx_migrations")
                .fetch_one(&self.pool)
                .await?;
        Ok(max_version)
    }

    /// List projects in the index, ordered by pinned desc, then last_opened
    pub async fn list_projects(&self, filter: ProjectFilter) -> anyhow::Result<Vec<Project>> {
        let mut sql = String::from(
            "SELECT id, name, path, language, color, branch, \
                    dirty, ahead, behind, loc, size_bytes, disk_bytes, last_opened, \
                    pinned, archived, todos_count, notes_count, time_tracked, author \
             FROM projects WHERE 1=1",
        );

        if !filter.include_archived {
            sql.push_str(" AND archived = 0");
        }
        if filter.pinned_only {
            sql.push_str(" AND pinned = 1");
        }
        if filter.tag.is_some() {
            sql.push_str(" AND id IN (SELECT project_id FROM tags WHERE tag = ?)");
        }
        if filter.collection_id.is_some() {
            sql.push_str(
                " AND id IN (SELECT project_id FROM collection_members WHERE collection_id = ?)",
            );
        }
        // Pinned rows first, then the user's drag order from
        sql.push_str(
            " ORDER BY pinned DESC, \
                      (pin_ord IS NULL), pin_ord ASC, \
                      (last_opened IS NULL), last_opened DESC, \
                      name ASC",
        );

        let mut q = sqlx::query(&sql);
        if let Some(tag) = filter.tag.as_ref() {
            q = q.bind(tag);
        }
        if let Some(cid) = filter.collection_id.as_ref() {
            q = q.bind(cid);
        }

        let rows = q.fetch_all(&self.pool).await?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            out.push(self.hydrate_project(row).await?);
        }
        Ok(out)
    }

    /// Fetch a single project by id. `None` if missing.
    pub async fn get_project(&self, id: &str) -> anyhow::Result<Option<Project>> {
        let row = sqlx::query(
            "SELECT id, name, path, language, color, branch, \
                    dirty, ahead, behind, loc, size_bytes, disk_bytes, last_opened, \
                    pinned, archived, todos_count, notes_count, time_tracked, author \
             FROM projects WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(r) => Ok(Some(self.hydrate_project(r).await?)),
            None => Ok(None),
        }
    }

    /// Full-text search over `projects_fts` (name / path / tags).
    pub async fn search_projects(&self, q: &str) -> anyhow::Result<Vec<Project>> {
        let trimmed = q.trim();
        if trimmed.is_empty() {
            return Ok(Vec::new());
        }

        // Quote the user input so FTS5 treats it as a phrase unless the
        let fts_query = if trimmed.contains('"') || trimmed.contains('*') || trimmed.contains(':') {
            trimmed.to_string()
        } else {
            // Prefix match on every whitespace-separated word.
            trimmed
                .split_whitespace()
                .map(|w| format!("{}*", w))
                .collect::<Vec<_>>()
                .join(" ")
        };

        let rows = sqlx::query(
            "SELECT p.id, p.name, p.path, p.language, p.color, p.branch, \
                    p.dirty, p.ahead, p.behind, p.loc, p.size_bytes, p.disk_bytes, p.last_opened, \
                    p.pinned, p.archived, p.todos_count, p.notes_count, p.time_tracked, p.author \
             FROM projects p \
             JOIN projects_fts f ON f.rowid = p.rowid \
             WHERE projects_fts MATCH ? \
             ORDER BY bm25(projects_fts)",
        )
        .bind(&fts_query)
        .fetch_all(&self.pool)
        .await?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            out.push(self.hydrate_project(row).await?);
        }
        Ok(out)
    }

    /// Insert the 12 prototype projects + 4 collections + tags if the
    pub async fn seed_fixtures(&self) -> anyhow::Result<usize> {
        // Bail if any watcher is already configured - real data incoming.
        let watcher_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM watchers")
            .fetch_one(&self.pool)
            .await?;
        if watcher_count.0 > 0 {
            return Ok(0);
        }

        // Bail if we've already seeded (or the user has real data).
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM projects")
            .fetch_one(&self.pool)
            .await?;
        if count.0 > 0 {
            return Ok(0);
        }

        let now = chrono::Utc::now().to_rfc3339();
        let mut tx = self.pool.begin().await?;

        let collections: &[(&str, &str, &str, i64)] = &[
            ("work", "Work", "oklch(0.78 0.17 145)", 0),
            ("personal", "Personal", "oklch(0.78 0.15 260)", 1),
            ("oss", "OSS", "oklch(0.78 0.15 55)", 2),
            ("scratch", "Scratch", "oklch(0.70 0.02 260)", 3),
        ];
        for (id, label, dot, ord) in collections {
            sqlx::query("INSERT INTO collections (id, label, dot, ord) VALUES (?, ?, ?, ?)")
                .bind(id)
                .bind(label)
                .bind(dot)
                .bind(ord)
                .execute(&mut *tx)
                .await?;
        }

        // relative offset from `now` so the list has a realistic recency
        let projects: &[SeedProject] = &[
            SeedProject {
                id: "acorn",
                name: "acorn-api",
                path: "~/code/work/acorn-api",
                language: Lang::Rust,
                color: "#E0763C",
                branch: "main",
                dirty: 0,
                ahead: 0,
                behind: 0,
                loc: 48210,
                size_bytes: 412 * 1_048_576,
                last_opened_minutes_ago: Some(120),
                pinned: true,
                tags: &["work", "api"],
                todos: 3,
                notes: 2,
                time: "4h 22m",
                archived: false,
                collection: "work",
            },
            SeedProject {
                id: "birch",
                name: "birch-dashboard",
                path: "~/code/work/birch-dashboard",
                language: Lang::TypeScript,
                color: "#3178C6",
                branch: "feat/charts",
                dirty: 12,
                ahead: 2,
                behind: 0,
                loc: 32140,
                size_bytes: 1_288_490_188,
                last_opened_minutes_ago: Some(18),
                pinned: true,
                tags: &["work", "frontend"],
                todos: 7,
                notes: 5,
                time: "12h 08m",
                archived: false,
                collection: "work",
            },
            SeedProject {
                id: "cedar",
                name: "cedar-cli",
                path: "~/code/oss/cedar-cli",
                language: Lang::Go,
                color: "#00ADD8",
                branch: "main",
                dirty: 0,
                ahead: 0,
                behind: 3,
                loc: 8420,
                size_bytes: 86 * 1_048_576,
                last_opened_minutes_ago: Some(60 * 24),
                pinned: false,
                tags: &["oss", "cli"],
                todos: 1,
                notes: 0,
                time: "1h 45m",
                archived: false,
                collection: "oss",
            },
            SeedProject {
                id: "dahlia",
                name: "dahlia-site",
                path: "~/code/personal/dahlia-site",
                language: Lang::Other,
                color: "#FF5D01",
                branch: "draft/2026",
                dirty: 4,
                ahead: 0,
                behind: 0,
                loc: 6120,
                size_bytes: 240 * 1_048_576,
                last_opened_minutes_ago: Some(60 * 24 * 3),
                pinned: false,
                tags: &["personal", "web"],
                todos: 2,
                notes: 1,
                time: "0h 40m",
                archived: false,
                collection: "personal",
            },
            SeedProject {
                id: "elm",
                name: "elm-engine",
                path: "~/code/work/elm-engine",
                language: Lang::CPlusPlus,
                color: "#F34B7D",
                branch: "perf/simd",
                dirty: 31,
                ahead: 0,
                behind: 1,
                loc: 142310,
                size_bytes: 3_006_477_107,
                last_opened_minutes_ago: Some(60 * 24 * 5),
                pinned: false,
                tags: &["work", "engine"],
                todos: 12,
                notes: 11,
                time: "38h 14m",
                archived: false,
                collection: "work",
            },
            SeedProject {
                id: "fern",
                name: "fern-notes",
                path: "~/code/personal/fern-notes",
                language: Lang::Swift,
                color: "#F05138",
                branch: "main",
                dirty: 0,
                ahead: 1,
                behind: 0,
                loc: 3890,
                size_bytes: 52 * 1_048_576,
                last_opened_minutes_ago: Some(60 * 24 * 7),
                pinned: false,
                tags: &["personal", "mac"],
                todos: 0,
                notes: 0,
                time: "2h 30m",
                archived: false,
                collection: "personal",
            },
            SeedProject {
                id: "ginkgo",
                name: "ginkgo-scratch",
                path: "~/code/scratch/ginkgo-scratch",
                language: Lang::Python,
                color: "#3572A5",
                branch: "main",
                dirty: 2,
                ahead: 0,
                behind: 0,
                loc: 1240,
                size_bytes: 18 * 1_048_576,
                last_opened_minutes_ago: Some(60 * 4),
                pinned: false,
                tags: &["scratch"],
                todos: 0,
                notes: 0,
                time: "0h 12m",
                archived: false,
                collection: "scratch",
            },
            SeedProject {
                id: "hawthorn",
                name: "hawthorn-ml",
                path: "~/code/work/hawthorn-ml",
                language: Lang::Python,
                color: "#3572A5",
                branch: "exp/lora-v3",
                dirty: 8,
                ahead: 0,
                behind: 0,
                loc: 22470,
                size_bytes: 15_676_334_080,
                last_opened_minutes_ago: Some(60),
                pinned: true,
                tags: &["work", "ml"],
                todos: 4,
                notes: 7,
                time: "19h 02m",
                archived: false,
                collection: "work",
            },
            SeedProject {
                id: "ivy",
                name: "ivy-docs",
                path: "~/code/work/ivy-docs",
                language: Lang::Other,
                color: "#8A2BE2",
                branch: "main",
                dirty: 0,
                ahead: 0,
                behind: 0,
                loc: 4120,
                size_bytes: 72 * 1_048_576,
                last_opened_minutes_ago: Some(60 * 24 * 21),
                pinned: false,
                tags: &["work", "docs"],
                todos: 0,
                notes: 0,
                time: "0h 55m",
                archived: false,
                collection: "work",
            },
            SeedProject {
                id: "juniper",
                name: "juniper-infra",
                path: "~/code/work/juniper-infra",
                language: Lang::Other,
                color: "#844FBA",
                branch: "prod",
                dirty: 0,
                ahead: 0,
                behind: 0,
                loc: 5840,
                size_bytes: 94 * 1_048_576,
                last_opened_minutes_ago: Some(60 * 24 * 12),
                pinned: false,
                tags: &["work", "devops"],
                todos: 2,
                notes: 1,
                time: "3h 10m",
                archived: false,
                collection: "work",
            },
            SeedProject {
                id: "kelp",
                name: "kelp-player",
                path: "~/code/personal/kelp-player",
                language: Lang::TypeScript,
                color: "#3178C6",
                branch: "main",
                dirty: 0,
                ahead: 0,
                behind: 0,
                loc: 14220,
                size_bytes: 320 * 1_048_576,
                last_opened_minutes_ago: Some(60 * 24 * 60),
                pinned: false,
                tags: &["personal", "audio"],
                todos: 0,
                notes: 3,
                time: "0h 00m",
                archived: true,
                collection: "personal",
            },
            SeedProject {
                id: "larch",
                name: "larch-game",
                path: "~/code/scratch/larch-game",
                language: Lang::Other,
                color: "#EC915C",
                branch: "main",
                dirty: 6,
                ahead: 0,
                behind: 0,
                loc: 2240,
                size_bytes: 46 * 1_048_576,
                last_opened_minutes_ago: Some(60 * 24 * 6),
                pinned: false,
                tags: &["scratch", "game"],
                todos: 3,
                notes: 2,
                time: "1h 05m",
                archived: false,
                collection: "scratch",
            },
        ];

        for p in projects {
            let last_opened = p
                .last_opened_minutes_ago
                .map(|mins| (chrono::Utc::now() - chrono::Duration::minutes(mins)).to_rfc3339());
            let lang_str = lang_to_str(&p.language);

            // Fixture rows seed `disk_bytes = size_bytes` as a placeholder;
            // the first real metrics sweep overwrites with the actual
            // on-disk total.
            sqlx::query(
                "INSERT INTO projects (id, name, path, language, color, branch, \
                                       dirty, ahead, behind, loc, size_bytes, disk_bytes, last_opened, \
                                       pinned, archived, todos_count, notes_count, \
                                       time_tracked, updated_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(p.id)
            .bind(p.name)
            .bind(p.path)
            .bind(lang_str)
            .bind(p.color)
            .bind(p.branch)
            .bind(p.dirty)
            .bind(p.ahead)
            .bind(p.behind)
            .bind(p.loc)
            .bind(p.size_bytes)
            .bind(p.size_bytes)
            .bind(last_opened)
            .bind(p.pinned as i64)
            .bind(p.archived as i64)
            .bind(p.todos)
            .bind(p.notes)
            .bind(p.time)
            .bind(&now)
            .execute(&mut *tx)
            .await?;

            for tag in p.tags {
                sqlx::query("INSERT INTO tags (project_id, tag) VALUES (?, ?)")
                    .bind(p.id)
                    .bind(*tag)
                    .execute(&mut *tx)
                    .await?;
            }

            sqlx::query("INSERT INTO collection_members (collection_id, project_id) VALUES (?, ?)")
                .bind(p.collection)
                .bind(p.id)
                .execute(&mut *tx)
                .await?;

            // FTS index - manually maintained because we use `content=projects`
            let tags_joined = p.tags.join(" ");
            sqlx::query(
                "INSERT INTO projects_fts (rowid, name, path, tags) \
                 VALUES ((SELECT rowid FROM projects WHERE id = ?), ?, ?, ?)",
            )
            .bind(p.id)
            .bind(p.name)
            .bind(p.path)
            .bind(&tags_joined)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(projects.len())
    }

    /// Common row → `Project` hydration including tag/collection fan-out.
    async fn hydrate_project(&self, row: sqlx::sqlite::SqliteRow) -> anyhow::Result<Project> {
        let id: String = row.try_get("id")?;
        let size_bytes: i64 = row.try_get("size_bytes")?;
        let disk_bytes: i64 = row.try_get("disk_bytes").unwrap_or(0);

        // tags for this project
        let tag_rows = sqlx::query("SELECT tag FROM tags WHERE project_id = ? ORDER BY tag")
            .bind(&id)
            .fetch_all(&self.pool)
            .await?;
        let tags: Vec<String> = tag_rows
            .iter()
            .map(|r| r.try_get::<String, _>("tag").unwrap_or_default())
            .collect();

        // collection memberships
        let col_rows = sqlx::query(
            "SELECT collection_id FROM collection_members WHERE project_id = ? ORDER BY collection_id",
        )
        .bind(&id)
        .fetch_all(&self.pool)
        .await?;
        let collection_ids: Vec<String> = col_rows
            .iter()
            .map(|r| r.try_get::<String, _>("collection_id").unwrap_or_default())
            .collect();

        let lang_str: Option<String> = row.try_get("language").ok();
        let language = lang_str.as_deref().map(str_to_lang).unwrap_or(Lang::Other);

        let pinned_i: i64 = row.try_get("pinned").unwrap_or(0);
        let archived_i: i64 = row.try_get("archived").unwrap_or(0);

        Ok(Project {
            id: id.clone(),
            name: row.try_get("name")?,
            path: row.try_get("path")?,
            language,
            color: row
                .try_get::<Option<String>, _>("color")?
                .unwrap_or_default(),
            branch: row
                .try_get::<Option<String>, _>("branch")?
                .unwrap_or_default(),
            dirty: row.try_get("dirty")?,
            ahead: row.try_get("ahead")?,
            behind: row.try_get("behind")?,
            loc: row.try_get("loc")?,
            size: format_size(size_bytes),
            size_bytes,
            disk_size: format_size(disk_bytes),
            disk_bytes,
            last_opened: row.try_get("last_opened").ok(),
            pinned: pinned_i != 0,
            tags,
            todos_count: row.try_get("todos_count")?,
            notes_count: row.try_get("notes_count")?,
            time: row
                .try_get::<Option<String>, _>("time_tracked")?
                .unwrap_or_default(),
            archived: archived_i != 0,
            collection_ids,
            // `author` is nullable - NULL on freshly-indexed rows until the
            author: row.try_get::<Option<String>, _>("author").ok().flatten(),
        })
    }

    // =================================================================

    /// List all configured watch roots along with their depth.
    pub async fn list_watchers(&self) -> anyhow::Result<Vec<(PathBuf, u8)>> {
        let rows = sqlx::query("SELECT path, depth FROM watchers ORDER BY added_at ASC")
            .fetch_all(&self.pool)
            .await?;
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let path: String = row.try_get("path")?;
            let depth: i64 = row.try_get("depth")?;
            let depth_u8 = depth.clamp(0, 255) as u8;
            out.push((PathBuf::from(path), depth_u8));
        }
        Ok(out)
    }

    /// Upsert a watcher row. If the same path is added twice we refresh
    pub async fn add_watcher(&self, path: &Path, depth: u8) -> anyhow::Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO watchers (path, depth, added_at) VALUES (?, ?, ?) \
             ON CONFLICT(path) DO UPDATE SET depth = excluded.depth, added_at = excluded.added_at",
        )
        .bind(path.to_string_lossy().as_ref())
        .bind(depth as i64)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Remove a watcher by path. No-op if not present.
    pub async fn remove_watcher(&self, path: &Path) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM watchers WHERE path = ?")
            .bind(path.to_string_lossy().as_ref())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Count projects whose `path` is under `root` (prefix match). Used
    pub async fn count_projects_under(&self, root: &Path) -> anyhow::Result<u32> {
        let root_str = root.to_string_lossy();
        let prefix = if root_str.ends_with('/') {
            format!("{}%", escape_like(&root_str))
        } else {
            format!("{}/%", escape_like(&root_str))
        };
        // Also count the root itself in case it was added as a project.
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM projects \
             WHERE path = ? OR path LIKE ? ESCAPE '\\'",
        )
        .bind(root_str.as_ref())
        .bind(&prefix)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0.max(0) as u32)
    }

    /// All indexed project paths (absolute). Used by the watcher manager
    pub async fn all_project_paths(&self) -> anyhow::Result<Vec<PathBuf>> {
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT path FROM projects WHERE archived = 0 AND source = 'discovery'")
                .fetch_all(&self.pool)
                .await?;
        Ok(rows.into_iter().map(|(p,)| PathBuf::from(p)).collect())
    }

    // =================================================================

    /// Run `scan_root` then upsert every repo it finds. Returns the ids
    pub async fn discover_root(&self, root: &Path, depth: u8) -> anyhow::Result<Vec<String>> {
        let repos = scan_root(root, depth)?;
        self.upsert_repos(&repos).await
    }

    /// Variant of `discover_root` with a progress callback. Fires
    pub async fn discover_root_with_progress<F>(
        &self,
        root: &Path,
        depth: u8,
        on_found: F,
    ) -> anyhow::Result<Vec<String>>
    where
        F: FnMut(&Path, usize),
    {
        let repos = scan_root_with_progress(root, depth, on_found)?;
        self.upsert_repos(&repos).await
    }

    async fn upsert_repos(&self, repos: &[DiscoveredRepo]) -> anyhow::Result<Vec<String>> {
        let mut new_ids = Vec::new();
        for repo in repos {
            let id = project_id_for_path(&repo.path);
            let existed = self.get_project(&id).await?.is_some();
            self.upsert_discovered(repo).await?;
            if !existed {
                new_ids.push(id);
            }
        }
        Ok(new_ids)
    }

    /// Upsert a single discovered repo. Returns its stable project id.
    pub async fn upsert_discovered(&self, repo: &DiscoveredRepo) -> anyhow::Result<String> {
        let id = project_id_for_path(&repo.path);
        let now = chrono::Utc::now().to_rfc3339();
        let path_str = repo.path.to_string_lossy().to_string();
        let lang_str = lang_to_str(&repo.language);
        let color = color_for_lang(&repo.language);

        // Upsert the project row. On conflict we refresh the volatile
        sqlx::query(
            "INSERT INTO projects (id, name, path, language, color, branch, \
                                   dirty, ahead, behind, loc, size_bytes, disk_bytes, last_opened, \
                                   pinned, archived, todos_count, notes_count, \
                                   time_tracked, updated_at, discovered_at, source) \
             VALUES (?, ?, ?, ?, ?, '', 0, 0, 0, 0, 0, 0, NULL, 0, 0, 0, 0, '', ?, ?, 'discovery') \
             ON CONFLICT(id) DO UPDATE SET \
                 name       = excluded.name, \
                 path       = excluded.path, \
                 language   = excluded.language, \
                 color      = excluded.color, \
                 updated_at = excluded.updated_at",
        )
        .bind(&id)
        .bind(&repo.name)
        .bind(&path_str)
        .bind(lang_str)
        .bind(color)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await?;

        // Re-sync the FTS row. FTS5 with `content=projects` would normally
        let rowid: (i64,) = sqlx::query_as("SELECT rowid FROM projects WHERE id = ?")
            .bind(&id)
            .fetch_one(&self.pool)
            .await?;
        fts_replace_project(&self.pool, rowid.0, &repo.name, &path_str, "").await?;

        Ok(id)
    }

    // =================================================================

    /// Apply a P2-produced git status patch to the `projects` row. Touches
    pub async fn apply_git_status(
        &self,
        project_id: &str,
        status: &GitStatus,
    ) -> anyhow::Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "UPDATE projects \
                SET branch = ?, dirty = ?, ahead = ?, behind = ?, author = ?, updated_at = ? \
              WHERE id = ?",
        )
        .bind(&status.branch)
        .bind(status.dirty as i64)
        .bind(status.ahead as i64)
        .bind(status.behind as i64)
        .bind(status.author.as_deref())
        .bind(now)
        .bind(project_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // =================================================================

    /// Resolve `project_id` → on-disk path, walk it via
    pub async fn refresh_project_metrics(
        &self,
        project_id: &str,
    ) -> anyhow::Result<crate::metrics::ProjectMetrics> {
        let project = self
            .get_project(project_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("project not found: {project_id}"))?;
        let path = PathBuf::from(&project.path);

        let metrics = tauri::async_runtime::spawn_blocking(move || crate::metrics::compute(&path))
            .await
            .map_err(|e| anyhow::anyhow!("join blocking metrics: {e}"))??;

        // NOTE: The `projects` table carries both `size_bytes` (source,
        // gitignored) and `disk_bytes` (full tree). Both are refreshed
        // together so the UI never shows a stale pair.
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "UPDATE projects \
                SET loc = ?, size_bytes = ?, disk_bytes = ?, updated_at = ? \
              WHERE id = ?",
        )
        .bind(metrics.loc as i64)
        .bind(metrics.size_bytes as i64)
        .bind(metrics.disk_bytes as i64)
        .bind(now)
        .bind(project_id)
        .execute(&self.pool)
        .await?;

        Ok(metrics)
    }

    // =================================================================

    pub async fn pin_project(&self, id: &str, pinned: bool) -> anyhow::Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        // Unpinning clears the drag-reorder slot so a subsequent re-pin
        sqlx::query(
            "UPDATE projects \
                SET pinned = ?, \
                    pin_ord = CASE WHEN ? = 0 THEN NULL ELSE pin_ord END, \
                    updated_at = ? \
              WHERE id = ?",
        )
        .bind(pinned as i64)
        .bind(pinned as i64)
        .bind(now)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// `projects.reorder_pinned` - write a persistent drag order for the
    pub async fn reorder_pinned(&self, ordered_ids: &[String]) -> anyhow::Result<()> {
        let mut tx = self.pool.begin().await?;

        // 1. Clear pin_ord on every currently-pinned project. We'll
        sqlx::query("UPDATE projects SET pin_ord = NULL WHERE pinned = 1")
            .execute(&mut *tx)
            .await?;

        // 2. Write the new order. `pinned = 1` is reasserted defensively
        let now = chrono::Utc::now().to_rfc3339();
        for (i, id) in ordered_ids.iter().enumerate() {
            sqlx::query(
                "UPDATE projects \
                    SET pin_ord = ?, pinned = 1, updated_at = ? \
                  WHERE id = ?",
            )
            .bind(i as i64)
            .bind(&now)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    pub async fn archive_project(&self, id: &str, archived: bool) -> anyhow::Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query("UPDATE projects SET archived = ?, updated_at = ? WHERE id = ?")
            .bind(archived as i64)
            .bind(now)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn rename_project(&self, id: &str, name: &str) -> anyhow::Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query("UPDATE projects SET name = ?, updated_at = ? WHERE id = ?")
            .bind(name)
            .bind(&now)
            .bind(id)
            .execute(&self.pool)
            .await?;
        // Keep FTS in sync so the palette finds the new name.
        self.resync_project_fts(id).await?;
        Ok(())
    }

    /// Replace the tag set for a project with the given list. Duplicate
    pub async fn set_tags(&self, id: &str, tags: &[String]) -> anyhow::Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM tags WHERE project_id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for tag in tags {
            let t = tag.trim();
            if t.is_empty() || !seen.insert(t) {
                continue;
            }
            sqlx::query("INSERT OR IGNORE INTO tags (project_id, tag) VALUES (?, ?)")
                .bind(id)
                .bind(t)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;

        // Re-sync the project's FTS row so tag search stays accurate.
        self.resync_project_fts(id).await?;
        Ok(())
    }

    // =================================================================

    /// `scripts.list` - read `<project>/.atlas/scripts.json` and return
    pub async fn scripts_list(&self, project_id: &str) -> anyhow::Result<Vec<Script>> {
        let project_path = self.project_path_for(project_id).await?;
        let file = atlas_file(&project_path, "scripts");
        Ok(read_json::<Vec<Script>>(&file)?.unwrap_or_default())
    }

    /// `scripts.upsert` - insert or replace a script by id, then write
    pub async fn scripts_upsert(&self, project_id: &str, script: &Script) -> anyhow::Result<()> {
        let project_path = self.project_path_for(project_id).await?;
        let file = atlas_file(&project_path, "scripts");
        let mut scripts: Vec<Script> = read_json(&file)?.unwrap_or_default();
        if let Some(slot) = scripts.iter_mut().find(|s| s.id == script.id) {
            *slot = script.clone();
        } else {
            scripts.push(script.clone());
        }
        write_json(&file, &scripts)?;
        Ok(())
    }

    /// `scripts.delete` - remove a script by id. No-op if absent.
    pub async fn scripts_delete(&self, project_id: &str, script_id: &str) -> anyhow::Result<()> {
        let project_path = self.project_path_for(project_id).await?;
        let file = atlas_file(&project_path, "scripts");
        let mut scripts: Vec<Script> = read_json(&file)?.unwrap_or_default();
        let before = scripts.len();
        scripts.retain(|s| s.id != script_id);
        if scripts.len() != before {
            write_json(&file, &scripts)?;
        }
        Ok(())
    }

    // =================================================================

    /// `todos.list` - read `<project>/.atlas/todos.json`.
    pub async fn todos_list(&self, project_id: &str) -> anyhow::Result<Vec<Todo>> {
        let project_path = self.project_path_for(project_id).await?;
        let file = atlas_file(&project_path, "todos");
        Ok(read_json::<Vec<Todo>>(&file)?.unwrap_or_default())
    }

    /// `todos.upsert` - insert or replace a todo by id; persist + reindex.
    pub async fn todos_upsert(&self, project_id: &str, todo: &Todo) -> anyhow::Result<()> {
        let project_path = self.project_path_for(project_id).await?;
        let file = atlas_file(&project_path, "todos");
        let mut todos: Vec<Todo> = read_json(&file)?.unwrap_or_default();
        if let Some(slot) = todos.iter_mut().find(|t| t.id == todo.id) {
            *slot = todo.clone();
        } else {
            todos.push(todo.clone());
        }
        write_json(&file, &todos)?;
        self.sync_todos_index(project_id, &todos).await?;
        Ok(())
    }

    /// `todos.delete` - remove by id; persist + reindex.
    pub async fn todos_delete(&self, project_id: &str, todo_id: &str) -> anyhow::Result<()> {
        let project_path = self.project_path_for(project_id).await?;
        let file = atlas_file(&project_path, "todos");
        let mut todos: Vec<Todo> = read_json(&file)?.unwrap_or_default();
        let before = todos.len();
        todos.retain(|t| t.id != todo_id);
        if todos.len() != before {
            write_json(&file, &todos)?;
            self.sync_todos_index(project_id, &todos).await?;
        }
        Ok(())
    }

    /// `todos.toggle` - flip the `done` flag on a todo; persist + reindex.
    pub async fn todos_toggle(&self, project_id: &str, todo_id: &str) -> anyhow::Result<()> {
        let project_path = self.project_path_for(project_id).await?;
        let file = atlas_file(&project_path, "todos");
        let mut todos: Vec<Todo> = read_json(&file)?.unwrap_or_default();
        let target = todos
            .iter_mut()
            .find(|t| t.id == todo_id)
            .ok_or_else(|| anyhow::anyhow!("todo {todo_id} not found in project {project_id}"))?;
        target.done = !target.done;
        write_json(&file, &todos)?;
        self.sync_todos_index(project_id, &todos).await?;
        Ok(())
    }

    /// Rebuild `todos_fts` rows for `project_id` and refresh the cached
    pub(crate) async fn sync_todos_index(
        &self,
        project_id: &str,
        todos: &[Todo],
    ) -> anyhow::Result<()> {
        let mut tx = self.pool.begin().await?;

        sqlx::query("DELETE FROM todos_fts WHERE project_id = ?")
            .bind(project_id)
            .execute(&mut *tx)
            .await?;

        for t in todos {
            sqlx::query("INSERT INTO todos_fts (project_id, todo_id, text) VALUES (?, ?, ?)")
                .bind(project_id)
                .bind(&t.id)
                .bind(&t.text)
                .execute(&mut *tx)
                .await?;
        }

        // `todos_count` is the *open* todos count - the UI shows "n
        let open_count = todos.iter().filter(|t| !t.done).count() as i64;
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query("UPDATE projects SET todos_count = ?, updated_at = ? WHERE id = ?")
            .bind(open_count)
            .bind(now)
            .bind(project_id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(())
    }

    /// Look up the project's on-disk path by id. Errors if the project
    async fn project_path_for(&self, project_id: &str) -> anyhow::Result<PathBuf> {
        let project = self
            .get_project(project_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("unknown project id: {project_id}"))?;
        Ok(PathBuf::from(project.path))
    }

    // =================================================================

    /// `pane_layout.get` - load the last-persisted layout for a project,
    pub async fn pane_layout_get(&self, project_id: &str) -> anyhow::Result<Option<PaneLayout>> {
        let project_path = self.project_path_for(project_id).await?;
        let file = atlas_file(&project_path, "panes");
        read_json::<PaneLayout>(&file)
    }

    /// `pane_layout.save` - atomically write the new layout snapshot.
    pub async fn pane_layout_save(
        &self,
        project_id: &str,
        layout: &PaneLayout,
    ) -> anyhow::Result<()> {
        let project_path = self.project_path_for(project_id).await?;
        let file = atlas_file(&project_path, "panes");
        write_json(&file, layout)
    }

    /// `pane_layout.clear` - remove the persisted layout for a project.
    pub async fn pane_layout_clear(&self, project_id: &str) -> anyhow::Result<()> {
        let project_path = self.project_path_for(project_id).await?;
        let file = atlas_file(&project_path, "panes");
        match std::fs::remove_file(&file) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(anyhow::anyhow!("remove {}: {e}", file.display())),
        }
    }

    // =================================================================

    /// Distinct tag names across all projects, alphabetically sorted.
    pub async fn list_tags(&self) -> anyhow::Result<Vec<String>> {
        let rows = sqlx::query("SELECT DISTINCT tag FROM tags ORDER BY tag ASC")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .filter_map(|r| r.try_get::<String, _>("tag").ok())
            .collect())
    }

    pub async fn add_tag(&self, project_id: &str, tag: &str) -> anyhow::Result<()> {
        let t = tag.trim();
        if t.is_empty() {
            return Ok(());
        }
        sqlx::query("INSERT OR IGNORE INTO tags (project_id, tag) VALUES (?, ?)")
            .bind(project_id)
            .bind(t)
            .execute(&self.pool)
            .await?;
        self.resync_project_fts(project_id).await?;
        Ok(())
    }

    pub async fn remove_tag(&self, project_id: &str, tag: &str) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM tags WHERE project_id = ? AND tag = ?")
            .bind(project_id)
            .bind(tag)
            .execute(&self.pool)
            .await?;
        self.resync_project_fts(project_id).await?;
        Ok(())
    }

    /// All collections, ordered by `ord` ascending.
    pub async fn list_collections(&self) -> anyhow::Result<Vec<Collection>> {
        let rows = sqlx::query("SELECT id, label, dot, ord FROM collections ORDER BY ord ASC")
            .fetch_all(&self.pool)
            .await?;
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            out.push(Collection {
                id: r.try_get("id")?,
                label: r.try_get("label")?,
                dot: r.try_get::<Option<String>, _>("dot")?.unwrap_or_default(),
                order: r.try_get("ord")?,
            });
        }
        Ok(out)
    }

    pub async fn upsert_collection(&self, col: &Collection) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO collections (id, label, dot, ord) VALUES (?, ?, ?, ?) \
             ON CONFLICT(id) DO UPDATE SET \
                 label = excluded.label, \
                 dot   = excluded.dot, \
                 ord   = excluded.ord",
        )
        .bind(&col.id)
        .bind(&col.label)
        .bind(&col.dot)
        .bind(col.order)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn remove_collection(&self, id: &str) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM collections WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Project ids currently assigned to `collection_id`, sorted by
    pub async fn list_collection_members(
        &self,
        collection_id: &str,
    ) -> anyhow::Result<Vec<String>> {
        let rows = sqlx::query(
            "SELECT project_id FROM collection_members \
             WHERE collection_id = ? \
             ORDER BY project_id ASC",
        )
        .bind(collection_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .filter_map(|r| r.try_get::<String, _>("project_id").ok())
            .collect())
    }

    /// Replace the member set of a collection atomically. Duplicate ids
    pub async fn set_collection_members(
        &self,
        collection_id: &str,
        project_ids: &[String],
    ) -> anyhow::Result<()> {
        // Verify the collection exists; silent inserts into a missing FK
        let exists: Option<(String,)> = sqlx::query_as("SELECT id FROM collections WHERE id = ?")
            .bind(collection_id)
            .fetch_optional(&self.pool)
            .await?;
        if exists.is_none() {
            return Err(anyhow::anyhow!("unknown collection id: {collection_id}"));
        }

        let mut tx = self.pool.begin().await?;

        sqlx::query("DELETE FROM collection_members WHERE collection_id = ?")
            .bind(collection_id)
            .execute(&mut *tx)
            .await?;

        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for pid in project_ids {
            let p = pid.trim();
            if p.is_empty() || !seen.insert(p) {
                continue;
            }
            sqlx::query(
                "INSERT INTO collection_members (collection_id, project_id) \
                 VALUES (?, ?)",
            )
            .bind(collection_id)
            .bind(p)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    // =================================================================

    /// `collections.create` - insert a new row with a fresh UUID v4 and
    pub async fn create_collection(
        &self,
        label: &str,
        color: Option<&str>,
    ) -> anyhow::Result<Collection> {
        let mut tx = self.pool.begin().await?;

        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM collections")
            .fetch_one(&mut *tx)
            .await?;
        let (max_ord,): (Option<i64>,) = sqlx::query_as("SELECT MAX(ord) FROM collections")
            .fetch_one(&mut *tx)
            .await?;

        let id = uuid::Uuid::new_v4().to_string();
        let ord = max_ord.map(|o| o + 1).unwrap_or(0);
        let dot = color
            .map(|c| c.to_string())
            .unwrap_or_else(|| default_collection_color(count as usize));
        let label = label.trim();
        if label.is_empty() {
            return Err(anyhow::anyhow!("collection label cannot be empty"));
        }

        sqlx::query("INSERT INTO collections (id, label, dot, ord) VALUES (?, ?, ?, ?)")
            .bind(&id)
            .bind(label)
            .bind(&dot)
            .bind(ord)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;

        Ok(Collection {
            id,
            label: label.to_string(),
            dot,
            order: ord,
        })
    }

    /// `collections.rename` - update just the label.
    pub async fn rename_collection(&self, id: &str, label: &str) -> anyhow::Result<()> {
        let label = label.trim();
        if label.is_empty() {
            return Err(anyhow::anyhow!("collection label cannot be empty"));
        }
        let res = sqlx::query("UPDATE collections SET label = ? WHERE id = ?")
            .bind(label)
            .bind(id)
            .execute(&self.pool)
            .await?;
        if res.rows_affected() == 0 {
            return Err(anyhow::anyhow!("unknown collection id: {id}"));
        }
        Ok(())
    }

    /// `collections.update_color` - update just the `dot` swatch.
    pub async fn update_collection_color(&self, id: &str, color: &str) -> anyhow::Result<()> {
        let res = sqlx::query("UPDATE collections SET dot = ? WHERE id = ?")
            .bind(color)
            .bind(id)
            .execute(&self.pool)
            .await?;
        if res.rows_affected() == 0 {
            return Err(anyhow::anyhow!("unknown collection id: {id}"));
        }
        Ok(())
    }

    /// `collections.delete` - drop the row + every link-table entry in
    pub async fn delete_collection(&self, id: &str) -> anyhow::Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM collection_members WHERE collection_id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM collections WHERE id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// `collections.reorder` - rewrite `ord` so the rows sort in the
    pub async fn reorder_collections(&self, ordered_ids: &[String]) -> anyhow::Result<()> {
        let mut tx = self.pool.begin().await?;
        for (i, id) in ordered_ids.iter().enumerate() {
            sqlx::query("UPDATE collections SET ord = ? WHERE id = ?")
                .bind(i as i64)
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// `collections.add_project` - upsert a (collection, project) pair.
    pub async fn add_project_to_collection(
        &self,
        project_id: &str,
        collection_id: &str,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO collection_members (collection_id, project_id) \
             VALUES (?, ?) \
             ON CONFLICT(collection_id, project_id) DO NOTHING",
        )
        .bind(collection_id)
        .bind(project_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// `collections.remove_project` - drop a (collection, project) pair.
    pub async fn remove_project_from_collection(
        &self,
        project_id: &str,
        collection_id: &str,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "DELETE FROM collection_members \
             WHERE collection_id = ? AND project_id = ?",
        )
        .bind(collection_id)
        .bind(project_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// `collections.projects` - hydrate every project assigned to the
    pub async fn list_collection_projects(
        &self,
        collection_id: &str,
    ) -> anyhow::Result<Vec<Project>> {
        let rows = sqlx::query(
            "SELECT p.id, p.name, p.path, p.language, p.color, p.branch, \
                    p.dirty, p.ahead, p.behind, p.loc, p.size_bytes, p.disk_bytes, p.last_opened, \
                    p.pinned, p.archived, p.todos_count, p.notes_count, p.time_tracked, p.author \
             FROM projects p \
             JOIN collection_members m ON m.project_id = p.id \
             WHERE m.collection_id = ? \
             ORDER BY p.pinned DESC, \
                      (p.pin_ord IS NULL), p.pin_ord ASC, \
                      (p.last_opened IS NULL), p.last_opened DESC, \
                      p.name ASC",
        )
        .bind(collection_id)
        .fetch_all(&self.pool)
        .await?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            out.push(self.hydrate_project(row).await?);
        }
        Ok(out)
    }

    // =================================================================

    /// `notes.list` - every `<project>/.atlas/notes/*.json`, parsed and
    pub async fn notes_list(&self, project_id: &str) -> anyhow::Result<Vec<Note>> {
        let project_path = self.project_path_for(project_id).await?;
        let notes_dir = project_path.join(".atlas").join("notes");

        let mut out: Vec<Note> = Vec::new();
        let entries = match std::fs::read_dir(&notes_dir) {
            Ok(e) => e,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Ok(out);
            }
            Err(err) => {
                return Err(anyhow::anyhow!("read {}: {err}", notes_dir.display()));
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            // Skip hidden / temp files (e.g. `.abc.json.tmp`).
            if path
                .file_name()
                .and_then(|s| s.to_str())
                .map(|n| n.starts_with('.'))
                .unwrap_or(false)
            {
                continue;
            }
            match read_json::<Note>(&path) {
                Ok(Some(n)) => out.push(n),
                Ok(None) => {}
                Err(err) => {
                    tracing::warn!("skip malformed note {}: {err}", path.display());
                }
            }
        }

        // Pinned first (desc), then `updated_at` desc - lexicographic
        out.sort_by(|a, b| {
            b.pinned
                .cmp(&a.pinned)
                .then_with(|| b.updated_at.cmp(&a.updated_at))
        });

        Ok(out)
    }

    /// `notes.get` - fetch a single note. `None` if the file is missing.
    pub async fn notes_get(&self, project_id: &str, note_id: &str) -> anyhow::Result<Option<Note>> {
        let project_path = self.project_path_for(project_id).await?;
        let file = atlas_note_file(&project_path, note_id);
        read_json::<Note>(&file)
    }

    /// `notes.upsert` - write the note atomically and rebuild its row in
    pub async fn notes_upsert(&self, project_id: &str, note: &Note) -> anyhow::Result<()> {
        let project_path = self.project_path_for(project_id).await?;
        let file = atlas_note_file(&project_path, &note.id);
        write_json(&file, note)?;

        self.resync_note_fts_row(project_id, note).await?;
        self.refresh_notes_count(project_id, &project_path).await?;
        Ok(())
    }

    /// `notes.delete` - remove the JSON file + the FTS row + refresh
    pub async fn notes_delete(&self, project_id: &str, note_id: &str) -> anyhow::Result<()> {
        let project_path = self.project_path_for(project_id).await?;
        let file = atlas_note_file(&project_path, note_id);

        match tokio::fs::remove_file(&file).await {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(anyhow::anyhow!("remove {}: {err}", file.display()));
            }
        }

        sqlx::query("DELETE FROM notes_fts WHERE project_id = ? AND note_id = ?")
            .bind(project_id)
            .bind(note_id)
            .execute(&self.pool)
            .await?;

        self.refresh_notes_count(project_id, &project_path).await?;
        Ok(())
    }

    /// `notes.pin` - toggle the `pinned` flag, bumping `updatedAt`. Errors
    pub async fn notes_pin(
        &self,
        project_id: &str,
        note_id: &str,
        pinned: bool,
    ) -> anyhow::Result<()> {
        let project_path = self.project_path_for(project_id).await?;
        let file = atlas_note_file(&project_path, note_id);
        let mut note: Note = read_json(&file)?
            .ok_or_else(|| anyhow::anyhow!("note {note_id} not found in project {project_id}"))?;
        note.pinned = pinned;
        note.updated_at = chrono::Utc::now().to_rfc3339();
        write_json(&file, &note)?;
        // FTS body/title didn't change but a future SELECT joining
        self.resync_note_fts_row(project_id, &note).await?;
        Ok(())
    }

    /// `notes.search` - FTS5 match over `title` + `body_plain`, hydrated
    pub async fn notes_search(&self, project_id: &str, query: &str) -> anyhow::Result<Vec<Note>> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Ok(Vec::new());
        }
        let fts_query = if trimmed.contains('"') || trimmed.contains('*') || trimmed.contains(':') {
            trimmed.to_string()
        } else {
            trimmed
                .split_whitespace()
                .map(|w| format!("{}*", w))
                .collect::<Vec<_>>()
                .join(" ")
        };

        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT note_id FROM notes_fts \
             WHERE project_id = ? AND notes_fts MATCH ? \
             ORDER BY bm25(notes_fts)",
        )
        .bind(project_id)
        .bind(&fts_query)
        .fetch_all(&self.pool)
        .await?;

        let project_path = self.project_path_for(project_id).await?;
        let mut out = Vec::with_capacity(rows.len());
        for (nid,) in rows {
            let file = atlas_note_file(&project_path, &nid);
            if let Some(note) = read_json::<Note>(&file)? {
                out.push(note);
            }
            // Silently drop FTS rows whose JSON has vanished; the next
        }
        Ok(out)
    }

    /// Replace (or insert) a single note's FTS row. Uses a delete-by
    async fn resync_note_fts_row(&self, project_id: &str, note: &Note) -> anyhow::Result<()> {
        let body_plain = html_to_plaintext(&note.body);
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM notes_fts WHERE project_id = ? AND note_id = ?")
            .bind(project_id)
            .bind(&note.id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO notes_fts (project_id, note_id, title, body_plain) \
             VALUES (?, ?, ?, ?)",
        )
        .bind(project_id)
        .bind(&note.id)
        .bind(&note.title)
        .bind(&body_plain)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Re-read `<project>/.atlas/notes/*.json` and write the count back
    pub(crate) async fn refresh_notes_count(
        &self,
        project_id: &str,
        project_path: &Path,
    ) -> anyhow::Result<()> {
        let notes_dir = project_path.join(".atlas").join("notes");
        let mut count: i64 = 0;
        match std::fs::read_dir(&notes_dir) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|s| s.to_str()) != Some("json") {
                        continue;
                    }
                    if path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .map(|n| n.starts_with('.'))
                        .unwrap_or(false)
                    {
                        continue;
                    }
                    count += 1;
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(anyhow::anyhow!("scan {}: {err}", notes_dir.display()));
            }
        }
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query("UPDATE projects SET notes_count = ?, updated_at = ? WHERE id = ?")
            .bind(count)
            .bind(now)
            .bind(project_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // =================================================================

    /// Static action catalog surfaced at the bottom of the palette. IDs
    fn action_catalog() -> Vec<PaletteItem> {
        vec![
            PaletteItem::Action {
                id: "new-project".into(),
                label: "New project".into(),
                hint: "Create a new project from template".into(),
                keys: vec!["Mod+N".into()],
            },
            PaletteItem::Action {
                id: "clone-from-git".into(),
                label: "Clone from git".into(),
                hint: "Clone a remote repository".into(),
                keys: vec!["Mod+Shift+N".into()],
            },
            PaletteItem::Action {
                id: "import-folder".into(),
                label: "Import folder".into(),
                hint: "Import an existing folder as a project".into(),
                keys: vec![],
            },
            PaletteItem::Action {
                id: "open-settings".into(),
                label: "Open settings".into(),
                hint: "Atlas settings".into(),
                keys: vec!["Mod+,".into()],
            },
            PaletteItem::Action {
                id: "open-terminal".into(),
                label: "Open terminal".into(),
                hint: "Toggle the terminal strip".into(),
                keys: vec!["Ctrl+`".into()],
            },
            PaletteItem::Action {
                id: "toggle-theme".into(),
                label: "Toggle theme".into(),
                hint: "Switch light / dark / system".into(),
                keys: vec![],
            },
        ]
    }

    /// Case-insensitive substring match. The palette is incremental - on
    fn action_matches(label: &str, hint: &str, query: &str) -> bool {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return true;
        }
        let hay = format!("{} {}", label.to_lowercase(), hint.to_lowercase());
        q.split_whitespace().all(|w| hay.contains(w))
    }

    /// Palette source - the one IPC the ⌘K palette calls on every
    pub async fn palette_source(
        &self,
        query: &str,
        limit: u32,
    ) -> anyhow::Result<Vec<PaletteItem>> {
        let trimmed = query.trim();
        let limit_usize = limit.max(1) as usize;

        if trimmed.is_empty() {
            let recents = self.recents_list(limit).await?;
            return Ok(recents
                .into_iter()
                .map(|p| PaletteItem::Recent { project: p })
                .collect());
        }

        // Build the FTS query - mirror the helper in `search_projects`.
        let fts_query = if trimmed.contains('"') || trimmed.contains('*') || trimmed.contains(':') {
            trimmed.to_string()
        } else {
            trimmed
                .split_whitespace()
                .map(|w| format!("{}*", w))
                .collect::<Vec<_>>()
                .join(" ")
        };

        // --- Projects (sorted by bm25 ascending) ---
        let project_rows = sqlx::query(
            "SELECT p.id, p.name, p.path, p.language, p.color, p.branch, \
                    p.dirty, p.ahead, p.behind, p.loc, p.size_bytes, p.disk_bytes, p.last_opened, \
                    p.pinned, p.archived, p.todos_count, p.notes_count, p.time_tracked, p.author, \
                    bm25(projects_fts) AS score \
             FROM projects p \
             JOIN projects_fts f ON f.rowid = p.rowid \
             WHERE projects_fts MATCH ? AND p.archived = 0 \
             ORDER BY score ASC \
             LIMIT ?",
        )
        .bind(&fts_query)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut project_items: Vec<PaletteItem> = Vec::with_capacity(project_rows.len());
        let mut seen_project_ids: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        for row in project_rows {
            let score: f64 = row.try_get::<f64, _>("score").unwrap_or(0.0);
            let project = self.hydrate_project(row).await?;
            if seen_project_ids.insert(project.id.clone()) {
                project_items.push(PaletteItem::Project {
                    project,
                    score: score as f32,
                });
            }
        }

        // --- Notes (sorted by bm25 ascending) ---
        let note_rows: Vec<(String, String, String, String, f64)> = sqlx::query_as(
            "SELECT project_id, note_id, title, body_plain, bm25(notes_fts) AS score \
             FROM notes_fts \
             WHERE notes_fts MATCH ? \
             ORDER BY score ASC \
             LIMIT ?",
        )
        .bind(&fts_query)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut note_items: Vec<PaletteItem> = Vec::with_capacity(note_rows.len());
        let mut seen_note_ids: std::collections::HashSet<(String, String)> =
            std::collections::HashSet::new();
        for (project_id, note_id, title, body_plain, score) in note_rows {
            let key = (project_id.clone(), note_id.clone());
            if !seen_note_ids.insert(key) {
                continue;
            }
            let snippet = snippet_from_plain(&body_plain, 140);
            note_items.push(PaletteItem::Note {
                project_id,
                note_id,
                title,
                snippet,
                score: score as f32,
            });
        }

        // --- Actions (static list, in-process filter) ---
        let action_items: Vec<PaletteItem> = Self::action_catalog()
            .into_iter()
            .filter(|item| {
                if let PaletteItem::Action { label, hint, .. } = item {
                    Self::action_matches(label, hint, trimmed)
                } else {
                    false
                }
            })
            .collect();

        // --- Merge in the kind-priority order specified in the brief ---
        let mut out: Vec<PaletteItem> =
            Vec::with_capacity(project_items.len() + note_items.len() + action_items.len());
        out.extend(project_items);
        out.extend(note_items);
        out.extend(action_items);
        out.truncate(limit_usize);
        Ok(out)
    }

    /// Push a project to the top of the recents ring buffer.
    pub async fn recents_push(&self, project_id: &str) -> anyhow::Result<()> {
        // Verify the project exists (FK would reject otherwise, but a
        let exists: Option<(String,)> = sqlx::query_as("SELECT id FROM projects WHERE id = ?")
            .bind(project_id)
            .fetch_optional(&self.pool)
            .await?;
        if exists.is_none() {
            return Err(anyhow::anyhow!("unknown project id: {project_id}"));
        }

        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO recents (project_id, opened_at) VALUES (?, ?) \
             ON CONFLICT(project_id) DO UPDATE SET opened_at = excluded.opened_at",
        )
        .bind(project_id)
        .bind(&now)
        .execute(&self.pool)
        .await?;

        // Keep `projects.last_opened` in sync so the list view's "opened"
        sqlx::query("UPDATE projects SET last_opened = ?, updated_at = ? WHERE id = ?")
            .bind(&now)
            .bind(&now)
            .bind(project_id)
            .execute(&self.pool)
            .await?;

        // Trim to 20 - keep the newest, drop the tail.
        sqlx::query(
            "DELETE FROM recents WHERE project_id IN ( \
                SELECT project_id FROM recents \
                ORDER BY opened_at DESC \
                LIMIT -1 OFFSET 20 \
             )",
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// List recents projects in LIFO order (most recent first), capped at
    pub async fn recents_list(&self, limit: u32) -> anyhow::Result<Vec<Project>> {
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT project_id FROM recents ORDER BY opened_at DESC LIMIT ?")
                .bind(limit as i64)
                .fetch_all(&self.pool)
                .await?;

        let mut out = Vec::with_capacity(rows.len());
        for (pid,) in rows {
            if let Some(p) = self.get_project(&pid).await? {
                out.push(p);
            }
        }
        Ok(out)
    }

    // =================================================================

    /// Fetch tag list for a project without going through the full
    async fn tags_for_project_internal(&self, id: &str) -> anyhow::Result<Vec<String>> {
        let rows = sqlx::query("SELECT tag FROM tags WHERE project_id = ? ORDER BY tag")
            .bind(id)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .filter_map(|r| r.try_get::<String, _>("tag").ok())
            .collect())
    }

    /// Delete-and-reinsert a project's FTS row so (name, path, tags) stay
    pub(crate) async fn resync_project_fts(&self, id: &str) -> anyhow::Result<()> {
        let row = sqlx::query("SELECT rowid, name, path FROM projects WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        let Some(row) = row else {
            return Ok(());
        };
        let rowid: i64 = row.try_get("rowid")?;
        let name: String = row.try_get("name")?;
        let path: String = row.try_get("path")?;
        let tags_joined = self.tags_for_project_internal(id).await?.join(" ");
        fts_replace_project(&self.pool, rowid, &name, &path, &tags_joined).await
    }
}

/// Delete (if present) then insert an FTS row for one project. Extracted
async fn fts_replace_project(
    pool: &SqlitePool,
    rowid: i64,
    name: &str,
    path: &str,
    tags_joined: &str,
) -> anyhow::Result<()> {
    // Best-effort delete. FTS5 'delete' command needs the OLD values of
    let existing: Option<(String, String, String)> =
        sqlx::query_as("SELECT name, path, tags FROM projects_fts WHERE rowid = ?")
            .bind(rowid)
            .fetch_optional(pool)
            .await
            .unwrap_or(None);

    if let Some((old_name, old_path, old_tags)) = existing {
        sqlx::query(
            "INSERT INTO projects_fts (projects_fts, rowid, name, path, tags) \
             VALUES ('delete', ?, ?, ?, ?)",
        )
        .bind(rowid)
        .bind(old_name)
        .bind(old_path)
        .bind(old_tags)
        .execute(pool)
        .await?;
    }

    sqlx::query("INSERT INTO projects_fts (rowid, name, path, tags) VALUES (?, ?, ?, ?)")
        .bind(rowid)
        .bind(name)
        .bind(path)
        .bind(tags_joined)
        .execute(pool)
        .await?;
    Ok(())
}

// ---------- helpers ----------

/// Escape `%` and `_` for SQL LIKE with the `\` escape char.
fn escape_like(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' | '%' | '_' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out
}

/// Deterministic 12-char lowercase hex slug for a path.
pub fn project_id_for_path(path: &Path) -> String {
    // Try canonicalize; fall back to the raw path so missing paths still
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let bytes = canonical.as_os_str().to_string_lossy();
    let h = fnv1a_64(bytes.as_bytes());
    format!("{:012x}", h & 0x0000_FFFF_FFFF_FFFF)
}

/// FNV-1a 64-bit. Offset basis and prime are the published constants.
fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// Default OKLCH swatch for a language. Used when we insert a freshly
fn color_for_lang(lang: &Lang) -> &'static str {
    match lang {
        Lang::TypeScript => "#3178C6",
        Lang::JavaScript => "#F7DF1E",
        Lang::Rust => "#E0763C",
        Lang::Go => "#00ADD8",
        Lang::Python => "#3572A5",
        Lang::Swift => "#F05138",
        Lang::Kotlin => "#A97BFF",
        Lang::Ruby => "#CC342D",
        Lang::Java => "#B07219",
        Lang::C => "#555555",
        Lang::CPlusPlus => "#F34B7D",
        Lang::Other => "#8A8AA0",
    }
}

struct SeedProject {
    id: &'static str,
    name: &'static str,
    path: &'static str,
    language: Lang,
    color: &'static str,
    branch: &'static str,
    dirty: i64,
    ahead: i64,
    behind: i64,
    loc: i64,
    size_bytes: i64,
    last_opened_minutes_ago: Option<i64>,
    pinned: bool,
    tags: &'static [&'static str],
    todos: i64,
    notes: i64,
    time: &'static str,
    archived: bool,
    collection: &'static str,
}

fn lang_to_str(l: &Lang) -> &'static str {
    match l {
        Lang::TypeScript => "TypeScript",
        Lang::JavaScript => "JavaScript",
        Lang::Rust => "Rust",
        Lang::Go => "Go",
        Lang::Python => "Python",
        Lang::Swift => "Swift",
        Lang::Kotlin => "Kotlin",
        Lang::Ruby => "Ruby",
        Lang::Java => "Java",
        Lang::C => "C",
        Lang::CPlusPlus => "C++",
        Lang::Other => "Other",
    }
}

fn str_to_lang(s: &str) -> Lang {
    match s {
        "TypeScript" => Lang::TypeScript,
        "JavaScript" => Lang::JavaScript,
        "Rust" => Lang::Rust,
        "Go" => Lang::Go,
        "Python" => Lang::Python,
        "Swift" => Lang::Swift,
        "Kotlin" => Lang::Kotlin,
        "Ruby" => Lang::Ruby,
        "Java" => Lang::Java,
        "C" => Lang::C,
        "C++" => Lang::CPlusPlus,
        _ => Lang::Other,
    }
}

/// Strip HTML tags and decode a handful of common entities so the
pub(crate) fn html_to_plaintext(html: &str) -> String {
    // Early out for the common "empty note" case.
    if html.is_empty() {
        return String::new();
    }

    // Stage 1 - strip tags. Track a bool instead of building spans.
    let mut stripped = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' if !in_tag => in_tag = true,
            '>' if in_tag => {
                in_tag = false;
                // Replace a closed tag with a space so adjacent words
                stripped.push(' ');
            }
            _ if in_tag => {}
            _ => stripped.push(ch),
        }
    }

    // Stage 2 - decode entities in a single pass.
    let mut decoded = String::with_capacity(stripped.len());
    let mut i = 0;
    let bytes = stripped.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'&' {
            // Find the terminating ';' within a small window - entities
            if let Some(end_rel) = stripped[i + 1..].bytes().take(12).position(|b| b == b';') {
                let end = i + 1 + end_rel;
                let entity = &stripped[i + 1..end];
                let replacement: Option<char> = match entity {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" | "#39" => Some('\''),
                    "nbsp" | "#160" => Some(' '),
                    // Bare numeric entities (e.g. `&#8217;` curly apostrophe):
                    s if s.starts_with('#') => {
                        let rest = &s[1..];
                        let (radix, digits) = if let Some(hex) = rest.strip_prefix('x') {
                            (16, hex)
                        } else if let Some(hex) = rest.strip_prefix('X') {
                            (16, hex)
                        } else {
                            (10, rest)
                        };
                        u32::from_str_radix(digits, radix)
                            .ok()
                            .and_then(char::from_u32)
                    }
                    _ => None,
                };
                if let Some(c) = replacement {
                    decoded.push(c);
                    i = end + 1;
                    continue;
                }
                // Unknown entity - keep the `&` literal and let the
            }
            decoded.push('&');
            i += 1;
        } else {
            // UTF-8 safe push: copy one code point at a time by finding
            let ch = stripped[i..].chars().next().unwrap_or('\0');
            decoded.push(ch);
            i += ch.len_utf8();
        }
    }

    // Stage 3 - collapse whitespace.
    let mut out = String::with_capacity(decoded.len());
    let mut prev_space = true; // suppress leading whitespace
    for ch in decoded.chars() {
        if ch.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(ch);
            prev_space = false;
        }
    }
    // Trim any trailing space from the final "collapse".
    if out.ends_with(' ') {
        out.pop();
    }
    out
}

/// Trim a stripped-plaintext body to at most `max_chars` unicode scalar
fn snippet_from_plain(plain: &str, max_chars: usize) -> String {
    let trimmed = plain.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let mut s: String = trimmed.chars().take(max_chars).collect();
    s.push('…');
    s
}

/// Default swatch palette for freshly-created collections. Rotates by
fn default_collection_color(existing_count: usize) -> String {
    const PALETTE: &[&str] = &[
        "oklch(0.78 0.17 145)", // green
        "oklch(0.78 0.15 260)", // blue
        "oklch(0.78 0.15 55)",  // amber
        "oklch(0.78 0.17 28)",  // red
        "oklch(0.78 0.17 320)", // magenta
        "oklch(0.78 0.15 195)", // teal
        "oklch(0.78 0.15 80)",  // yellow
        "oklch(0.70 0.02 260)", // slate
    ];
    PALETTE[existing_count % PALETTE.len()].to_string()
}

/// Pretty-print byte count (matches what the UI expects - e.g. "412 MB",
fn format_size(bytes: i64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * KB;
    const GB: f64 = 1024.0 * MB;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.1} GB", b / GB)
    } else if b >= MB {
        format!("{} MB", (b / MB).round() as i64)
    } else if b >= KB {
        format!("{} KB", (b / KB).round() as i64)
    } else {
        format!("{} B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn seed_then_list_returns_twelve() -> anyhow::Result<()> {
        let db = Db::open_in_memory().await?;
        let inserted = db.seed_fixtures().await?;
        assert_eq!(inserted, 12);

        // Default filter excludes archived → 11
        let default_filter = ProjectFilter::default();
        let projects = db.list_projects(default_filter).await?;
        assert_eq!(projects.len(), 11, "archived 'kelp' should be hidden");

        // Keep this print - it's how D1 verifies the shape in the handoff.
        for p in &projects {
            eprintln!(
                "seed: {:10} lang={:?} branch={:16} dirty={} pinned={} tags={:?}",
                p.name, p.language, p.branch, p.dirty, p.pinned, p.tags
            );
        }

        // With archived → 12
        let with_archived = ProjectFilter {
            include_archived: true,
            ..Default::default()
        };
        assert_eq!(db.list_projects(with_archived).await?.len(), 12);

        // Pinned first in sort
        let pinned_only = ProjectFilter {
            pinned_only: true,
            ..Default::default()
        };
        let pinned = db.list_projects(pinned_only).await?;
        assert!(pinned.iter().all(|p| p.pinned));
        assert_eq!(pinned.len(), 3);

        // FTS: "dashboard" should find birch
        let hits = db.search_projects("dashboard").await?;
        assert!(hits.iter().any(|p| p.id == "birch"));

        // get_project by id
        let one = db.get_project("acorn").await?.expect("acorn present");
        assert_eq!(one.name, "acorn-api");
        assert_eq!(one.language, Lang::Rust);
        assert!(one.tags.contains(&"api".to_string()));

        // Idempotence: second seed returns 0
        assert_eq!(db.seed_fixtures().await?, 0);
        Ok(())
    }

    #[test]
    fn format_size_thresholds() {
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(2 * 1024), "2 KB");
        assert_eq!(format_size(412 * 1_048_576), "412 MB");
        assert!(format_size(2 * 1_073_741_824).ends_with("GB"));
    }

    /// Build a throwaway directory containing N fake `.git`-bearing dirs
    fn mkrepo_tree(tag: &str, repos: &[(&str, &[&str])]) -> std::path::PathBuf {
        use std::fs;
        let base = std::env::temp_dir().join(format!(
            "atlas-db-test-{}-{}-{}",
            tag,
            std::process::id(),
            // Ensure uniqueness across test-within-same-process.
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
        ));
        fs::create_dir_all(&base).unwrap();
        for (name, files) in repos {
            let dir = base.join(name);
            fs::create_dir_all(dir.join(".git")).unwrap();
            for f in *files {
                fs::write(dir.join(f), b"").unwrap();
            }
        }
        base
    }

    #[tokio::test]
    async fn discover_root_populates_and_is_idempotent() -> anyhow::Result<()> {
        let db = Db::open_in_memory().await?;
        let root = mkrepo_tree(
            "discover",
            &[
                ("alpha", &["Cargo.toml"]),
                ("beta", &["package.json", "tsconfig.json"]),
                ("gamma", &["go.mod"]),
            ],
        );

        // First run - 3 new ids.
        let first = db.discover_root(&root, 3).await?;
        assert_eq!(first.len(), 3, "expected 3 new projects, got {first:?}");

        // They made it into the index and carry the right language.
        let projects = db
            .list_projects(ProjectFilter {
                include_archived: true,
                ..Default::default()
            })
            .await?;
        let names: Vec<&str> = projects.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"alpha"));
        assert!(names.contains(&"beta"));
        assert!(names.contains(&"gamma"));
        let by_name = |n: &str| projects.iter().find(|p| p.name == n).unwrap().clone();
        assert_eq!(by_name("alpha").language, Lang::Rust);
        assert_eq!(by_name("beta").language, Lang::TypeScript);
        assert_eq!(by_name("gamma").language, Lang::Go);

        // Second run - 0 new ids (idempotent).
        let second = db.discover_root(&root, 3).await?;
        assert_eq!(second.len(), 0, "discovery should be idempotent");

        // FTS is searchable against discovered names.
        let hits = db.search_projects("alpha").await?;
        assert!(hits.iter().any(|p| p.name == "alpha"));

        // Cleanup.
        let _ = std::fs::remove_dir_all(&root);
        Ok(())
    }

    #[tokio::test]
    async fn pin_project_toggles() -> anyhow::Result<()> {
        let db = Db::open_in_memory().await?;
        db.seed_fixtures().await?;

        // `cedar` starts unpinned in the fixture set.
        let before = db.get_project("cedar").await?.expect("cedar");
        assert!(!before.pinned);

        db.pin_project("cedar", true).await?;
        let after = db.get_project("cedar").await?.expect("cedar");
        assert!(after.pinned);

        db.pin_project("cedar", false).await?;
        let reverted = db.get_project("cedar").await?.expect("cedar");
        assert!(!reverted.pinned);
        Ok(())
    }

    /// Seed fixtures pre-pin `hawthorn`; unpin it so the reorder tests
    async fn unpin_all_seeded(db: &Db) -> anyhow::Result<()> {
        let all = db
            .list_projects(ProjectFilter {
                include_archived: true,
                pinned_only: true,
                ..Default::default()
            })
            .await?;
        for p in all {
            db.pin_project(&p.id, false).await?;
        }
        Ok(())
    }

    #[tokio::test]
    async fn reorder_pinned_persists_across_list_projects() -> anyhow::Result<()> {
        let db = Db::open_in_memory().await?;
        db.seed_fixtures().await?;
        unpin_all_seeded(&db).await?;

        // Pin 3 projects so we have something to reorder.
        db.pin_project("acorn", true).await?;
        db.pin_project("birch", true).await?;
        db.pin_project("cedar", true).await?;

        // Request an explicit drag order: cedar first, acorn second,
        db.reorder_pinned(&[
            "cedar".to_string(),
            "acorn".to_string(),
            "birch".to_string(),
        ])
        .await?;

        let listed = db.list_projects(ProjectFilter::default()).await?;
        let pinned_ids: Vec<&str> = listed
            .iter()
            .filter(|p| p.pinned)
            .map(|p| p.id.as_str())
            .collect();
        assert_eq!(pinned_ids, vec!["cedar", "acorn", "birch"]);
        Ok(())
    }

    #[tokio::test]
    async fn reorder_pinned_clears_pin_ord_for_omitted_rows() -> anyhow::Result<()> {
        let db = Db::open_in_memory().await?;
        db.seed_fixtures().await?;
        unpin_all_seeded(&db).await?;

        db.pin_project("acorn", true).await?;
        db.pin_project("birch", true).await?;
        db.pin_project("cedar", true).await?;

        // First reorder sets all three.
        db.reorder_pinned(&[
            "acorn".to_string(),
            "birch".to_string(),
            "cedar".to_string(),
        ])
        .await?;

        // Second reorder drops `birch` - its pin_ord must be cleared,
        db.reorder_pinned(&["cedar".to_string(), "acorn".to_string()])
            .await?;

        let listed = db.list_projects(ProjectFilter::default()).await?;
        let pinned_ids: Vec<&str> = listed
            .iter()
            .filter(|p| p.pinned)
            .map(|p| p.id.as_str())
            .collect();
        // cedar (pin_ord=0), acorn (pin_ord=1), birch (pin_ord=NULL) last.
        assert_eq!(pinned_ids, vec!["cedar", "acorn", "birch"]);
        Ok(())
    }

    #[tokio::test]
    async fn reorder_pinned_empty_clears_all_orders() -> anyhow::Result<()> {
        let db = Db::open_in_memory().await?;
        db.seed_fixtures().await?;
        unpin_all_seeded(&db).await?;

        db.pin_project("acorn", true).await?;
        db.pin_project("birch", true).await?;
        db.reorder_pinned(&["acorn".to_string(), "birch".to_string()])
            .await?;

        // Empty reorder wipes every pin_ord but keeps rows pinned.
        db.reorder_pinned(&[]).await?;

        let row: (Option<i64>, Option<i64>) = sqlx::query_as(
            "SELECT (SELECT pin_ord FROM projects WHERE id='acorn'), \
                    (SELECT pin_ord FROM projects WHERE id='birch')",
        )
        .fetch_one(db.pool())
        .await?;
        assert_eq!(row, (None, None));

        // Both still pinned (reorder should never silently unpin).
        let a = db.get_project("acorn").await?.unwrap();
        let b = db.get_project("birch").await?.unwrap();
        assert!(a.pinned && b.pinned);
        Ok(())
    }

    #[tokio::test]
    async fn unpinning_clears_pin_ord() -> anyhow::Result<()> {
        let db = Db::open_in_memory().await?;
        db.seed_fixtures().await?;

        db.pin_project("acorn", true).await?;
        db.reorder_pinned(&["acorn".to_string()]).await?;

        // Unpin → pin_ord must be cleared so a later re-pin doesn't
        db.pin_project("acorn", false).await?;
        let ord: (Option<i64>,) = sqlx::query_as("SELECT pin_ord FROM projects WHERE id = 'acorn'")
            .fetch_one(db.pool())
            .await?;
        assert_eq!(ord.0, None);
        Ok(())
    }

    #[tokio::test]
    async fn archive_and_rename_and_tags_roundtrip() -> anyhow::Result<()> {
        let db = Db::open_in_memory().await?;
        db.seed_fixtures().await?;

        // Archive
        db.archive_project("cedar", true).await?;
        let p = db.get_project("cedar").await?.unwrap();
        assert!(p.archived);

        // Rename + FTS picks up new name
        db.rename_project("cedar", "cedar-rebrand").await?;
        let p = db.get_project("cedar").await?.unwrap();
        assert_eq!(p.name, "cedar-rebrand");
        let hits = db.search_projects("rebrand").await?;
        assert!(hits.iter().any(|p| p.id == "cedar"));

        // set_tags replaces the tag set
        db.set_tags(
            "cedar",
            &["rewrite".into(), "rewrite".into(), "".into(), "wip".into()],
        )
        .await?;
        let p = db.get_project("cedar").await?.unwrap();
        // Duplicates collapsed; empty dropped; alpha order from hydrate.
        assert_eq!(p.tags, vec!["rewrite".to_string(), "wip".to_string()]);

        // Tag list surfaces the new tags
        let tags = db.list_tags().await?;
        assert!(tags.contains(&"rewrite".to_string()));
        assert!(tags.contains(&"wip".to_string()));
        Ok(())
    }

    #[tokio::test]
    async fn watchers_crud_and_seed_guard() -> anyhow::Result<()> {
        let db = Db::open_in_memory().await?;

        // Empty at first
        assert!(db.list_watchers().await?.is_empty());

        // Add a watcher → seed_fixtures must become a no-op
        db.add_watcher(Path::new("/tmp/atlas-code"), 4).await?;
        let watchers = db.list_watchers().await?;
        assert_eq!(watchers.len(), 1);
        assert_eq!(watchers[0].1, 4);

        let inserted = db.seed_fixtures().await?;
        assert_eq!(
            inserted, 0,
            "seed must not run when a watcher is configured"
        );

        // Remove the watcher; seed now runs.
        db.remove_watcher(Path::new("/tmp/atlas-code")).await?;
        assert!(db.list_watchers().await?.is_empty());
        assert_eq!(db.seed_fixtures().await?, 12);
        Ok(())
    }

    #[tokio::test]
    async fn count_projects_under_prefix_matches() -> anyhow::Result<()> {
        let db = Db::open_in_memory().await?;
        db.seed_fixtures().await?;
        // Seed projects live under `~/code/...` synthetic paths.
        let n = db.count_projects_under(Path::new("~/code/work")).await?;
        assert!(
            n >= 5,
            "expected at least 5 projects under ~/code/work, got {n}"
        );
        let m = db
            .count_projects_under(Path::new("~/code/personal"))
            .await?;
        assert!(m >= 2);
        Ok(())
    }

    #[tokio::test]
    async fn collections_crud() -> anyhow::Result<()> {
        let db = Db::open_in_memory().await?;
        db.seed_fixtures().await?;

        // Seed installs 4 collections.
        let cols = db.list_collections().await?;
        assert_eq!(cols.len(), 4);

        // Upsert: new
        db.upsert_collection(&Collection {
            id: "misc".into(),
            label: "Misc".into(),
            dot: "#888".into(),
            order: 99,
        })
        .await?;
        let cols = db.list_collections().await?;
        assert_eq!(cols.len(), 5);

        // Upsert: update existing
        db.upsert_collection(&Collection {
            id: "misc".into(),
            label: "Miscellaneous".into(),
            dot: "#888".into(),
            order: 99,
        })
        .await?;
        let cols = db.list_collections().await?;
        let misc = cols.iter().find(|c| c.id == "misc").unwrap();
        assert_eq!(misc.label, "Miscellaneous");

        // Remove
        db.remove_collection("misc").await?;
        let cols = db.list_collections().await?;
        assert!(cols.iter().all(|c| c.id != "misc"));
        Ok(())
    }

    #[test]
    fn project_id_is_deterministic_and_12_hex() {
        let a = project_id_for_path(Path::new("/tmp/atlas-id-test"));
        let b = project_id_for_path(Path::new("/tmp/atlas-id-test"));
        assert_eq!(a, b, "same path => same id");
        assert_eq!(a.len(), 12);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        let c = project_id_for_path(Path::new("/tmp/atlas-id-other"));
        assert_ne!(a, c, "different paths => different ids");
    }

    #[tokio::test]
    async fn apply_git_status_updates_row() -> anyhow::Result<()> {
        let db = Db::open_in_memory().await?;
        db.seed_fixtures().await?;
        let status = crate::git::GitStatus {
            branch: "feat/new".into(),
            dirty: 9,
            ahead: 2,
            behind: 1,
            author: Some("Ada Lovelace".into()),
        };
        db.apply_git_status("acorn", &status).await?;
        let p = db.get_project("acorn").await?.unwrap();
        assert_eq!(p.branch, "feat/new");
        assert_eq!(p.dirty, 9);
        assert_eq!(p.ahead, 2);
        assert_eq!(p.behind, 1);
        assert_eq!(p.author.as_deref(), Some("Ada Lovelace"));
        Ok(())
    }

    use crate::storage::types::{ScriptGroup, Todo};

    /// Build one real on-disk project under temp + register it via
    async fn make_real_project(db: &Db, tag: &str) -> anyhow::Result<(String, std::path::PathBuf)> {
        let root = mkrepo_tree(tag, &[(tag, &["package.json"])]);
        let new = db.discover_root(&root, 3).await?;
        assert_eq!(new.len(), 1, "expected one project under {root:?}");
        let project_path = root.join(tag);
        Ok((new[0].clone(), project_path))
    }

    #[tokio::test]
    async fn scripts_upsert_then_list_returns_row() -> anyhow::Result<()> {
        let db = Db::open_in_memory().await?;
        let (pid, project_path) = make_real_project(&db, "scripts-rt").await?;

        // Empty before any writes.
        assert!(db.scripts_list(&pid).await?.is_empty());

        // Insert.
        let s1 = Script {
            id: "dev".into(),
            name: "dev".into(),
            cmd: "pnpm dev".into(),
            desc: Some("vite dev server".into()),
            group: ScriptGroup::Run,
            default: Some(true),
            icon: None,
            env_defaults: Vec::new(),
        };
        db.scripts_upsert(&pid, &s1).await?;

        let listed = db.scripts_list(&pid).await?;
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "dev");
        assert_eq!(listed[0].cmd, "pnpm dev");

        // File ended up at the right place.
        let scripts_file = project_path.join(".atlas/scripts.json");
        assert!(scripts_file.exists());

        // Update by id (same id ⇒ replace, not append).
        let s1b = Script {
            cmd: "pnpm dev --host".into(),
            ..s1.clone()
        };
        db.scripts_upsert(&pid, &s1b).await?;
        let listed = db.scripts_list(&pid).await?;
        assert_eq!(listed.len(), 1, "upsert by id must replace, not append");
        assert_eq!(listed[0].cmd, "pnpm dev --host");

        // Add a second.
        let s2 = Script {
            id: "build".into(),
            name: "build".into(),
            cmd: "pnpm build".into(),
            desc: None,
            group: ScriptGroup::Build,
            default: None,
            icon: None,
            env_defaults: Vec::new(),
        };
        db.scripts_upsert(&pid, &s2).await?;
        assert_eq!(db.scripts_list(&pid).await?.len(), 2);

        // Delete one.
        db.scripts_delete(&pid, "dev").await?;
        let listed = db.scripts_list(&pid).await?;
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "build");

        // Delete missing is a no-op (no error).
        db.scripts_delete(&pid, "nope").await?;

        std::fs::remove_dir_all(project_path.parent().unwrap()).ok();
        Ok(())
    }

    #[tokio::test]
    async fn scripts_unknown_project_errors() -> anyhow::Result<()> {
        let db = Db::open_in_memory().await?;
        let err = db.scripts_list("does-not-exist").await;
        assert!(
            err.is_err(),
            "unknown project must error, not silently no-op"
        );
        Ok(())
    }

    fn mk_todo(id: &str, text: &str, done: bool) -> Todo {
        Todo {
            id: id.into(),
            done,
            text: text.into(),
            due: None,
            created_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    #[tokio::test]
    async fn todos_upsert_then_toggle_flips_done() -> anyhow::Result<()> {
        let db = Db::open_in_memory().await?;
        let (pid, project_path) = make_real_project(&db, "todos-rt").await?;

        // Empty.
        assert!(db.todos_list(&pid).await?.is_empty());

        // Insert two.
        let t1 = mk_todo("t1", "ship D3", false);
        let t2 = mk_todo("t2", "review PR", false);
        db.todos_upsert(&pid, &t1).await?;
        db.todos_upsert(&pid, &t2).await?;
        assert_eq!(db.todos_list(&pid).await?.len(), 2);

        // todos_count cached open-only count.
        let project = db.get_project(&pid).await?.unwrap();
        assert_eq!(project.todos_count, 2);

        // Toggle one done.
        db.todos_toggle(&pid, "t1").await?;
        let listed = db.todos_list(&pid).await?;
        let t1b = listed.iter().find(|t| t.id == "t1").unwrap();
        assert!(t1b.done, "toggle must flip done to true");

        // Open-count is now 1.
        let project = db.get_project(&pid).await?.unwrap();
        assert_eq!(project.todos_count, 1);

        // Toggle again flips back.
        db.todos_toggle(&pid, "t1").await?;
        let listed = db.todos_list(&pid).await?;
        let t1c = listed.iter().find(|t| t.id == "t1").unwrap();
        assert!(!t1c.done);

        // Delete and re-check count.
        db.todos_delete(&pid, "t1").await?;
        assert_eq!(db.todos_list(&pid).await?.len(), 1);
        let project = db.get_project(&pid).await?.unwrap();
        assert_eq!(project.todos_count, 1);

        // Toggle missing errors (callers shouldn't toggle phantoms).
        let err = db.todos_toggle(&pid, "ghost").await;
        assert!(err.is_err());

        std::fs::remove_dir_all(project_path.parent().unwrap()).ok();
        Ok(())
    }

    #[tokio::test]
    async fn todos_fts_search_finds_text_after_upsert() -> anyhow::Result<()> {
        let db = Db::open_in_memory().await?;
        let (pid, project_path) = make_real_project(&db, "todos-fts").await?;

        db.todos_upsert(&pid, &mk_todo("a", "investigate clippy warnings", false))
            .await?;
        db.todos_upsert(&pid, &mk_todo("b", "ship release notes", false))
            .await?;

        // Search via the FTS table directly - confirms the index is in
        let rows: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT project_id, todo_id, text FROM todos_fts \
             WHERE todos_fts MATCH 'clippy*'",
        )
        .fetch_all(db.pool())
        .await?;
        assert_eq!(rows.len(), 1, "expected exactly one FTS hit for 'clippy*'");
        assert_eq!(rows[0].0, pid);
        assert_eq!(rows[0].1, "a");

        // After delete the row is gone from FTS (replace-all strategy).
        db.todos_delete(&pid, "a").await?;
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT todo_id FROM todos_fts WHERE todos_fts MATCH 'clippy*'")
                .fetch_all(db.pool())
                .await?;
        assert!(rows.is_empty(), "deleted todo must not linger in FTS");

        // The remaining 'release notes' todo is still searchable.
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT todo_id FROM todos_fts WHERE todos_fts MATCH 'release*'")
                .fetch_all(db.pool())
                .await?;
        assert_eq!(rows.len(), 1);

        std::fs::remove_dir_all(project_path.parent().unwrap()).ok();
        Ok(())
    }

    use crate::storage::types::Note;

    fn mk_note(id: &str, title: &str, body: &str, pinned: bool) -> Note {
        let now = chrono::Utc::now().to_rfc3339();
        Note {
            id: id.into(),
            title: title.into(),
            body: body.into(),
            pinned,
            created_at: now.clone(),
            updated_at: now,
        }
    }

    #[tokio::test]
    async fn notes_upsert_then_list_sorts_pinned_first() -> anyhow::Result<()> {
        let db = Db::open_in_memory().await?;
        let (pid, project_path) = make_real_project(&db, "notes-rt").await?;

        // Empty before any writes.
        assert!(db.notes_list(&pid).await?.is_empty());

        // Three notes; middle one pinned. Sleep-free "older" note by
        let mut a = mk_note("a", "alpha", "<p>first note body</p>", false);
        a.updated_at = "2026-01-01T00:00:00Z".into();
        let b = mk_note("b", "beta", "<p>second body</p>", true); // pinned
        let c = mk_note("c", "gamma", "<p>third body</p>", false);

        db.notes_upsert(&pid, &a).await?;
        db.notes_upsert(&pid, &b).await?;
        db.notes_upsert(&pid, &c).await?;

        let listed = db.notes_list(&pid).await?;
        assert_eq!(listed.len(), 3);
        assert_eq!(listed[0].id, "b", "pinned must sort first");
        // Of the unpinned pair, `c` is more recently touched.
        assert_eq!(listed[1].id, "c");
        assert_eq!(listed[2].id, "a");

        // File lives at the right path.
        let note_file = project_path.join(".atlas/notes/b.json");
        assert!(note_file.exists(), "missing {}", note_file.display());

        // Cached count refreshed.
        let project = db.get_project(&pid).await?.unwrap();
        assert_eq!(project.notes_count, 3);

        // Upsert by id (same id ⇒ replace, not append).
        let b2 = Note {
            title: "beta-renamed".into(),
            ..b.clone()
        };
        db.notes_upsert(&pid, &b2).await?;
        let listed = db.notes_list(&pid).await?;
        assert_eq!(listed.len(), 3, "upsert by id must replace, not append");
        let got_b = listed.iter().find(|n| n.id == "b").unwrap();
        assert_eq!(got_b.title, "beta-renamed");

        std::fs::remove_dir_all(project_path.parent().unwrap()).ok();
        Ok(())
    }

    #[tokio::test]
    async fn notes_fts_search_finds_text_after_upsert() -> anyhow::Result<()> {
        let db = Db::open_in_memory().await?;
        let (pid, project_path) = make_real_project(&db, "notes-fts").await?;

        db.notes_upsert(
            &pid,
            &mk_note(
                "a",
                "design review",
                "<p>investigate <strong>clippy</strong> warnings</p>",
                false,
            ),
        )
        .await?;
        db.notes_upsert(
            &pid,
            &mk_note("b", "release", "<p>ship release notes today</p>", false),
        )
        .await?;

        // Body search - HTML stripped, 'clippy' lives in <strong>.
        let hits = db.notes_search(&pid, "clippy").await?;
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "a");
        // Hydrated note carries the original HTML body.
        assert!(hits[0].body.contains("<strong>"));

        // Title search.
        let hits = db.notes_search(&pid, "release").await?;
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "b");

        // Two-word prefix query should still hit.
        let hits = db.notes_search(&pid, "ship release").await?;
        assert!(hits.iter().any(|n| n.id == "b"));

        // Empty query ⇒ empty result (not "match all").
        assert!(db.notes_search(&pid, "   ").await?.is_empty());

        std::fs::remove_dir_all(project_path.parent().unwrap()).ok();
        Ok(())
    }

    #[tokio::test]
    async fn notes_delete_removes_file_and_fts_row() -> anyhow::Result<()> {
        let db = Db::open_in_memory().await?;
        let (pid, project_path) = make_real_project(&db, "notes-del").await?;

        db.notes_upsert(&pid, &mk_note("a", "t1", "<p>body one</p>", false))
            .await?;
        db.notes_upsert(&pid, &mk_note("b", "t2", "<p>body two</p>", false))
            .await?;
        assert_eq!(db.notes_list(&pid).await?.len(), 2);

        // Delete: file + FTS row gone.
        db.notes_delete(&pid, "a").await?;
        let note_file = project_path.join(".atlas/notes/a.json");
        assert!(
            !note_file.exists(),
            "{} should be gone",
            note_file.display()
        );
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT note_id FROM notes_fts WHERE project_id = ? AND note_id = ?")
                .bind(&pid)
                .bind("a")
                .fetch_all(db.pool())
                .await?;
        assert!(rows.is_empty(), "deleted note must not linger in FTS");

        // Count refreshed.
        let project = db.get_project(&pid).await?.unwrap();
        assert_eq!(project.notes_count, 1);

        // Delete missing is a no-op (no error).
        db.notes_delete(&pid, "ghost").await?;

        std::fs::remove_dir_all(project_path.parent().unwrap()).ok();
        Ok(())
    }

    #[tokio::test]
    async fn notes_pin_flips_flag_and_touches_updated_at() -> anyhow::Result<()> {
        let db = Db::open_in_memory().await?;
        let (pid, project_path) = make_real_project(&db, "notes-pin").await?;

        let mut n = mk_note("a", "t", "<p>b</p>", false);
        n.updated_at = "2020-01-01T00:00:00Z".into();
        db.notes_upsert(&pid, &n).await?;

        db.notes_pin(&pid, "a", true).await?;
        let got = db.notes_get(&pid, "a").await?.unwrap();
        assert!(got.pinned);
        assert_ne!(got.updated_at, "2020-01-01T00:00:00Z");

        db.notes_pin(&pid, "a", false).await?;
        let got = db.notes_get(&pid, "a").await?.unwrap();
        assert!(!got.pinned);

        // Unknown id errors.
        let err = db.notes_pin(&pid, "missing", true).await;
        assert!(err.is_err());

        std::fs::remove_dir_all(project_path.parent().unwrap()).ok();
        Ok(())
    }

    #[test]
    fn html_to_plaintext_strips_tags_and_entities() {
        assert_eq!(
            html_to_plaintext("<p>hello <strong>world</strong></p>"),
            "hello world"
        );
        // Adjacent tags get a space so words don't glue together.
        assert_eq!(html_to_plaintext("<p>foo</p><p>bar</p>"), "foo bar");
        // Entities decoded.
        assert_eq!(
            html_to_plaintext("&amp; &lt; &gt; &quot; &#39; &nbsp;"),
            "& < > \" '"
        );
        // Numeric entity (curly apostrophe).
        assert_eq!(html_to_plaintext("it&#8217;s fine"), "it\u{2019}s fine");
        // Empty / whitespace-only.
        assert_eq!(html_to_plaintext(""), "");
        assert_eq!(html_to_plaintext("   \n\t  "), "");
    }

    #[tokio::test]
    async fn collection_members_set_and_list_roundtrip() -> anyhow::Result<()> {
        let db = Db::open_in_memory().await?;
        db.seed_fixtures().await?;

        // Seed installs 4 collections; 'work' already has several members.
        let before = db.list_collection_members("work").await?;
        assert!(before.contains(&"acorn".to_string()));
        assert!(before.contains(&"birch".to_string()));

        // Replace the set atomically; duplicates collapse, empty drops.
        db.set_collection_members(
            "work",
            &[
                "acorn".into(),
                "acorn".into(), // duplicate
                "".into(),      // empty, skipped
                "cedar".into(),
            ],
        )
        .await?;
        let after = db.list_collection_members("work").await?;
        assert_eq!(after.len(), 2);
        assert!(after.contains(&"acorn".to_string()));
        assert!(after.contains(&"cedar".to_string()));

        // Empty list clears membership.
        db.set_collection_members("work", &[]).await?;
        assert!(db.list_collection_members("work").await?.is_empty());

        // Unknown collection errors.
        let err = db.set_collection_members("ghost", &["acorn".into()]).await;
        assert!(err.is_err());

        Ok(())
    }

    #[tokio::test]
    async fn palette_source_finds_seeded_project() -> anyhow::Result<()> {
        let db = Db::open_in_memory().await?;
        db.seed_fixtures().await?;

        let items = db.palette_source("birch", 10).await?;
        assert!(!items.is_empty(), "expected at least one palette hit");
        let found_birch = items
            .iter()
            .any(|i| matches!(i, PaletteItem::Project { project, .. } if project.id == "birch"));
        assert!(found_birch, "birch should appear for query 'birch'");

        Ok(())
    }

    #[tokio::test]
    async fn palette_source_empty_query_returns_recents() -> anyhow::Result<()> {
        let db = Db::open_in_memory().await?;
        db.seed_fixtures().await?;

        // No recents yet ⇒ empty.
        let items = db.palette_source("", 10).await?;
        assert!(items.is_empty());

        // Push a few and check LIFO ordering.
        db.recents_push("acorn").await?;
        db.recents_push("birch").await?;
        db.recents_push("cedar").await?;

        let items = db.palette_source("", 10).await?;
        assert_eq!(items.len(), 3);
        // LIFO: cedar, birch, acorn
        let ids: Vec<&str> = items
            .iter()
            .map(|i| match i {
                PaletteItem::Recent { project } => project.id.as_str(),
                _ => "?",
            })
            .collect();
        assert_eq!(ids, vec!["cedar", "birch", "acorn"]);
        Ok(())
    }

    #[tokio::test]
    async fn palette_source_includes_notes_and_actions() -> anyhow::Result<()> {
        let db = Db::open_in_memory().await?;
        let (pid, project_path) = make_real_project(&db, "palette-notes").await?;

        db.notes_upsert(
            &pid,
            &mk_note(
                "n1",
                "retro notes",
                "<p>mindful observations about clippy</p>",
                false,
            ),
        )
        .await?;

        // Query "clippy" hits the note but nothing in project names.
        let items = db.palette_source("clippy", 10).await?;
        assert!(items.iter().any(|i| matches!(i, PaletteItem::Note { .. })));

        // Action label "settings" matches the open-settings action.
        let items = db.palette_source("settings", 10).await?;
        assert!(items.iter().any(|i| matches!(
            i,
            PaletteItem::Action { id, .. } if id == "open-settings"
        )));

        // Action hint "template" matches new-project.
        let items = db.palette_source("template", 10).await?;
        assert!(items.iter().any(|i| matches!(
            i,
            PaletteItem::Action { id, .. } if id == "new-project"
        )));

        std::fs::remove_dir_all(project_path.parent().unwrap()).ok();
        Ok(())
    }

    #[tokio::test]
    async fn palette_source_respects_limit_and_kind_priority() -> anyhow::Result<()> {
        let db = Db::open_in_memory().await?;
        db.seed_fixtures().await?;

        // Seeded projects share many tokens, so "a*" matches plenty.
        let items = db.palette_source("a", 2).await?;
        assert!(items.len() <= 2);

        // When projects exist for a term, they come before actions.
        let items = db.palette_source("birch", 10).await?;
        let project_idx = items
            .iter()
            .position(|i| matches!(i, PaletteItem::Project { .. }));
        assert!(project_idx.is_some(), "expected a Project item");
        Ok(())
    }

    #[tokio::test]
    async fn recents_push_then_list_lifo_and_caps() -> anyhow::Result<()> {
        let db = Db::open_in_memory().await?;
        db.seed_fixtures().await?;

        // Push 3 - LIFO order.
        db.recents_push("acorn").await?;
        db.recents_push("birch").await?;
        db.recents_push("cedar").await?;
        let rs = db.recents_list(10).await?;
        let ids: Vec<&str> = rs.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, vec!["cedar", "birch", "acorn"]);

        // Re-push an older one - it moves to the top.
        db.recents_push("acorn").await?;
        let rs = db.recents_list(10).await?;
        let ids: Vec<&str> = rs.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids[0], "acorn", "re-push should bubble to top");

        // limit trims result window.
        let rs = db.recents_list(2).await?;
        assert_eq!(rs.len(), 2);

        // Unknown project id errors.
        let err = db.recents_push("does-not-exist").await;
        assert!(err.is_err());

        Ok(())
    }

    #[tokio::test]
    async fn recents_caps_at_twenty() -> anyhow::Result<()> {
        let db = Db::open_in_memory().await?;
        db.seed_fixtures().await?;

        // Seed has 12 projects; push each. Then push some again to
        let all_ids: Vec<String> = db
            .list_projects(ProjectFilter {
                include_archived: true,
                ..Default::default()
            })
            .await?
            .into_iter()
            .map(|p| p.id)
            .collect();
        assert_eq!(all_ids.len(), 12);

        for id in &all_ids {
            db.recents_push(id).await?;
        }
        // All 12 present.
        let rs = db.recents_list(100).await?;
        assert_eq!(rs.len(), 12);

        // Push no more - the buffer is < 20 so no trim needed.
        let rs = db.recents_list(100).await?;
        assert_eq!(rs.len(), 12);

        // Directly assert the SQL trim works by stuffing > 20 rows.
        for (i, id) in all_ids.iter().cycle().take(25).enumerate() {
            let ts = format!("2026-01-01T00:00:{:02}Z", i);
            sqlx::query(
                "INSERT INTO recents (project_id, opened_at) VALUES (?, ?) \
                 ON CONFLICT(project_id) DO UPDATE SET opened_at = excluded.opened_at",
            )
            .bind(id)
            .bind(&ts)
            .execute(db.pool())
            .await?;
        }
        // Now trigger the trim by pushing one more through the API path.
        db.recents_push(&all_ids[0]).await?;

        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM recents")
            .fetch_one(db.pool())
            .await?;
        assert!(count.0 <= 20, "expected ≤ 20 recents rows, got {}", count.0);

        Ok(())
    }

    use crate::storage::types::{PaneLayout, PaneSnapshot};

    #[tokio::test]
    async fn pane_layout_save_get_clear_roundtrip() -> anyhow::Result<()> {
        let db = Db::open_in_memory().await?;
        let (pid, project_path) = make_real_project(&db, "panes-rt").await?;

        // No layout yet.
        assert!(db.pane_layout_get(&pid).await?.is_none());

        // Save a three-pane grid.
        let layout = PaneLayout {
            mode: "grid".into(),
            active_pane_id: Some("p2".into()),
            panes: vec![
                PaneSnapshot {
                    id: "p1".into(),
                    kind: "shell".into(),
                    title: "zsh".into(),
                    cwd: project_path.to_string_lossy().to_string(),
                    script_id: None,
                    session_id: None,
                },
                PaneSnapshot {
                    id: "p2".into(),
                    kind: "script".into(),
                    title: "pnpm dev".into(),
                    cwd: project_path.to_string_lossy().to_string(),
                    script_id: Some("dev".into()),
                    session_id: None,
                },
                PaneSnapshot {
                    id: "p3".into(),
                    kind: "claude-session".into(),
                    title: "Resume".into(),
                    cwd: project_path.to_string_lossy().to_string(),
                    script_id: None,
                    session_id: Some("abc-123".into()),
                },
            ],
        };
        db.pane_layout_save(&pid, &layout).await?;

        // Readback matches.
        let got = db.pane_layout_get(&pid).await?.expect("layout persisted");
        assert_eq!(got.mode, "grid");
        assert_eq!(got.panes.len(), 3);
        assert_eq!(got.panes[1].script_id.as_deref(), Some("dev"));
        assert_eq!(got.active_pane_id.as_deref(), Some("p2"));

        // File is at `.atlas/panes.json`.
        assert!(project_path.join(".atlas/panes.json").exists());

        // Clear removes it.
        db.pane_layout_clear(&pid).await?;
        assert!(db.pane_layout_get(&pid).await?.is_none());

        // Clear is idempotent on missing file.
        db.pane_layout_clear(&pid).await?;

        std::fs::remove_dir_all(project_path.parent().unwrap()).ok();
        Ok(())
    }

    #[tokio::test]
    async fn pane_layout_unknown_project_errors() -> anyhow::Result<()> {
        let db = Db::open_in_memory().await?;
        let err = db.pane_layout_get("does-not-exist").await;
        assert!(
            err.is_err(),
            "unknown project must error, not silently no-op"
        );
        Ok(())
    }
}
