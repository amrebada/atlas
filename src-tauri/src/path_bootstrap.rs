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
//! No-op on Linux and Windows, where GUI launchers already inherit a
//! reasonable PATH.

/// Try to inherit the login shell's PATH. Best-effort: a failure is
/// logged and swallowed, the app keeps running with the minimal PATH.
pub fn bootstrap() {
    #[cfg(target_os = "macos")]
    macos::bootstrap();
}

#[cfg(target_os = "macos")]
mod macos {
    use std::process::Command;
    use std::time::Duration;

    pub fn bootstrap() {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());

        // `-i -l -c env` runs an interactive login shell that sources
        // `.zprofile`, `.zshrc`, `~/.profile`, etc. then prints the
        // resulting environment. 2 seconds is plenty for even a noisy
        // rc file; kill the child if it hangs.
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

        let deadline = std::time::Instant::now() + Duration::from_secs(2);
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
}
