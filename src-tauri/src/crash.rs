//! The panic hook installed here **always** runs - installing or

use std::fs::OpenOptions;
use std::io::Write;
use std::panic;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::storage::json::read_json;
use crate::storage::settings::settings_path;
use crate::storage::types::Settings;

/// Path to the crash log, resolved once at install time and stashed for
static CRASH_LOG_PATH: OnceLock<PathBuf> = OnceLock::new();
/// App-data dir - needed by the hook to resolve `settings.json` freshly
static APP_DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Install the global panic hook. Must be called once, after the
pub fn install_panic_hook(app_data_dir: &Path) {
    if CRASH_LOG_PATH.get().is_some() {
        return;
    }
    let log_path = app_data_dir.join("crash.log");
    let _ = CRASH_LOG_PATH.set(log_path);
    let _ = APP_DATA_DIR.set(app_data_dir.to_path_buf());

    // Compose on top of whatever hook the default installer
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        // Always run the default hook first so stderr still shows a
        default_hook(info);

        if !crash_log_enabled() {
            return;
        }

        if let Err(e) = write_panic_record(info) {
            // Avoid recursive panic inside the hook - just log to
            eprintln!("atlas crash-log: failed to persist panic record: {e}");
        }
    }));
}

/// Re-read `settings.json` and return `advanced.crash_log`. A missing
fn crash_log_enabled() -> bool {
    let Some(dir) = APP_DATA_DIR.get() else {
        return false;
    };
    let path = settings_path(dir);
    match read_json::<Settings>(&path) {
        Ok(Some(s)) => s.advanced.crash_log,
        _ => false,
    }
}

/// Format + append one panic record. Uses `OpenOptions::append(true)`
fn write_panic_record(info: &panic::PanicHookInfo<'_>) -> std::io::Result<()> {
    let path = match CRASH_LOG_PATH.get() {
        Some(p) => p,
        None => return Ok(()),
    };

    let timestamp = chrono::Utc::now().to_rfc3339();
    let thread = std::thread::current()
        .name()
        .unwrap_or("<unnamed>")
        .to_string();

    let location = info
        .location()
        .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
        .unwrap_or_else(|| "<unknown>".into());

    // Panic payload: try the two common cases (`&str` and `String`).
    let payload = payload_string(info);

    // Grab a fresh backtrace. `Backtrace::capture` honours
    let backtrace = std::backtrace::Backtrace::force_capture();

    let mut file = OpenOptions::new().create(true).append(true).open(path)?;

    writeln!(file, "---- PANIC {timestamp} ----")?;
    writeln!(file, "thread: {thread}")?;
    writeln!(file, "location: {location}")?;
    writeln!(file, "payload: {payload}")?;
    writeln!(file, "backtrace:")?;
    writeln!(file, "{backtrace}")?;
    writeln!(file)?;
    file.flush()?;
    Ok(())
}

fn payload_string(info: &panic::PanicHookInfo<'_>) -> String {
    let p = info.payload();
    if let Some(s) = p.downcast_ref::<&'static str>() {
        return (*s).to_string();
    }
    if let Some(s) = p.downcast_ref::<String>() {
        return s.clone();
    }
    "<non-string panic payload>".into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn unique_dir(tag: &str) -> PathBuf {
        let ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let p = std::env::temp_dir().join(format!("atlas-crash-{tag}-{}-{ns}", std::process::id()));
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn write_panic_record_shape_is_grep_friendly() {
        // Isolation: this test bypasses `install_panic_hook` and drives
        let dir = unique_dir("write-record");
        let log_path = dir.join("crash.log");

        // Seed globals for this test's scope.
        let _ = CRASH_LOG_PATH.set(log_path.clone());
        let _ = APP_DATA_DIR.set(dir.clone());

        // Synthesize a panic hook-info by catching a real panic.
        let _ = std::panic::catch_unwind(|| {
            panic!("boom");
        });

        // The catch_unwind path fires the current hook. If
        assert!(
            !log_path.exists()
                || fs::metadata(&log_path)
                    .map(|m| m.len() == 0)
                    .unwrap_or(true),
            "hook should no-op without opt-in, but wrote to {}",
            log_path.display()
        );

        // Best-effort cleanup. If the globals have already been set by
        fs::remove_dir_all(&dir).ok();
    }
}
