//! Restore the user's shell PATH when Atlas is launched from the GUI.
//!
//! Apps started from Finder / Dock / Spotlight inherit only the very
//! short PATH set by `launchd` (`/usr/bin:/bin:/usr/sbin:/sbin`). Tools
//! users install via Homebrew, volta, nvm, asdf, pyenv, pipx, etc. live
//! outside that PATH, so every script / PTY the app spawns fails with
//! `command not found`. We fix that at startup by shelling out to the
//! user's login shell with `-ilc env`, scraping its PATH, and setting
//! it on this process so every child inherits it.
//!
//! After the shell-based attempt (which may fail or time out under
//! cold-cache GUI launches) we *always* append a list of well-known
//! user directories that exist on disk, as defense-in-depth so tools
//! like `claude` installed in `~/.local/bin` still resolve even when
//! the login shell discovery silently fails.
//!
//! No-op on Linux and Windows, where GUI launchers already inherit a
//! reasonable PATH.

/// Try to inherit the login shell's PATH. Best-effort: a failure is
/// logged and swallowed, the app keeps running with whatever PATH we
/// managed to assemble.
pub fn bootstrap() {
    #[cfg(target_os = "macos")]
    macos::bootstrap();
}

#[cfg(target_os = "macos")]
mod macos {
    use std::path::PathBuf;
    use std::process::Command;
    use std::time::Duration;

    pub fn bootstrap() {
        inherit_from_login_shell();
        augment_with_well_known_dirs();
    }

    fn inherit_from_login_shell() {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());

        // `-i -l -c env` runs an interactive login shell that sources
        // `.zprofile`, `.zshrc`, `~/.profile`, etc. then prints the
        // resulting environment. 5 seconds covers cold-cache launches
        // where the first shell after boot can take a few seconds.
        let child = Command::new(&shell)
            .args(["-ilc", "/usr/bin/env"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn();

        let mut child = match child {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(error = %e, shell = %shell, "path bootstrap: shell spawn failed");
                return;
            }
        };

        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) => {
                    if std::time::Instant::now() > deadline {
                        let _ = child.kill();
                        tracing::warn!("path bootstrap: shell timed out, killed");
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(e) => {
                    tracing::warn!(error = %e, "path bootstrap: wait failed");
                    return;
                }
            }
        }

        let out = match child.wait_with_output() {
            Ok(o) => o,
            Err(e) => {
                tracing::warn!(error = %e, "path bootstrap: collect stdout failed");
                return;
            }
        };
        if !out.status.success() {
            tracing::warn!(status = ?out.status, "path bootstrap: shell exited non-zero");
            return;
        }

        let env_text = String::from_utf8_lossy(&out.stdout);
        for line in env_text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            if key == "PATH" && !value.is_empty() {
                // SAFETY: called from the main thread at startup before any
                // other thread is spawned, so no concurrent reads are possible.
                unsafe {
                    std::env::set_var("PATH", value);
                }
                tracing::info!(path = %value, "path bootstrap: PATH inherited from login shell");
                return;
            }
        }

        tracing::warn!("path bootstrap: login shell did not print PATH");
    }

    /// Append common per-user tool directories that exist on disk to PATH.
    /// Idempotent — entries already present are skipped. Runs regardless of
    /// whether the login-shell discovery succeeded, so tools installed in
    /// the usual locations are always findable.
    fn augment_with_well_known_dirs() {
        let home = match std::env::var_os("HOME").map(PathBuf::from) {
            Some(h) => h,
            None => return,
        };

        // Static system dirs first; then $HOME-rooted entries.
        let mut candidates: Vec<PathBuf> = vec![
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/opt/homebrew/sbin"),
            PathBuf::from("/usr/local/bin"),
        ];
        for rel in [
            ".local/bin",
            ".cargo/bin",
            ".bun/bin",
            ".deno/bin",
            "Library/pnpm",
            ".yarn/bin",
            "go/bin",
            ".pub-cache/bin",
            ".lmstudio/bin",
            ".gem/bin",
            ".shorebird/bin",
        ] {
            candidates.push(home.join(rel));
        }

        let current = std::env::var_os("PATH").unwrap_or_default();
        let mut already: Vec<PathBuf> = std::env::split_paths(&current).collect();
        let mut appended: Vec<PathBuf> = Vec::new();
        for dir in candidates {
            if !dir.is_dir() {
                continue;
            }
            if already.iter().any(|p| p == &dir) {
                continue;
            }
            already.push(dir.clone());
            appended.push(dir);
        }

        if appended.is_empty() {
            return;
        }

        let new_path = match std::env::join_paths(already.iter()) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(error = %e, "path bootstrap: join_paths failed");
                return;
            }
        };
        // SAFETY: called from the main thread at startup before any other
        // thread is spawned, so no concurrent reads are possible.
        unsafe {
            std::env::set_var("PATH", &new_path);
        }
        tracing::info!(
            appended = ?appended,
            "path bootstrap: appended well-known dirs to PATH",
        );
    }
}
