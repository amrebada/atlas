//! Project metrics - lines of code + on-disk size.

use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use ignore::WalkBuilder;
use rayon::prelude::*;

const BINARY_PROBE_BYTES: usize = 8 * 1024;
const MAX_LOC_BYTES: u64 = 2 * 1024 * 1024;

pub struct ProjectMetrics {
    pub loc: u64,
    pub size_bytes: u64,
}

pub fn compute(project_path: &Path) -> anyhow::Result<ProjectMetrics> {
    if !project_path.exists() {
        anyhow::bail!("path does not exist: {}", project_path.display());
    }

    let paths: Vec<_> = WalkBuilder::new(project_path)
        .hidden(false)
        .parents(true)
        .git_ignore(true)
        .git_exclude(true)
        .follow_links(false)
        .build()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
        .map(|e| e.into_path())
        .collect();

    let loc = AtomicU64::new(0);
    let size = AtomicU64::new(0);

    paths.par_iter().for_each(|path| {
        let Ok(meta) = path.metadata() else { return };
        let len = meta.len();
        size.fetch_add(len, Ordering::Relaxed);

        if len == 0 || len > MAX_LOC_BYTES {
            return;
        }

        let Ok(mut f) = File::open(path) else { return };
        let mut probe = [0u8; BINARY_PROBE_BYTES];
        let read = match f.read(&mut probe) {
            Ok(n) => n,
            Err(_) => return,
        };
        if probe[..read].contains(&0) {
            return;
        }

        // Re-open: `f` has been consumed by the probe read, and seeking
        let Ok(f) = File::open(path) else { return };
        let reader = BufReader::new(f);
        let n = reader.lines().map_while(Result::ok).count() as u64;
        loc.fetch_add(n, Ordering::Relaxed);
    });

    Ok(ProjectMetrics {
        loc: loc.load(Ordering::Relaxed),
        size_bytes: size.load(Ordering::Relaxed),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn counts_text_lines_and_size() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), "one\ntwo\nthree\n").unwrap();
        fs::write(dir.path().join("b.rs"), "fn main() {}\n").unwrap();
        let m = compute(dir.path()).unwrap();
        assert_eq!(m.loc, 4);
        assert!(m.size_bytes >= "one\ntwo\nthree\n".len() as u64);
    }

    #[test]
    fn skips_binary_files_for_loc_but_counts_size() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("blob.bin"), [0u8, 1, 2, 3, 0, 5]).unwrap();
        fs::write(dir.path().join("a.txt"), "hi\n").unwrap();
        let m = compute(dir.path()).unwrap();
        assert_eq!(m.loc, 1);
        assert!(m.size_bytes >= 6 + 3);
    }

    #[test]
    fn respects_gitignore() {
        // `ignore::WalkBuilder` only honors `.gitignore` inside a git
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        fs::write(dir.path().join(".gitignore"), "ignored.txt\n").unwrap();
        fs::write(dir.path().join("ignored.txt"), "skip\nme\n").unwrap();
        fs::write(dir.path().join("kept.txt"), "ok\n").unwrap();
        let m = compute(dir.path()).unwrap();
        // `.gitignore` itself (1 line) + `kept.txt` (1 line). The
        assert_eq!(m.loc, 2);
    }
}
