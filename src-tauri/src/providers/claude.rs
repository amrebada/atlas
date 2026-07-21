//! Claude Code provider — reads `~/.claude/projects/<slug>/*.jsonl` and
//! discovers installed skills (`~/.claude/skills`, `<project>/.claude/skills`,
//! plugin `skills/` dirs listed in `installed_plugins.json`).

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use super::shared::{
    canonicalize_or_self, derive_status, format_duration, home_dir, parse_ts, paths_equal,
    truncate_prompt,
};
use super::{ParsedSession, ResumeInvocation, SessionDetail, SessionProvider, ID_CLAUDE};

pub struct ClaudeProvider;

impl SessionProvider for ClaudeProvider {
    fn id(&self) -> &'static str {
        ID_CLAUDE
    }
    fn label(&self) -> &'static str {
        "Claude Code"
    }
    fn binary_name(&self) -> &'static str {
        "claude"
    }

    fn list_for_project(&self, project_path: &Path) -> anyhow::Result<Vec<ParsedSession>> {
        let canon = canonicalize_or_self(project_path);
        let Some(dir) = claude_dir_for_project(&canon)? else {
            return Ok(Vec::new());
        };

        let mut out: Vec<ParsedSession> = Vec::new();
        for entry in std::fs::read_dir(&dir)? {
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            match parse_session_file(&path) {
                Ok(Some(p)) => out.push(p),
                Ok(None) => {}
                Err(err) => {
                    tracing::trace!(?err, path = %path.display(), "claude: parse failed");
                }
            }
        }
        Ok(out)
    }

    fn resume_invocation(&self, detail: &SessionDetail) -> ResumeInvocation {
        let cwd = detail
            .cwd
            .clone()
            .unwrap_or_else(|| std::env::temp_dir().to_string_lossy().into_owned());
        ResumeInvocation {
            provider: ID_CLAUDE.into(),
            command: "claude".into(),
            args: vec!["--resume".into(), detail.id.clone()],
            cwd,
        }
    }

    fn new_invocation(&self, project_path: &Path) -> ResumeInvocation {
        ResumeInvocation {
            provider: ID_CLAUDE.into(),
            command: "claude".into(),
            args: Vec::new(),
            cwd: project_path.to_string_lossy().into_owned(),
        }
    }
}

// ---------- JSONL row schema ----------

#[derive(Debug, Deserialize)]
struct Row {
    #[serde(rename = "type")]
    kind: Option<String>,
    timestamp: Option<String>,
    cwd: Option<String>,
    #[serde(rename = "gitBranch")]
    git_branch: Option<String>,
    message: Option<Message>,
}

#[derive(Debug, Deserialize)]
struct Message {
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    content: Option<Value>,
    #[serde(default)]
    model: Option<String>,
}

// ---------- Slug discovery ----------

pub fn claude_dir_for_project(project_path: &Path) -> anyhow::Result<Option<PathBuf>> {
    let canon_target = canonicalize_or_self(project_path);

    let Some(root) = claude_projects_root() else {
        return Ok(None);
    };
    if !root.exists() {
        return Ok(None);
    }

    let expected_slug = path_to_slug(&canon_target);

    let direct = root.join(&expected_slug);
    if direct.is_dir() {
        if let Some(cwd) = read_cwd_from_dir(&direct)? {
            if paths_equal(&cwd, &canon_target) {
                return Ok(Some(direct));
            }
        }
    }

    for entry in std::fs::read_dir(&root)? {
        let entry = match entry {
            Ok(e) => e,
            Err(err) => {
                tracing::trace!(?err, "read_dir entry in claude projects root");
                continue;
            }
        };
        let slug_dir = entry.path();
        if !slug_dir.is_dir() {
            continue;
        }
        match read_cwd_from_dir(&slug_dir) {
            Ok(Some(cwd)) if paths_equal(&cwd, &canon_target) => return Ok(Some(slug_dir)),
            Ok(_) => {}
            Err(err) => tracing::trace!(?err, dir = %slug_dir.display(), "read_cwd_from_dir"),
        }
    }

    Ok(None)
}

/// `~/.claude` — the root every Claude Code scan (projects, skills,
/// plugins) hangs off. Home resolution matches the rest of this file.
pub fn claude_home() -> Option<PathBuf> {
    home_dir().map(|h| h.join(".claude"))
}

fn claude_projects_root() -> Option<PathBuf> {
    claude_home().map(|h| h.join("projects"))
}

fn path_to_slug(path: &Path) -> String {
    let s = path.to_string_lossy();
    let trimmed = s.trim_start_matches('/');
    let replaced = trimmed.replace(['/', '\\'], "-");
    format!("-{replaced}")
}

fn read_cwd_from_dir(dir: &Path) -> anyhow::Result<Option<PathBuf>> {
    let newest = newest_jsonl(dir)?;
    let Some(path) = newest else {
        return Ok(None);
    };
    read_cwd_from_file(&path)
}

fn read_cwd_from_file(path: &Path) -> anyhow::Result<Option<PathBuf>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    for (idx, line) in reader.lines().enumerate() {
        if idx > 500 {
            break;
        }
        let Ok(line) = line else { continue };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let row: Row = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(err) => {
                tracing::trace!(?err, path = %path.display(), "malformed jsonl line");
                continue;
            }
        };
        if let Some(cwd) = row.cwd {
            if !cwd.is_empty() {
                return Ok(Some(PathBuf::from(cwd)));
            }
        }
    }
    Ok(None)
}

fn newest_jsonl(dir: &Path) -> anyhow::Result<Option<PathBuf>> {
    let mut newest: Option<(SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(dir)? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let Ok(mtime) = meta.modified() else { continue };
        match &newest {
            Some((cur, _)) if *cur >= mtime => {}
            _ => newest = Some((mtime, path)),
        }
    }
    Ok(newest.map(|(_, p)| p))
}

/// Find the transcript file of a freshly-spawned `claude` session running in
/// `project_path`. Scans every `~/.claude/projects/*/` slug dir for a
/// `.jsonl` modified at or after `since` whose recorded `cwd` matches the
/// project, and returns the newest match.
///
/// Used by the pilot orchestrator to bind to the session it just launched
/// without having to reproduce Claude Code's slug-encoding scheme.
pub fn find_session_for_project(project_path: &Path, since: SystemTime) -> Option<PathBuf> {
    let root = claude_projects_root()?;
    let canon = canonicalize_or_self(project_path);
    // Tolerate a little clock skew between our `since` and file mtimes.
    let cutoff = since.checked_sub(Duration::from_secs(3)).unwrap_or(since);

    let mut best: Option<(SystemTime, PathBuf)> = None;
    for slug in std::fs::read_dir(&root).ok()?.flatten() {
        let slug_dir = slug.path();
        if !slug_dir.is_dir() {
            continue;
        }
        let Ok(files) = std::fs::read_dir(&slug_dir) else {
            continue;
        };
        for f in files.flatten() {
            let path = f.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let Ok(mtime) = f.metadata().and_then(|m| m.modified()) else {
                continue;
            };
            if mtime < cutoff {
                continue;
            }
            match read_cwd_from_file(&path) {
                Ok(Some(cwd)) if paths_equal(&cwd, &canon) => match &best {
                    Some((t, _)) if *t >= mtime => {}
                    _ => best = Some((mtime, path)),
                },
                _ => {}
            }
        }
    }
    best.map(|(_, p)| p)
}

// ---------- Single-file parser ----------

pub fn parse_session_file(path: &Path) -> anyhow::Result<Option<ParsedSession>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let mut first_user_prompt: Option<(DateTime<Utc>, String)> = None;
    let mut last_user_prompt: Option<(DateTime<Utc>, String)> = None;
    let mut last_timestamp: Option<DateTime<Utc>> = None;
    let mut turns: u32 = 0;
    let mut model: Option<String> = None;
    let mut branch: Option<String> = None;
    let mut cwd: Option<String> = None;

    for line in reader.lines() {
        let Ok(line) = line else { continue };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let row: Row = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(err) => {
                tracing::trace!(?err, "skip malformed jsonl line");
                continue;
            }
        };

        let ts = row.timestamp.as_deref().and_then(parse_ts);
        if let Some(t) = ts {
            last_timestamp = Some(last_timestamp.map_or(t, |cur| cur.max(t)));
        }

        if cwd.is_none() {
            if let Some(c) = row.cwd.as_ref() {
                if !c.is_empty() {
                    cwd = Some(c.clone());
                }
            }
        }
        if branch.is_none() {
            if let Some(b) = row.git_branch.as_ref() {
                if !b.is_empty() && b != "HEAD" {
                    branch = Some(b.clone());
                }
            }
        }

        match row.kind.as_deref() {
            Some("user") => {
                if let Some(msg) = &row.message {
                    if msg.role.as_deref().unwrap_or("user") != "user" {
                        continue;
                    }
                    if let Some(text) = extract_user_text(msg.content.as_ref()) {
                        let at = ts.unwrap_or_else(Utc::now);
                        if first_user_prompt.is_none() {
                            first_user_prompt = Some((at, text.clone()));
                        }
                        last_user_prompt = Some((at, text));
                        turns = turns.saturating_add(1);
                    }
                }
            }
            Some("assistant") => {
                if let Some(m) = row.message.as_ref().and_then(|m| m.model.clone()) {
                    if !m.is_empty() {
                        model = Some(m);
                    }
                }
            }
            _ => {}
        }
    }

    let Some((when, first_text)) = first_user_prompt else {
        return Ok(None);
    };
    let (last_ts, last_text) = last_user_prompt.unwrap_or_else(|| (when, first_text.clone()));
    let duration_end = last_timestamp.unwrap_or(last_ts);

    let id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("session")
        .to_string();

    let mtime = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let status = derive_status(duration_end, mtime);

    Ok(Some(ParsedSession {
        provider: ID_CLAUDE.into(),
        id,
        title: truncate_prompt(&first_text, 80),
        when,
        turns,
        duration: format_duration(duration_end - when),
        status,
        last: truncate_prompt(&last_text, 160),
        model,
        branch,
        cwd,
        source_path: Some(path.to_path_buf()),
    }))
}

fn extract_user_text(content: Option<&Value>) -> Option<String> {
    match content? {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Array(items) => {
            let mut buf = String::new();
            for item in items {
                if let Some(obj) = item.as_object() {
                    if obj.get("type").and_then(Value::as_str) == Some("text") {
                        if let Some(t) = obj.get("text").and_then(Value::as_str) {
                            if !buf.is_empty() {
                                buf.push('\n');
                            }
                            buf.push_str(t);
                        }
                    }
                }
            }
            if buf.is_empty() {
                None
            } else {
                Some(buf)
            }
        }
        _ => None,
    }
}

// ---------- Skills discovery ----------

/// One installed Claude Code skill (user, project, or plugin scope).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct ClaudeSkill {
    /// Invocation name: the skill's directory name, or `plugin:dir` for
    /// plugin skills. (The frontmatter `name:` is display-only.)
    pub name: String,
    /// Frontmatter `description:`, `""` when absent.
    pub description: String,
    /// `"user"` | `"project"` | `"plugin"`.
    pub scope: String,
    /// `Some(plugin name)` for plugin skills.
    pub plugin: Option<String>,
    /// Absolute path of the skill directory.
    pub path: String,
}

/// Discover every installed skill visible from `claude_home` (`~/.claude`)
/// plus, when given, `<project>/.claude/skills`. Pure over its base paths so
/// tests can point it at a temp layout. Missing or malformed files are
/// normal — they are skipped silently, never an error.
///
/// Sorted: project scope first, then user, then plugin; alphabetical within
/// each scope.
pub fn discover_skills(claude_home: &Path, project_path: Option<&Path>) -> Vec<ClaudeSkill> {
    let mut out: Vec<ClaudeSkill> = Vec::new();
    if let Some(project) = project_path {
        scan_skills_dir(
            &project.join(".claude").join("skills"),
            "project",
            None,
            &mut out,
        );
    }
    scan_skills_dir(&claude_home.join("skills"), "user", None, &mut out);
    discover_plugin_skills(claude_home, &mut out);
    out.sort_by(|a, b| {
        scope_rank(&a.scope)
            .cmp(&scope_rank(&b.scope))
            .then_with(|| a.name.cmp(&b.name))
    });
    out
}

fn scope_rank(scope: &str) -> u8 {
    match scope {
        "project" => 0,
        "user" => 1,
        _ => 2,
    }
}

/// Collect every `<dir>/SKILL.md` under `skills_dir` into `out`. Tolerates a
/// missing dir, stray files, and unreadable SKILL.md content.
fn scan_skills_dir(
    skills_dir: &Path,
    scope: &str,
    plugin: Option<&str>,
    out: &mut Vec<ClaudeSkill>,
) {
    let Ok(entries) = std::fs::read_dir(skills_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(dir_name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let skill_md = path.join("SKILL.md");
        if !skill_md.is_file() {
            continue;
        }
        let content = std::fs::read_to_string(&skill_md).unwrap_or_default();
        let (_display_name, description) = parse_skill_frontmatter(&content);
        let name = match plugin {
            Some(p) => format!("{p}:{dir_name}"),
            None => dir_name.to_string(),
        };
        out.push(ClaudeSkill {
            name,
            description: description.unwrap_or_default(),
            scope: scope.to_string(),
            plugin: plugin.map(str::to_string),
            path: path.to_string_lossy().into_owned(),
        });
    }
}

/// Plugin skills: `<claude_home>/plugins/installed_plugins.json` maps
/// `"<plugin>@<marketplace>"` to install records; each
/// `<installPath>/skills/<dir>/SKILL.md` is a skill invoked as
/// `<plugin>:<dir>`. Anything missing or malformed yields nothing.
fn discover_plugin_skills(claude_home: &Path, out: &mut Vec<ClaudeSkill>) {
    let manifest = claude_home.join("plugins").join("installed_plugins.json");
    let Ok(raw) = std::fs::read_to_string(&manifest) else {
        return;
    };
    let Ok(value) = serde_json::from_str::<Value>(&raw) else {
        return;
    };
    let Some(plugins) = value.get("plugins").and_then(Value::as_object) else {
        return;
    };
    for (key, installs) in plugins {
        let plugin_name = key.split('@').next().unwrap_or(key.as_str());
        if plugin_name.is_empty() {
            continue;
        }
        let Some(installs) = installs.as_array() else {
            continue;
        };
        for install in installs {
            let Some(install_path) = install.get("installPath").and_then(Value::as_str) else {
                continue;
            };
            scan_skills_dir(
                &Path::new(install_path).join("skills"),
                "plugin",
                Some(plugin_name),
                out,
            );
        }
    }
}

/// Extract `(name, description)` from the first fenced `---` frontmatter
/// block: line-based, single-line values, trimmed, surrounding quotes
/// stripped. `(None, None)` when there is no frontmatter.
pub fn parse_skill_frontmatter(content: &str) -> (Option<String>, Option<String>) {
    let mut lines = content.lines();
    match lines.next() {
        Some(first) if first.trim() == "---" => {}
        _ => return (None, None),
    }
    let mut name: Option<String> = None;
    let mut description: Option<String> = None;
    for line in lines {
        if line.trim() == "---" {
            break;
        }
        if let Some(v) = line.strip_prefix("name:") {
            if name.is_none() {
                name = Some(strip_quotes(v.trim()).to_string());
            }
        } else if let Some(v) = line.strip_prefix("description:") {
            if description.is_none() {
                description = Some(strip_quotes(v.trim()).to_string());
            }
        }
    }
    (name, description)
}

fn strip_quotes(s: &str) -> &str {
    let b = s.as_bytes();
    if b.len() >= 2
        && ((b[0] == b'"' && b[b.len() - 1] == b'"') || (b[0] == b'\'' && b[b.len() - 1] == b'\''))
    {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

// ---------- Tests ----------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;

    fn tempfile_path(prefix: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        let ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        p.push(format!("{prefix}-{ns}"));
        p
    }

    #[test]
    fn parses_minimal_session() {
        let tmp = tempfile_path("atlas_claude_min");
        {
            let mut f = File::create(&tmp).unwrap();
            writeln!(
                f,
                r#"{{"type":"user","timestamp":"2026-04-18T10:00:00Z","cwd":"/tmp/p","gitBranch":"main","message":{{"role":"user","content":"hello world"}}}}"#
            )
            .unwrap();
            writeln!(
                f,
                r#"{{"type":"assistant","timestamp":"2026-04-18T10:00:05Z","message":{{"role":"assistant","model":"claude-opus-4-7","content":"hi"}}}}"#
            )
            .unwrap();
            writeln!(
                f,
                r#"{{"type":"user","timestamp":"2026-04-18T10:02:14Z","message":{{"role":"user","content":"follow up?"}}}}"#
            )
            .unwrap();
        }

        let parsed = parse_session_file(&tmp).unwrap().expect("some session");
        assert_eq!(parsed.provider, ID_CLAUDE);
        assert_eq!(parsed.turns, 2);
        assert_eq!(parsed.title, "hello world");
        assert_eq!(parsed.last, "follow up?");
        assert_eq!(parsed.model.as_deref(), Some("claude-opus-4-7"));
        assert_eq!(parsed.branch.as_deref(), Some("main"));
        assert_eq!(parsed.cwd.as_deref(), Some("/tmp/p"));
        assert_eq!(parsed.duration, "2m 14s");

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn skips_malformed_and_tool_results() {
        let tmp = tempfile_path("atlas_claude_tolerant");
        {
            let mut f = File::create(&tmp).unwrap();
            writeln!(f, "{{ not json").unwrap();
            writeln!(
                f,
                r#"{{"type":"queue-operation","operation":"enqueue","timestamp":"2026-04-18T09:00:00Z"}}"#
            )
            .unwrap();
            writeln!(
                f,
                r#"{{"type":"user","timestamp":"2026-04-18T10:00:00Z","cwd":"/tmp/p","message":{{"role":"user","content":"first"}}}}"#
            )
            .unwrap();
            writeln!(
                f,
                r#"{{"type":"user","timestamp":"2026-04-18T10:00:01Z","message":{{"role":"user","content":[{{"type":"tool_result","content":"ok","tool_use_id":"x"}}]}}}}"#
            )
            .unwrap();
            writeln!(
                f,
                r#"{{"type":"user","timestamp":"2026-04-18T10:01:00Z","message":{{"role":"user","content":[{{"type":"text","text":"second"}}]}}}}"#
            )
            .unwrap();
        }

        let parsed = parse_session_file(&tmp).unwrap().expect("some session");
        assert_eq!(parsed.turns, 2);
        assert_eq!(parsed.title, "first");
        assert_eq!(parsed.last, "second");

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn path_to_slug_matches_claude_encoding() {
        assert_eq!(
            path_to_slug(Path::new("/Users/amre/workspace/atlas")),
            "-Users-amre-workspace-atlas"
        );
        assert_eq!(
            path_to_slug(Path::new("/tmp/one-day-build/cli")),
            "-tmp-one-day-build-cli"
        );
    }

    // ---------- Skills discovery ----------

    fn write_skill(dir: &Path, frontmatter: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), frontmatter).unwrap();
    }

    #[test]
    fn skill_frontmatter_parses_name_and_description() {
        let (name, desc) = parse_skill_frontmatter(
            "---\nname: my-skill\ndescription: Does useful things\n---\n\nBody text.\n",
        );
        assert_eq!(name.as_deref(), Some("my-skill"));
        assert_eq!(desc.as_deref(), Some("Does useful things"));
    }

    #[test]
    fn skill_frontmatter_strips_surrounding_quotes() {
        let (name, desc) = parse_skill_frontmatter(
            "---\nname: \"quoted-name\"\ndescription: 'single: quoted, value'\n---\n",
        );
        assert_eq!(name.as_deref(), Some("quoted-name"));
        assert_eq!(desc.as_deref(), Some("single: quoted, value"));
    }

    #[test]
    fn skill_frontmatter_tolerates_missing_description() {
        let (name, desc) = parse_skill_frontmatter("---\nname: lonely\n---\nBody.\n");
        assert_eq!(name.as_deref(), Some("lonely"));
        assert_eq!(desc, None);
    }

    #[test]
    fn skill_frontmatter_none_without_fences() {
        let (name, desc) = parse_skill_frontmatter("# Just a heading\n\nname: not-frontmatter\n");
        assert_eq!(name, None);
        assert_eq!(desc, None);
    }

    #[test]
    fn discovers_skills_across_scopes_sorted() {
        let home = tempfile::tempdir().unwrap(); // stands in for ~/.claude
        let project = tempfile::tempdir().unwrap();
        let plugin_install = tempfile::tempdir().unwrap();

        // User scope: two real skills + a dir without SKILL.md + a stray file.
        write_skill(
            &home.path().join("skills").join("zeta"),
            "---\nname: Zeta Display\ndescription: User zeta\n---\n",
        );
        write_skill(
            &home.path().join("skills").join("alpha"),
            "no frontmatter\n",
        );
        std::fs::create_dir_all(home.path().join("skills").join("not-a-skill")).unwrap();
        std::fs::write(home.path().join("skills").join("stray.md"), "x").unwrap();

        // Project scope.
        let proj_skill = project.path().join(".claude").join("skills").join("proj-a");
        write_skill(&proj_skill, "---\ndescription: Project helper\n---\n");

        // Plugin scope: manifest points at plugin_install; a cache dir with a
        // SKILL.md must NOT be picked up (only installPath entries count).
        write_skill(
            &plugin_install.path().join("skills").join("tool"),
            "---\nname: tool\ndescription: Plugin tool\n---\n",
        );
        write_skill(
            &plugin_install.path().join("skills").join("helper"),
            "---\ndescription: Plugin helper\n---\n",
        );
        write_skill(
            &home
                .path()
                .join("plugins")
                .join("cache")
                .join("mkt")
                .join("repo")
                .join("skills")
                .join("cached"),
            "---\ndescription: must not appear\n---\n",
        );
        let manifest = serde_json::json!({
            "version": 2,
            "plugins": {
                "myplug@marketplace": [
                    { "installPath": plugin_install.path().to_string_lossy() }
                ]
            }
        });
        std::fs::write(
            home.path().join("plugins").join("installed_plugins.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .unwrap();

        let skills = discover_skills(home.path(), Some(project.path()));
        let names: Vec<&str> = skills.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["proj-a", "alpha", "zeta", "myplug:helper", "myplug:tool"]
        );
        let scopes: Vec<&str> = skills.iter().map(|s| s.scope.as_str()).collect();
        assert_eq!(scopes, vec!["project", "user", "user", "plugin", "plugin"]);
        assert_eq!(skills[0].description, "Project helper");
        assert_eq!(skills[0].plugin, None);
        assert_eq!(skills[0].path, proj_skill.to_string_lossy());
        assert_eq!(skills[1].description, ""); // no frontmatter => empty
        assert_eq!(skills[2].description, "User zeta"); // dir name wins over frontmatter name
        assert_eq!(skills[3].plugin.as_deref(), Some("myplug"));
        assert!(!names.contains(&"myplug:cached"));
    }

    #[test]
    fn discovery_without_project_path_has_no_project_scope() {
        let home = tempfile::tempdir().unwrap();
        write_skill(
            &home.path().join("skills").join("one"),
            "---\ndescription: only user\n---\n",
        );

        let skills = discover_skills(home.path(), None);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].scope, "user");

        // A project path with no .claude/skills dir is tolerated silently.
        let empty_project = tempfile::tempdir().unwrap();
        let skills = discover_skills(home.path(), Some(empty_project.path()));
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].scope, "user");
    }

    #[test]
    fn malformed_installed_plugins_json_yields_user_skills_only() {
        let home = tempfile::tempdir().unwrap();
        write_skill(
            &home.path().join("skills").join("one"),
            "---\ndescription: still here\n---\n",
        );
        std::fs::create_dir_all(home.path().join("plugins")).unwrap();
        std::fs::write(
            home.path().join("plugins").join("installed_plugins.json"),
            "{ not json",
        )
        .unwrap();

        let skills = discover_skills(home.path(), None);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "one");
        assert_eq!(skills[0].scope, "user");
    }
}
