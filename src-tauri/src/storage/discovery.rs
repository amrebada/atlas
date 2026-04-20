//! Project discovery walker - finds `.git` directories under a root and

use crate::storage::types::Lang;
use ignore::WalkBuilder;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// A single repository surfaced by `scan_root`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredRepo {
    pub path: PathBuf,
    pub name: String,
    pub language: Lang,
}

/// Directory names we never descend into. These are on top of whatever
const DENY_DIRS: &[&str] = &[
    "node_modules",
    "target",
    ".venv",
    "venv",
    "dist",
    "build",
    ".next",
    ".cache",
];

/// Walk `root` with a progress callback. The callback fires once per
pub fn scan_root_with_progress<F>(
    root: &Path,
    depth: u8,
    mut on_found: F,
) -> anyhow::Result<Vec<DiscoveredRepo>>
where
    F: FnMut(&Path, usize),
{
    scan_root_inner(root, depth, &mut on_found)
}

/// Walk `root` up to `depth` levels deep collecting every directory that
pub fn scan_root(root: &Path, depth: u8) -> anyhow::Result<Vec<DiscoveredRepo>> {
    scan_root_inner(root, depth, &mut |_, _| {})
}

#[tracing::instrument(
    level = "info",
    skip(on_progress),
    fields(root = %root.display(), depth),
)]
fn scan_root_inner(
    root: &Path,
    depth: u8,
    on_progress: &mut dyn FnMut(&Path, usize),
) -> anyhow::Result<Vec<DiscoveredRepo>> {
    if !root.exists() {
        anyhow::bail!("root does not exist: {}", root.display());
    }

    let start = std::time::Instant::now();

    // Ancestor-skip set. When we find `<dir>/.git`, we insert `<dir>` here;
    let known_repos: Arc<Mutex<HashSet<PathBuf>>> = Arc::new(Mutex::new(HashSet::new()));
    let filter_known = known_repos.clone();

    let mut builder = WalkBuilder::new(root);
    builder
        .max_depth(Some(depth as usize))
        .hidden(false)
        .git_ignore(false)
        .git_exclude(false)
        .git_global(false)
        .ignore(false)
        .parents(false)
        .follow_links(false)
        .filter_entry(move |entry| {
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if !is_dir {
                return true;
            }

            // Deny heavy build-output / package dirs outright.
            if let Some(name) = entry.file_name().to_str() {
                if DENY_DIRS.contains(&name) {
                    return false;
                }
            }

            // Skip descent into anything already identified as a repo.
            let path = entry.path();
            let Ok(known) = filter_known.lock() else {
                return true;
            };
            let mut cur = path.parent();
            while let Some(p) = cur {
                if known.contains(p) {
                    return false;
                }
                cur = p.parent();
            }
            true
        });

    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut out: Vec<DiscoveredRepo> = Vec::new();

    for result in builder.build() {
        let entry = match result {
            Ok(e) => e,
            Err(err) => {
                tracing::trace!(?err, "walk error, skipping");
                continue;
            }
        };

        // Report live progress for every directory we enter - NOT just on
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            on_progress(entry.path(), out.len());
        }

        // We hunt for `<dir>/.git` - either a dir (normal repo) or a file
        if entry.file_name() != ".git" {
            continue;
        }

        let repo_dir = match entry.path().parent() {
            Some(p) => p,
            None => continue,
        };

        let canonical = match repo_dir.canonicalize() {
            Ok(p) => p,
            Err(err) => {
                tracing::trace!(path = %repo_dir.display(), ?err, "canonicalize failed");
                continue;
            }
        };

        if !seen.insert(canonical.clone()) {
            continue;
        }

        // Record in the shared set so filter_entry can prune descent into
        if let Ok(mut known) = known_repos.lock() {
            known.insert(canonical.clone());
        }

        out.push(classify(&canonical));
        on_progress(&canonical, out.len());
    }

    // Stable order for deterministic tests.
    out.sort_by(|a, b| a.path.cmp(&b.path));
    tracing::info!(
        elapsed_ms = start.elapsed().as_millis() as u64,
        found = out.len(),
        "walk complete",
    );
    Ok(out)
}

/// Cheap per-repo metadata lookup: basename for `name`, file-extension
pub fn classify(repo_path: &Path) -> DiscoveredRepo {
    let name = repo_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    let language = infer_language(repo_path);

    DiscoveredRepo {
        path: repo_path.to_path_buf(),
        name,
        language,
    }
}

/// Order matters - we probe the most diagnostic marker first.
fn infer_language(repo_path: &Path) -> Lang {
    // Read the top-level entries once. If we can't even list the repo
    let Ok(entries) = std::fs::read_dir(repo_path) else {
        return Lang::Other;
    };

    // Collect filenames into a small set; case-insensitive lookup below.
    let mut names: HashSet<String> = HashSet::new();
    for entry in entries.flatten() {
        if let Some(name) = entry.file_name().to_str() {
            names.insert(name.to_ascii_lowercase());
        }
    }

    let has = |needle: &str| names.contains(needle);
    let has_prefix = |prefix: &str| names.iter().any(|n| n.starts_with(prefix));

    // Rust
    if has("cargo.toml") {
        return Lang::Rust;
    }
    // package.json → TS if tsconfig present, else JS
    if has("package.json") {
        if has("tsconfig.json") {
            return Lang::TypeScript;
        }
        return Lang::JavaScript;
    }
    // Go
    if has("go.mod") {
        return Lang::Go;
    }
    // Python
    if has("pyproject.toml") || has("requirements.txt") {
        return Lang::Python;
    }
    // Ruby
    if has("gemfile") {
        return Lang::Ruby;
    }
    // Swift
    if has("package.swift") {
        return Lang::Swift;
    }
    // Java - pom.xml or any build.gradle*
    if has("pom.xml") || has_prefix("build.gradle") {
        return Lang::Java;
    }

    Lang::Other
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Build a tempdir with a fake repo for `classify` tests.
    fn mkrepo(parent: &Path, name: &str, files: &[&str]) -> PathBuf {
        let dir = parent.join(name);
        fs::create_dir_all(dir.join(".git")).expect("mkdir .git");
        for f in files {
            fs::write(dir.join(f), b"").expect("touch");
        }
        dir
    }

    #[test]
    fn classify_rust() {
        let tmp = tempdir("classify-rust");
        let repo = mkrepo(&tmp, "rs-proj", &["Cargo.toml"]);
        let got = classify(&repo);
        assert_eq!(got.language, Lang::Rust);
        assert_eq!(got.name, "rs-proj");
    }

    #[test]
    fn classify_typescript_when_tsconfig_present() {
        let tmp = tempdir("classify-ts");
        let repo = mkrepo(&tmp, "ts-proj", &["package.json", "tsconfig.json"]);
        assert_eq!(classify(&repo).language, Lang::TypeScript);
    }

    #[test]
    fn classify_javascript_when_tsconfig_absent() {
        let tmp = tempdir("classify-js");
        let repo = mkrepo(&tmp, "js-proj", &["package.json"]);
        assert_eq!(classify(&repo).language, Lang::JavaScript);
    }

    #[test]
    fn classify_python_via_pyproject() {
        let tmp = tempdir("classify-py");
        let repo = mkrepo(&tmp, "py-proj", &["pyproject.toml"]);
        assert_eq!(classify(&repo).language, Lang::Python);
    }

    #[test]
    fn classify_python_via_requirements() {
        let tmp = tempdir("classify-py2");
        let repo = mkrepo(&tmp, "py-proj2", &["requirements.txt"]);
        assert_eq!(classify(&repo).language, Lang::Python);
    }

    #[test]
    fn classify_go() {
        let tmp = tempdir("classify-go");
        let repo = mkrepo(&tmp, "go-proj", &["go.mod"]);
        assert_eq!(classify(&repo).language, Lang::Go);
    }

    #[test]
    fn classify_java_via_gradle() {
        let tmp = tempdir("classify-java");
        let repo = mkrepo(&tmp, "j-proj", &["build.gradle.kts"]);
        assert_eq!(classify(&repo).language, Lang::Java);
    }

    #[test]
    fn classify_other_when_unknown() {
        let tmp = tempdir("classify-other");
        let repo = mkrepo(&tmp, "mystery", &["README.md"]);
        assert_eq!(classify(&repo).language, Lang::Other);
    }

    #[test]
    fn scan_root_finds_repos_and_skips_node_modules() -> anyhow::Result<()> {
        let tmp = tempdir("scan");
        mkrepo(&tmp, "alpha", &["Cargo.toml"]);
        mkrepo(&tmp, "beta", &["package.json"]);
        // A repo nested inside a deny-listed dir must NOT be returned.
        mkrepo(&tmp.join("node_modules"), "hidden", &["package.json"]);

        let mut got = scan_root(&tmp, 3)?;
        got.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].name, "alpha");
        assert_eq!(got[0].language, Lang::Rust);
        assert_eq!(got[1].name, "beta");
        assert_eq!(got[1].language, Lang::JavaScript);
        Ok(())
    }

    #[test]
    fn scan_root_respects_depth() -> anyhow::Result<()> {
        let tmp = tempdir("scan-depth");
        mkrepo(&tmp.join("a").join("b").join("c"), "deep", &["Cargo.toml"]);
        // Depth 2 shouldn't reach it.
        let shallow = scan_root(&tmp, 2)?;
        assert!(shallow.is_empty(), "depth 2 should not find deep repo");
        // Depth 5 should.
        let deep = scan_root(&tmp, 5)?;
        assert_eq!(deep.len(), 1);
        Ok(())
    }

    fn tempdir(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "atlas-discovery-{}-{}-{}",
            tag,
            std::process::id(),
            unique()
        ));
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn unique() -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        N.fetch_add(1, Ordering::Relaxed)
    }
}
