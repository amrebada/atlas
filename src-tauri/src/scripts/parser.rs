//! Script source parsers - `package.json` scripts and `Makefile` targets.

use std::collections::HashSet;
use std::path::Path;

use crate::storage::types::{Script, ScriptGroup};

/// Parse a `package.json` file at `path` and extract its `scripts` block as
pub fn parse_package_json(path: &Path) -> anyhow::Result<Vec<Script>> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    Ok(parse_package_json_bytes(&bytes))
}

/// Same as `parse_package_json` but operates on raw bytes - used by the
pub fn parse_package_json_bytes(bytes: &[u8]) -> Vec<Script> {
    let value: serde_json::Value = match serde_json::from_slice(bytes) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let scripts_obj = match value.get("scripts").and_then(|v| v.as_object()) {
        Some(o) => o,
        None => return Vec::new(),
    };

    let mut out = Vec::with_capacity(scripts_obj.len());
    let mut seen_default = false;
    for (name, cmd_val) in scripts_obj {
        let cmd = match cmd_val.as_str() {
            Some(s) => s,
            None => continue, // skip arrays / objects / null
        };
        let name = name.trim();
        if name.is_empty() {
            continue;
        }

        let group = group_for_name(name);
        let id = format!("npm:{name}");
        let is_default = !seen_default && matches!(group, ScriptGroup::Run);
        if is_default {
            seen_default = true;
        }

        out.push(Script {
            id,
            name: name.to_string(),
            cmd: format!("npm run {name}"),
            desc: Some(cmd.to_string()),
            group,
            default: if is_default { Some(true) } else { None },
            icon: Some(icon_for_group(&group_for_name(name)).to_string()),
        });
    }

    out
}

/// Parse a `Makefile` at `path`. Returns `Ok(vec![])` when the file is
pub fn parse_makefile(path: &Path) -> anyhow::Result<Vec<Script>> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    Ok(parse_makefile_text(&text))
}

/// Same as `parse_makefile` but on a string slice. Used by tests.
pub fn parse_makefile_text(text: &str) -> Vec<Script> {
    let mut out = Vec::new();
    let mut seen_default = false;
    let mut seen_names: HashSet<String> = HashSet::new();

    for raw in text.lines() {
        // Recipe lines start with a TAB. Skip them so we never confuse a
        if raw.starts_with('\t') {
            continue;
        }
        let line = strip_comment(raw);
        let trimmed = line.trim_start();

        // Targets must begin at column 0 (no leading whitespace beyond
        if trimmed.len() != line.len() {
            continue;
        }
        if trimmed.is_empty() {
            continue;
        }

        let Some((lhs, _rhs)) = split_target(trimmed) else {
            continue;
        };

        let lhs = lhs.trim();
        if !is_valid_target_name(lhs) {
            continue;
        }
        // Special-case `.PHONY` - it's a directive, not a runnable target.
        if lhs.eq_ignore_ascii_case(".phony") {
            continue;
        }

        if !seen_names.insert(lhs.to_string()) {
            continue;
        }

        let group = group_for_name(lhs);
        let is_default = !seen_default && matches!(group, ScriptGroup::Run);
        if is_default {
            seen_default = true;
        }

        out.push(Script {
            id: format!("make:{lhs}"),
            name: lhs.to_string(),
            cmd: format!("make {lhs}"),
            desc: None,
            group,
            default: if is_default { Some(true) } else { None },
            icon: Some(icon_for_group(&group_for_name(lhs)).to_string()),
        });
    }

    out
}

/// Parse a Taskfile (https://taskfile.dev) at `path` and extract its
pub fn parse_taskfile(path: &Path) -> anyhow::Result<Vec<Script>> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    Ok(parse_taskfile_bytes(&bytes))
}

/// Same as `parse_taskfile` but operates on raw bytes. Used by unit tests.
pub fn parse_taskfile_bytes(bytes: &[u8]) -> Vec<Script> {
    let value: serde_yml::Value = match serde_yml::from_slice(bytes) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let tasks = match value.get("tasks").and_then(|v| v.as_mapping()) {
        Some(m) => m,
        None => return Vec::new(),
    };

    let mut out = Vec::with_capacity(tasks.len());
    let mut seen_default = false;
    for (name_v, task_v) in tasks {
        let name = match name_v.as_str() {
            Some(s) => s.trim(),
            None => continue,
        };
        if name.is_empty() {
            continue;
        }

        // Desc is only pulled from the object form.
        let desc = task_v
            .get("desc")
            .and_then(|v| v.as_str())
            .or_else(|| task_v.get("summary").and_then(|v| v.as_str()))
            .map(|s| s.trim().to_string());

        let group = group_for_name(name);
        let id = format!("task:{name}");
        let is_default = !seen_default && matches!(group, ScriptGroup::Run);
        if is_default {
            seen_default = true;
        }

        out.push(Script {
            id,
            name: name.to_string(),
            cmd: format!("task {name}"),
            desc,
            group,
            default: if is_default { Some(true) } else { None },
            icon: Some(icon_for_group(&group_for_name(name)).to_string()),
        });
    }
    out
}

/// Discover every known script source under `project_path` and merge into
pub fn discover_scripts(project_path: &Path) -> anyhow::Result<Vec<Script>> {
    let mut out = Vec::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();

    let pkg_path = project_path.join("package.json");
    for s in parse_package_json(&pkg_path)? {
        if seen.insert((s.name.clone(), s.cmd.clone())) {
            out.push(s);
        }
    }

    let mk_path = project_path.join("Makefile");
    for s in parse_makefile(&mk_path)? {
        if seen.insert((s.name.clone(), s.cmd.clone())) {
            out.push(s);
        }
    }

    // Taskfile supports both `.yml` and `.yaml` extensions; also allow the
    for candidate in [
        "Taskfile.yml",
        "Taskfile.yaml",
        "Taskfile.dist.yml",
        "Taskfile.dist.yaml",
    ] {
        let tf_path = project_path.join(candidate);
        for s in parse_taskfile(&tf_path)? {
            if seen.insert((s.name.clone(), s.cmd.clone())) {
                out.push(s);
            }
        }
    }

    Ok(out)
}

// ---------- helpers ----------

fn group_for_name(name: &str) -> ScriptGroup {
    let n = name.to_ascii_lowercase();
    if n.starts_with("dev") || n.starts_with("start") || n == "serve" || n.starts_with("watch") {
        ScriptGroup::Run
    } else if n.starts_with("build") || n.starts_with("compile") || n.starts_with("bundle") {
        ScriptGroup::Build
    } else if n == "test"
        || n.starts_with("test")
        || n == "lint"
        || n.starts_with("lint")
        || n == "typecheck"
        || n == "tsc"
        || n.starts_with("check")
        || n.starts_with("format")
        || n.starts_with("fmt")
        || n.starts_with("e2e")
        || n.starts_with("vet")
    {
        ScriptGroup::Check
    } else {
        ScriptGroup::Util
    }
}

fn icon_for_group(group: &ScriptGroup) -> &'static str {
    match group {
        ScriptGroup::Run => "play",
        ScriptGroup::Build => "package",
        ScriptGroup::Check => "check",
        ScriptGroup::Util => "tool",
    }
}

/// Strip `# …` comments (Make uses `#`, not `//`). Quoted strings are
fn strip_comment(line: &str) -> &str {
    match line.find('#') {
        Some(i) => &line[..i],
        None => line,
    }
}

/// Find the first `:` that introduces a target body. Rejects lines whose
fn split_target(line: &str) -> Option<(&str, &str)> {
    let bytes = line.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b':' {
            // Reject `::` (double-colon rule - we don't support these).
            if bytes.get(i + 1) == Some(&b':') {
                return None;
            }
            // Reject `:=` (assignment).
            if bytes.get(i + 1) == Some(&b'=') {
                return None;
            }
            // Reject preceding `=`/`?`/`+`/`!` - those compose into
            return Some((&line[..i], &line[i + 1..]));
        }
        if b == b'=' {
            // `name = value` style assignment encountered before any `:`  -
            return None;
        }
    }
    None
}

/// A valid Make target identifier per our narrow heuristic.
fn is_valid_target_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    // Reject pattern rules, variable refs, whitespace, and embedded `:`.
    for ch in name.chars() {
        if ch.is_whitespace() || ch == '$' || ch == '%' || ch == ':' || ch == '\\' {
            return false;
        }
    }
    // First char must be alpha or underscore (allow leading `.` for things
    let first = name.chars().next().unwrap_or(' ');
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    true
}

// ---------- tests ----------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_package_json_basic_buckets_groups() {
        let json = br#"{
            "name": "demo",
            "scripts": {
                "dev": "vite",
                "build": "vite build",
                "test": "vitest",
                "lint": "eslint .",
                "typecheck": "tsc --noEmit",
                "clean": "rm -rf dist"
            }
        }"#;
        let scripts = parse_package_json_bytes(json);
        assert_eq!(scripts.len(), 6, "want 6 scripts, got {}", scripts.len());

        let by_name = |n: &str| scripts.iter().find(|s| s.name == n).expect(n);
        assert!(matches!(by_name("dev").group, ScriptGroup::Run));
        assert!(matches!(by_name("build").group, ScriptGroup::Build));
        assert!(matches!(by_name("test").group, ScriptGroup::Check));
        assert!(matches!(by_name("lint").group, ScriptGroup::Check));
        assert!(matches!(by_name("typecheck").group, ScriptGroup::Check));
        assert!(matches!(by_name("clean").group, ScriptGroup::Util));

        // Default = first run-group entry → "dev"
        assert_eq!(by_name("dev").default, Some(true));
        // No others should be default
        let defaults = scripts.iter().filter(|s| s.default == Some(true)).count();
        assert_eq!(defaults, 1, "exactly one default");

        // cmd uses `npm run <name>` so the runner is unambiguous; the raw
        assert_eq!(by_name("dev").cmd, "npm run dev");
        assert_eq!(by_name("dev").desc.as_deref(), Some("vite"));
    }

    #[test]
    fn parse_package_json_returns_empty_on_missing_scripts() {
        let json = br#"{"name":"demo","version":"1.0.0"}"#;
        assert!(parse_package_json_bytes(json).is_empty());

        // Garbage in → empty out (never panic).
        assert!(parse_package_json_bytes(b"not even close to json").is_empty());
        // Empty object → empty.
        assert!(parse_package_json_bytes(b"{}").is_empty());
    }

    #[test]
    fn parse_package_json_skips_non_string_values() {
        // Some real-world package.json files have arrays or nested objects
        let json = br#"{
            "scripts": {
                "good": "echo ok",
                "bad": ["echo", "no"],
                "alsobad": null
            }
        }"#;
        let scripts = parse_package_json_bytes(json);
        assert_eq!(scripts.len(), 1);
        assert_eq!(scripts[0].name, "good");
    }

    #[test]
    fn parse_makefile_finds_simple_targets() {
        let mk = "\
.PHONY: dev test build install
dev:
\tcargo run

build:
\tcargo build --release

test:
\tcargo test

install: build
\tcp target/release/foo /usr/local/bin/

# this is a comment, not a target
VAR := value
NESTED:= other
";
        let scripts = parse_makefile_text(mk);
        let names: Vec<&str> = scripts.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["dev", "build", "test", "install"]);

        let dev = scripts.iter().find(|s| s.name == "dev").unwrap();
        assert!(matches!(dev.group, ScriptGroup::Run));
        assert_eq!(dev.cmd, "make dev");
        assert_eq!(dev.default, Some(true));

        let build = scripts.iter().find(|s| s.name == "build").unwrap();
        assert!(matches!(build.group, ScriptGroup::Build));
        assert_eq!(build.default, None);
    }

    #[test]
    fn parse_makefile_rejects_assignments_and_pattern_rules() {
        let mk = "\
CC := gcc
CFLAGS = -O2
%.o: %.c
\t$(CC) -c $<

real-target:
\techo go
";
        let scripts = parse_makefile_text(mk);
        let names: Vec<&str> = scripts.iter().map(|s| s.name.as_str()).collect();
        // Only `real-target`... but our identifier filter rejects `-` since
        assert_eq!(names, vec!["real-target"]);
    }

    #[test]
    fn parse_taskfile_handles_rich_and_shorthand_forms() {
        let yaml = br#"
version: '3'
tasks:
  dev:
    desc: Run the dev server
    cmds:
      - go run ./cmd/server
  build:
    desc: Build the binary
    cmds:
      - go build -o bin/app ./cmd/server
  test:
    summary: Run the test suite
    cmds:
      - go test ./...
  lint: golangci-lint run
  clean:
    cmds:
      - rm -rf bin/
"#;
        let scripts = parse_taskfile_bytes(yaml);
        assert_eq!(scripts.len(), 5, "want 5 tasks, got {}", scripts.len());

        let by_name = |n: &str| scripts.iter().find(|s| s.name == n).expect(n);

        // Groups via the shared heuristic.
        assert!(matches!(by_name("dev").group, ScriptGroup::Run));
        assert!(matches!(by_name("build").group, ScriptGroup::Build));
        assert!(matches!(by_name("test").group, ScriptGroup::Check));
        assert!(matches!(by_name("lint").group, ScriptGroup::Check));
        assert!(matches!(by_name("clean").group, ScriptGroup::Util));

        // cmd uses the `task` binary so env / deps declared in the Taskfile
        assert_eq!(by_name("dev").cmd, "task dev");
        assert_eq!(by_name("lint").cmd, "task lint");

        // Desc comes from `desc:` or falls back to `summary:`.
        assert_eq!(by_name("dev").desc.as_deref(), Some("Run the dev server"));
        assert_eq!(by_name("test").desc.as_deref(), Some("Run the test suite"));
        // Shorthand task (just a string) has no desc.
        assert_eq!(by_name("lint").desc, None);

        // First run-group entry is default.
        assert_eq!(by_name("dev").default, Some(true));
        assert_eq!(
            scripts.iter().filter(|s| s.default == Some(true)).count(),
            1,
            "exactly one default"
        );
    }

    #[test]
    fn parse_taskfile_returns_empty_on_invalid_yaml() {
        assert!(parse_taskfile_bytes(b"not: : valid: yaml: ::").is_empty());
        assert!(parse_taskfile_bytes(b"").is_empty());
        // Valid YAML but no `tasks:` key.
        assert!(parse_taskfile_bytes(b"version: '3'\nincludes:\n  foo: ./foo").is_empty());
    }

    #[test]
    fn discover_scripts_picks_up_taskfile() {
        let tmp = std::env::temp_dir().join(format!(
            "atlas-taskfile-discover-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        std::fs::create_dir_all(&tmp).unwrap();

        std::fs::write(
            tmp.join("Taskfile.yml"),
            b"version: '3'\ntasks:\n  dev:\n    cmds: [go run ./cmd/server]\n  build:\n    cmds: [go build ./...]\n",
        )
        .unwrap();

        let scripts = discover_scripts(&tmp).unwrap();
        let names: Vec<&str> = scripts.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"dev"));
        assert!(names.contains(&"build"));

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn discover_scripts_dedupes_across_sources() {
        // Set up a tempdir with both a package.json and a Makefile that
        let tmp = std::env::temp_dir().join(format!(
            "atlas-scripts-discover-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        std::fs::create_dir_all(&tmp).unwrap();

        std::fs::write(
            tmp.join("package.json"),
            br#"{"scripts":{"dev":"vite","build":"vite build"}}"#,
        )
        .unwrap();
        std::fs::write(
            tmp.join("Makefile"),
            "build:\n\tmake -C subdir\n\nlint:\n\tmake -C subdir lint\n",
        )
        .unwrap();

        let scripts = discover_scripts(&tmp).unwrap();
        // package.json contributes dev + build; Makefile contributes
        let names: Vec<&str> = scripts.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"dev"));
        assert!(names.contains(&"lint"));
        // Both `build` entries survive because the cmd differs.
        assert_eq!(names.iter().filter(|n| **n == "build").count(), 2);

        std::fs::remove_dir_all(&tmp).ok();
    }
}
