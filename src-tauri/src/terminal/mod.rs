//! `TerminalManager` owns a map of `PaneId -> PtyPane`. Each pane is a

pub mod pane;

use std::collections::HashMap;
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use uuid::Uuid;

use crate::events;
use crate::storage::types::{PaneId, PaneKind, PaneStatus};

pub use pane::{OpenRequest, PaneDto, PtyPane};

/// Abstraction over the Tauri event bus so unit tests can observe chunks
pub trait TerminalEmitter: Send + Sync + 'static {
    fn emit_data(&self, pane_id: &str, chunk: &[u8]);
    fn emit_exit(&self, pane_id: &str, code: Option<i32>);
}

/// Default emitter; pumps chunks to the frontend via `tauri::Emitter`.
pub struct AppHandleEmitter {
    pub app: tauri::AppHandle,
}

impl TerminalEmitter for AppHandleEmitter {
    fn emit_data(&self, pane_id: &str, chunk: &[u8]) {
        if let Err(e) = events::emit_terminal_data(&self.app, pane_id, chunk) {
            tracing::warn!(error = %e, "emit terminal:data failed");
        }
    }
    fn emit_exit(&self, pane_id: &str, code: Option<i32>) {
        if let Err(e) = events::emit_terminal_exit(&self.app, pane_id, code) {
            tracing::warn!(error = %e, "emit terminal:exit failed");
        }
    }
}

/// How long a pane must be silent before the ticker flips `Active` → `Idle`.
const IDLE_AFTER: Duration = Duration::from_millis(2_000);
/// How often the ticker checks per-pane silence.
const TICK_EVERY: Duration = Duration::from_millis(500);
/// Reader loop chunk size. Matches the brief's 4 KB budget.
const READ_CHUNK: usize = 4 * 1024;

type PaneMap = HashMap<PaneId, Arc<Mutex<PtyPane>>>;

/// Tauri-managed state; commands borrow it via `State<'_, TerminalManager>`.
pub struct TerminalManager {
    inner: Arc<Mutex<PaneMap>>,
    emitter: Arc<dyn TerminalEmitter>,
}

impl TerminalManager {
    /// Construct a manager wired to the Tauri event bus. Used in production
    pub fn new(app: tauri::AppHandle) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            emitter: Arc::new(AppHandleEmitter { app }),
        }
    }

    /// Construct a manager with a custom emitter - used by unit tests to
    #[allow(dead_code)]
    pub fn with_emitter(emitter: Arc<dyn TerminalEmitter>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            emitter,
        }
    }

    /// Open a PTY pane and spawn its child process.
    #[tracing::instrument(
        level = "info",
        skip_all,
        fields(
            cwd = %req.cwd.display(),
            kind = ?req.kind,
            command = req.command.as_deref().unwrap_or("<default-shell>"),
        ),
    )]
    pub fn open(&self, req: OpenRequest) -> anyhow::Result<PaneId> {
        let start = std::time::Instant::now();
        let pane_id = Uuid::new_v4().to_string();

        // Canonicalize cwd - a non-existent path here would explode inside
        let cwd = req.cwd.canonicalize().unwrap_or_else(|_| req.cwd.clone());

        let (program, args) = Self::resolve_program(&req);

        let size = PtySize {
            rows: req.rows.unwrap_or(24),
            cols: req.cols.unwrap_or(80),
            pixel_width: 0,
            pixel_height: 0,
        };

        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(size)
            .map_err(|e| anyhow::anyhow!("openpty: {e}"))?;

        let mut cmd = CommandBuilder::new(&program);
        for a in &args {
            cmd.arg(a);
        }
        cmd.cwd(&cwd);
        for (k, v) in &req.env {
            cmd.env(k, v);
        }
        // Set TERM so curses-aware tools light up. xterm-256color is a
        cmd.env("TERM", "xterm-256color");

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| anyhow::anyhow!("spawn {program}: {e}"))?;

        // Drop the slave so the child gets EOF on its stdin once the
        drop(pair.slave);

        let reader_handle = pair
            .master
            .try_clone_reader()
            .map_err(|e| anyhow::anyhow!("clone reader: {e}"))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| anyhow::anyhow!("take writer: {e}"))?;

        // Split the child into a killer (held by the pane for `close()`)
        let killer = child.clone_killer();

        let title = req.title.clone().unwrap_or_else(|| {
            if args.is_empty() {
                program.clone()
            } else {
                format!("{program} {}", args.join(" "))
            }
        });

        let initial_status = match req.kind {
            PaneKind::Script => PaneStatus::Running,
            _ => PaneStatus::Active,
        };

        // Placeholder join handles; replaced immediately below. Using
        let placeholder_a = tokio::spawn(async {});
        let placeholder_b = tokio::spawn(async {});

        let pane = Arc::new(Mutex::new(PtyPane {
            id: pane_id.clone(),
            kind: req.kind.clone(),
            title,
            cwd: cwd.clone(),
            branch: req.branch.clone(),
            script_id: req.script_id.clone(),
            session_id: req.session_id.clone(),
            status: initial_status,
            last_output_at: Instant::now(),
            master: pair.master,
            writer,
            killer,
            reader_task: placeholder_a,
            ticker_task: placeholder_b,
        }));

        let reader = Self::spawn_reader(
            self.emitter.clone(),
            pane.clone(),
            pane_id.clone(),
            reader_handle,
            child,
        );
        let ticker = Self::spawn_ticker(pane.clone());

        // Swap the real handles in; abort the placeholders so they don't
        {
            let mut p = pane
                .lock()
                .map_err(|e| anyhow::anyhow!("pane mutex poisoned during open: {e}"))?;
            let old_r = std::mem::replace(&mut p.reader_task, reader);
            let old_t = std::mem::replace(&mut p.ticker_task, ticker);
            old_r.abort();
            old_t.abort();
        }

        let mut map = self
            .inner
            .lock()
            .map_err(|e| anyhow::anyhow!("pane map mutex poisoned: {e}"))?;
        map.insert(pane_id.clone(), pane);
        tracing::info!(
            pane_id = %pane_id,
            elapsed_ms = start.elapsed().as_millis() as u64,
            "terminal pane opened",
        );
        Ok(pane_id)
    }

    /// Send `data` to the pane's stdin. Returns `Err` when the pane id is
    pub fn write(&self, id: &PaneId, data: &[u8]) -> anyhow::Result<()> {
        let pane = self.get(id)?;
        let mut p = pane
            .lock()
            .map_err(|e| anyhow::anyhow!("pane mutex poisoned during write: {e}"))?;
        p.writer
            .write_all(data)
            .map_err(|e| anyhow::anyhow!("write pane {id}: {e}"))?;
        p.writer
            .flush()
            .map_err(|e| anyhow::anyhow!("flush pane {id}: {e}"))?;
        Ok(())
    }

    /// Inform the kernel of a new terminal size. Propagates to the child
    pub fn resize(&self, id: &PaneId, cols: u16, rows: u16) -> anyhow::Result<()> {
        let pane = self.get(id)?;
        let p = pane
            .lock()
            .map_err(|e| anyhow::anyhow!("pane mutex poisoned during resize: {e}"))?;
        p.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| anyhow::anyhow!("resize pane {id}: {e}"))?;
        Ok(())
    }

    /// Kill the child (SIGHUP on Unix) and remove the pane from the map.
    pub fn close(&self, id: &PaneId) -> anyhow::Result<()> {
        let pane = {
            let mut map = self
                .inner
                .lock()
                .map_err(|e| anyhow::anyhow!("pane map mutex poisoned during close: {e}"))?;
            map.remove(id)
                .ok_or_else(|| anyhow::anyhow!("pane {id} not found"))?
        };
        let mut p = pane
            .lock()
            .map_err(|e| anyhow::anyhow!("pane mutex poisoned during close: {e}"))?;
        // Best-effort kill. On Unix portable-pty sends SIGHUP; if the
        if let Err(e) = p.killer.kill() {
            tracing::debug!(error = %e, pane = %id, "pane kill (likely already exited)");
        }
        p.ticker_task.abort();
        p.reader_task.abort();
        Ok(())
    }

    /// Safe snapshot of every open pane. Holds the map lock just long
    pub fn list(&self) -> Vec<PaneDto> {
        let map = match self.inner.lock() {
            Ok(g) => g,
            Err(poisoned) => {
                tracing::warn!("pane map mutex poisoned in list; recovering");
                poisoned.into_inner()
            }
        };
        let mut out = Vec::with_capacity(map.len());
        for pane in map.values() {
            match pane.lock() {
                Ok(p) => out.push(p.to_dto()),
                Err(poisoned) => out.push(poisoned.into_inner().to_dto()),
            }
        }
        out
    }

    // ------- internals -------

    fn get(&self, id: &PaneId) -> anyhow::Result<Arc<Mutex<PtyPane>>> {
        let map = self
            .inner
            .lock()
            .map_err(|e| anyhow::anyhow!("pane map mutex poisoned during get: {e}"))?;
        map.get(id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("pane {id} not found"))
    }

    /// Resolve `(program, args)` from an `OpenRequest`. When `command` is
    fn resolve_program(req: &OpenRequest) -> (String, Vec<String>) {
        let program = req.command.clone().unwrap_or_else(Self::default_shell);
        (program, req.args.clone())
    }

    /// Platform-default shell. Callers can override via `OpenRequest.command`.
    pub fn default_shell() -> String {
        if cfg!(target_os = "windows") {
            "cmd.exe".into()
        } else if cfg!(target_os = "macos") {
            "/bin/zsh".into()
        } else {
            "/bin/sh".into()
        }
    }

    /// Spawn the blocking reader loop. Takes ownership of the `Child` so it
    fn spawn_reader(
        emitter: Arc<dyn TerminalEmitter>,
        pane: Arc<Mutex<PtyPane>>,
        pane_id: PaneId,
        mut reader: Box<dyn Read + Send>,
        mut child: Box<dyn portable_pty::Child + Send + Sync>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::task::spawn_blocking(move || {
            let mut buf = vec![0u8; READ_CHUNK];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        emitter.emit_data(&pane_id, &buf[..n]);
                        // Touch the pane state - brief critical section
                        if let Ok(mut p) = pane.lock() {
                            p.last_output_at = Instant::now();
                            if !matches!(p.status, PaneStatus::Error) {
                                p.status = match p.kind {
                                    PaneKind::Script => PaneStatus::Running,
                                    _ => PaneStatus::Active,
                                };
                            }
                        }
                    }
                    Err(e) => {
                        tracing::debug!(error = %e, pane = %pane_id, "pane read err (EOF?)");
                        break;
                    }
                }
            }

            // Wait on the child; reaps the process too so no zombies.
            let code = match child.wait() {
                Ok(status) => {
                    let raw = status.exit_code() as i32;
                    if status.success() {
                        Some(0)
                    } else {
                        Some(raw)
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, pane = %pane_id, "child.wait failed");
                    None
                }
            };

            // Paint the final status before emitting exit so a UI that
            if let Ok(mut p) = pane.lock() {
                p.status = match code {
                    Some(0) => PaneStatus::Idle,
                    Some(_) => PaneStatus::Error,
                    None => PaneStatus::Error,
                };
            }

            emitter.emit_exit(&pane_id, code);
        })
    }

    /// Spawn the idle ticker. Checks `last_output_at` every `TICK_EVERY`
    fn spawn_ticker(pane: Arc<Mutex<PtyPane>>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(TICK_EVERY).await;
                let Ok(mut p) = pane.lock() else {
                    return;
                };
                if p.reader_task.is_finished() {
                    return;
                }
                if matches!(p.status, PaneStatus::Active)
                    && p.last_output_at.elapsed() >= IDLE_AFTER
                {
                    p.status = PaneStatus::Idle;
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::Mutex as StdMutex;
    use std::time::Duration;

    /// Test emitter that stashes every chunk + exit code it sees so the
    #[derive(Default)]
    struct RecordingEmitter {
        chunks: StdMutex<Vec<u8>>,
        exits: StdMutex<Vec<Option<i32>>>,
    }

    impl TerminalEmitter for RecordingEmitter {
        fn emit_data(&self, _pane_id: &str, chunk: &[u8]) {
            if let Ok(mut b) = self.chunks.lock() {
                b.extend_from_slice(chunk);
            }
        }
        fn emit_exit(&self, _pane_id: &str, code: Option<i32>) {
            if let Ok(mut v) = self.exits.lock() {
                v.push(code);
            }
        }
    }

    fn tempdir(prefix: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        let ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        p.push(format!("{prefix}-{ns}"));
        std::fs::create_dir_all(&p).expect("create tempdir");
        p
    }

    /// Default shell is present + absolute on the host. We don't assert
    #[test]
    fn default_shell_is_absolute_on_unix() {
        let sh = TerminalManager::default_shell();
        if cfg!(unix) {
            assert!(sh.starts_with('/'), "non-absolute unix shell: {sh}");
        }
        assert!(!sh.is_empty());
    }

    /// Round-trip PTY: open `/bin/sh`, write `echo hello\n`, and observe
    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pty_open_write_reads_output() -> anyhow::Result<()> {
        let rec = Arc::new(RecordingEmitter::default());
        let mgr = TerminalManager::with_emitter(rec.clone() as Arc<dyn TerminalEmitter>);

        let cwd = tempdir("atlas_pty_open");
        let pane_id = mgr.open(OpenRequest {
            kind: PaneKind::Shell,
            cwd: cwd.clone(),
            command: Some("/bin/sh".into()),
            args: vec![],
            env: vec![],
            title: None,
            branch: None,
            script_id: None,
            session_id: None,
            cols: Some(80),
            rows: Some(24),
        })?;

        // Send a command; the shell should echo + print "hello".
        mgr.write(&pane_id, b"echo hello\n")?;

        // Poll for up to 5s for the emitter to record the expected bytes.
        let start = Instant::now();
        let mut saw_hello = false;
        while start.elapsed() < Duration::from_secs(5) {
            {
                let chunks = rec.chunks.lock().unwrap();
                if chunks.windows(5).any(|w| w == b"hello") {
                    saw_hello = true;
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        // Tear the pane down before asserting so a failing test doesn't
        let _ = mgr.close(&pane_id);
        let _ = std::fs::remove_dir_all(&cwd);

        assert!(saw_hello, "reader task never emitted 'hello'");
        Ok(())
    }

    /// `list()` returns DTO snapshots without leaking handles. Calling it
    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn list_reflects_lifecycle() -> anyhow::Result<()> {
        let rec = Arc::new(RecordingEmitter::default());
        let mgr = TerminalManager::with_emitter(rec as Arc<dyn TerminalEmitter>);

        let cwd = tempdir("atlas_pty_list");
        let id = mgr.open(OpenRequest {
            kind: PaneKind::Shell,
            cwd: cwd.clone(),
            command: Some("/bin/sh".into()),
            args: vec![],
            env: vec![],
            title: Some("test-pane".into()),
            branch: None,
            script_id: None,
            session_id: None,
            cols: None,
            rows: None,
        })?;

        let list = mgr.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, id);
        assert_eq!(list[0].title, "test-pane");

        mgr.close(&id)?;
        let list = mgr.list();
        assert!(list.is_empty());
        let _ = std::fs::remove_dir_all(&cwd);
        Ok(())
    }
}
