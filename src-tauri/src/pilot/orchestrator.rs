//! The pilot orchestrator — the session state machine.
//!
//! One `PilotManager` is Tauri-managed state. It owns at most one *run* per
//! pilot project: a wrapped `claude` PTY pane plus a polling task that tails
//! that session's transcript and drives it.
//!
//! Run lifecycle:
//!   * spawn a fresh `claude` seeded with the atlas skill invocation,
//!   * discover the new transcript file by matching `cwd`,
//!   * poll it: react to sentinels (auto-`continue`, gates, epic-done),
//!     mirror `TodoWrite` progress into the epic file,
//!   * commit/push on `EPIC_DONE` and auto-advance to the next epic,
//!   * hold an OS wake lock while actively working.

#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use tauri::{AppHandle, Manager};

use crate::events;
use crate::providers::claude;
use crate::storage::pilot_io;
use crate::storage::types::{EpicStatus, PaneId, PaneKind, PilotGate, PilotStatus};
use crate::terminal::{OpenRequest, TerminalManager};

use super::transcript::{Sentinel, TodoItem, TodoStatus, TranscriptEvent, TranscriptReader};

/// How often a run polls its transcript.
const TICK: Duration = Duration::from_millis(750);
/// Silence after which a run with no actionable sentinel is treated as
/// blocked on the user (decision: idle-without-sentinel → NEEDS_INPUT).
const IDLE_AFTER: Duration = Duration::from_secs(45);

/// What a run is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    /// The gated planning session (grill-me → PRD → epics).
    Planning,
    /// Implementing one epic, by number.
    Epic(i64),
}

/// A live run: its pane, pause flag, and polling task.
struct RunHandle {
    mode: RunMode,
    pane_id: PaneId,
    paused: Arc<AtomicBool>,
    task: tokio::task::JoinHandle<()>,
}

/// Process-wide, reference-counted OS wake lock. Held while any run is
/// actively working; prevents system idle-sleep, never display sleep.
struct WakeLock {
    refs: usize,
    child: Option<std::process::Child>,
}

impl WakeLock {
    fn new() -> Self {
        Self {
            refs: 0,
            child: None,
        }
    }

    fn acquire(&mut self) {
        self.refs += 1;
        if self.refs == 1 && self.child.is_none() {
            self.child = spawn_wake_lock();
        }
    }

    fn release(&mut self) {
        self.refs = self.refs.saturating_sub(1);
        if self.refs == 0 {
            if let Some(mut c) = self.child.take() {
                let _ = c.kill();
                let _ = c.wait();
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn spawn_wake_lock() -> Option<std::process::Child> {
    // `-i` prevents system idle sleep; the display is free to sleep.
    Command::new("caffeinate")
        .arg("-i")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()
}

#[cfg(not(target_os = "macos"))]
fn spawn_wake_lock() -> Option<std::process::Child> {
    // TODO(pilot): Linux/Windows wake lock via the `keepawake` crate.
    None
}

/// Shared orchestrator state behind an `Arc` so polling tasks can call back
/// into it (to auto-advance, persist progress, reconcile the wake lock).
struct Inner {
    app: AppHandle,
    runs: Mutex<HashMap<String, RunHandle>>,
    wake: Mutex<WakeLock>,
}

/// Tauri-managed pilot orchestrator.
pub struct PilotManager {
    inner: Arc<Inner>,
}

impl PilotManager {
    pub fn new(app: AppHandle) -> Self {
        Self {
            inner: Arc::new(Inner {
                app,
                runs: Mutex::new(HashMap::new()),
                wake: Mutex::new(WakeLock::new()),
            }),
        }
    }

    /// True if a run is currently active for this project.
    pub fn is_running(&self, project: &Path) -> bool {
        self.inner
            .runs
            .lock()
            .unwrap()
            .contains_key(&project_key(project))
    }

    /// PTY pane id of the project's active run, for embedding the terminal.
    pub fn pane_id(&self, project: &Path) -> Option<PaneId> {
        self.inner
            .runs
            .lock()
            .unwrap()
            .get(&project_key(project))
            .map(|r| r.pane_id.clone())
    }

    /// True if the project's run is currently paused.
    pub fn is_paused(&self, project: &Path) -> bool {
        self.inner
            .runs
            .lock()
            .unwrap()
            .get(&project_key(project))
            .map(|r| r.paused.load(Ordering::Relaxed))
            .unwrap_or(false)
    }

    /// Start the gated planning session for a draft project.
    pub fn start_planning(&self, project: &Path) -> anyhow::Result<()> {
        if self.is_running(project) {
            anyhow::bail!("a pilot run is already active for this project");
        }
        let seed = format!(
            "Use the atlas skill in planning mode. The repository is at {}. Begin.",
            project.display()
        );
        self.inner
            .spawn_run(project, RunMode::Planning, vec![seed])
    }

    /// Start (or restart) implementation of a specific epic.
    pub fn start_epic(&self, project: &Path, number: i64) -> anyhow::Result<()> {
        if self.is_running(project) {
            anyhow::bail!("a pilot run is already active for this project");
        }
        self.inner.begin_epic(project, number)
    }

    /// Resume an interrupted run via `claude --resume` (crash recovery).
    pub fn resume(&self, project: &Path) -> anyhow::Result<()> {
        if self.is_running(project) {
            return Ok(());
        }
        let proj = pilot_io::load_project(project)?
            .ok_or_else(|| anyhow::anyhow!("not a pilot project"))?;
        // Find the session id to resume: the active epic, else planning.
        let (mode, session_id) = if proj.status == PilotStatus::Draft {
            (RunMode::Planning, proj.planning_session_id.clone())
        } else {
            let epics = pilot_io::load_epics(project)?;
            match epics.iter().find(|e| {
                matches!(e.status, EpicStatus::Active | EpicStatus::Interrupted)
            }) {
                Some(e) => (RunMode::Epic(e.number), e.session_id.clone()),
                None => anyhow::bail!("nothing to resume"),
            }
        };
        let sid = session_id.ok_or_else(|| anyhow::anyhow!("no session id recorded yet"))?;
        self.inner
            .spawn_run(project, mode, vec!["--resume".into(), sid])
    }

    /// Send a message (a question answer or a mid-epic modification) to the
    /// project's active run.
    pub fn send_message(&self, project: &Path, text: &str) -> anyhow::Result<()> {
        let pane_id = self.inner.pane_for(project)?;
        // Terminal "Enter" is carriage return — `\n` would type the text
        // into Claude's prompt without ever submitting it.
        let mut payload = text.trim_end().to_string();
        payload.push('\r');
        self.inner.write_pane(&pane_id, &payload)
    }

    /// Approve the current planning gate: type `continue` into the planning
    /// session. Approving the epics gate flips the project to `active` and
    /// kicks off the first epic.
    pub fn approve_gate(&self, project: &Path) -> anyhow::Result<()> {
        let mut proj = pilot_io::load_project(project)?
            .ok_or_else(|| anyhow::anyhow!("not a pilot project"))?;
        let pane_id = self.inner.pane_for(project)?;

        if proj.gate == Some(PilotGate::Epics) {
            // Planning is complete. Interactive `claude` does not exit on
            // its own, so flip to active, auto-start the first epic, and
            // explicitly close the planning session.
            proj.status = PilotStatus::Active;
            proj.gate = None;
            pilot_io::save_project(project, &proj)?;
            self.inner.advance(project);
            let tm = self.inner.app.state::<TerminalManager>();
            let _ = tm.close(&pane_id);
        } else {
            // Reqs/PRD gate: tell the skill to proceed to the next stage.
            // The next GATE sentinel re-sets the gate.
            self.inner.write_pane(&pane_id, "continue\r")?;
            proj.gate = None;
            pilot_io::save_project(project, &proj)?;
        }
        self.inner.emit(project);
        Ok(())
    }

    /// Pause the run — withhold the next `continue` (effective at the next
    /// task boundary).
    pub fn pause(&self, project: &Path) -> anyhow::Result<()> {
        self.inner.set_paused(project, true)
    }

    /// Resume a paused run.
    pub fn resume_paused(&self, project: &Path) -> anyhow::Result<()> {
        self.inner.set_paused(project, false)
    }

    /// Hard interrupt: send ESC to the session and pause it.
    pub fn interrupt(&self, project: &Path) -> anyhow::Result<()> {
        let pane_id = self.inner.pane_for(project)?;
        self.inner.write_pane(&pane_id, "\x1b")?;
        self.inner.set_paused(project, true)
    }
}

impl Inner {
    /// Look up the pane id of the project's active run.
    fn pane_for(&self, project: &Path) -> anyhow::Result<PaneId> {
        self.runs
            .lock()
            .unwrap()
            .get(&project_key(project))
            .map(|r| r.pane_id.clone())
            .ok_or_else(|| anyhow::anyhow!("no active pilot run for this project"))
    }

    fn set_paused(&self, project: &Path, paused: bool) -> anyhow::Result<()> {
        let runs = self.runs.lock().unwrap();
        let run = runs
            .get(&project_key(project))
            .ok_or_else(|| anyhow::anyhow!("no active pilot run for this project"))?;
        run.paused.store(paused, Ordering::Relaxed);
        drop(runs);
        self.emit(project);
        Ok(())
    }

    fn write_pane(&self, pane_id: &PaneId, data: &str) -> anyhow::Result<()> {
        let tm = self.app.state::<TerminalManager>();
        tm.write(pane_id, data.as_bytes())
    }

    fn emit(&self, project: &Path) {
        if let Err(e) = events::emit_pilot(&self.app, &project.to_string_lossy()) {
            tracing::warn!(error = %e, "pilot: emit failed");
        }
    }

    fn wake_acquire(&self) {
        self.wake.lock().unwrap().acquire();
    }

    fn wake_release(&self) {
        self.wake.lock().unwrap().release();
    }

    /// Open a `claude` pane and spawn its polling task.
    fn spawn_run(
        self: &Arc<Self>,
        project: &Path,
        mode: RunMode,
        args: Vec<String>,
    ) -> anyhow::Result<()> {
        let pane_id = {
            let tm = self.app.state::<TerminalManager>();
            tm.open(OpenRequest {
                kind: PaneKind::ClaudeSession,
                cwd: project.to_path_buf(),
                command: Some("claude".into()),
                args,
                env: Vec::new(),
                title: Some(run_title(mode)),
                branch: None,
                script_id: None,
                session_id: None,
                cols: Some(120),
                rows: Some(32),
            })?
        };

        let paused = Arc::new(AtomicBool::new(false));
        let task = {
            let inner = Arc::clone(self);
            let project = project.to_path_buf();
            let paused = Arc::clone(&paused);
            let pane_id = pane_id.clone();
            tokio::spawn(async move {
                poll_loop(inner, project, mode, pane_id, paused).await;
            })
        };

        self.runs.lock().unwrap().insert(
            project_key(project),
            RunHandle {
                mode,
                pane_id,
                paused,
                task,
            },
        );
        self.emit(project);
        Ok(())
    }

    /// Mark an epic `active` and start a fresh session for it.
    fn begin_epic(self: &Arc<Self>, project: &Path, number: i64) -> anyhow::Result<()> {
        let epic = pilot_io::load_epic(project, number)?
            .ok_or_else(|| anyhow::anyhow!("epic {number} not found"))?;
        let mut epic = epic;
        epic.status = EpicStatus::Active;
        let title = epic.title.clone();
        pilot_io::save_epic(project, &epic)?;

        let seed = format!(
            "Use the atlas skill in epic mode. The repository is at {}. \
             Start epic {}: {}.",
            project.display(),
            number,
            title
        );
        self.spawn_run(project, RunMode::Epic(number), vec![seed])
    }

    /// React to `EPIC_DONE`: mark the epic done, then advance.
    fn finish_epic(self: &Arc<Self>, project: &Path, number: i64) {
        if let Ok(Some(mut epic)) = pilot_io::load_epic(project, number) {
            for t in &mut epic.tasks {
                t.done = true;
            }
            epic.status = EpicStatus::Done;
            let _ = pilot_io::save_epic(project, &epic);
        }
        self.emit(project);
        self.advance(project);
    }

    /// Start the next pending epic whose dependencies are all complete, or
    /// mark the whole project done when none remain. Also used to kick off
    /// the first epic once planning is approved.
    fn advance(self: &Arc<Self>, project: &Path) {
        let epics = pilot_io::load_epics(project).unwrap_or_default();
        let done: HashSet<i64> = epics
            .iter()
            .filter(|e| e.status == EpicStatus::Done)
            .map(|e| e.number)
            .collect();
        let next = epics
            .iter()
            .find(|e| {
                e.status == EpicStatus::Pending
                    && e.depends_on.iter().all(|d| done.contains(d))
            })
            .map(|e| e.number);

        match next {
            Some(n) => {
                if let Err(e) = self.begin_epic(project, n) {
                    tracing::warn!(error = %e, "pilot: failed to start next epic");
                }
            }
            None => {
                if let Ok(Some(mut proj)) = pilot_io::load_project(project) {
                    proj.status = PilotStatus::Done;
                    let _ = pilot_io::save_project(project, &proj);
                }
                self.emit(project);
            }
        }
    }

    /// Persist the discovered Claude session id onto the run's record.
    fn record_session_id(&self, project: &Path, mode: RunMode, session_id: &str) {
        match mode {
            RunMode::Planning => {
                if let Ok(Some(mut proj)) = pilot_io::load_project(project) {
                    if proj.planning_session_id.as_deref() != Some(session_id) {
                        proj.planning_session_id = Some(session_id.to_string());
                        let _ = pilot_io::save_project(project, &proj);
                    }
                }
            }
            RunMode::Epic(n) => {
                if let Ok(Some(mut epic)) = pilot_io::load_epic(project, n) {
                    if epic.session_id.as_deref() != Some(session_id) {
                        epic.session_id = Some(session_id.to_string());
                        let _ = pilot_io::save_epic(project, &epic);
                    }
                }
            }
        }
    }

    /// Mirror a `TodoWrite` snapshot onto the epic's task `done` flags.
    fn apply_todos(&self, project: &Path, mode: RunMode, todos: &[TodoItem]) {
        let RunMode::Epic(n) = mode else { return };
        let Ok(Some(mut epic)) = pilot_io::load_epic(project, n) else {
            return;
        };
        let mut changed = false;
        for (task, todo) in epic.tasks.iter_mut().zip(todos.iter()) {
            let done = todo.status == TodoStatus::Completed;
            if task.done != done {
                task.done = done;
                changed = true;
            }
        }
        if changed {
            let _ = pilot_io::save_epic(project, &epic);
        }
    }

    /// Count one task-checkpoint cycle (one `continue`) against the epic.
    fn bump_iteration(&self, project: &Path, mode: RunMode) {
        let RunMode::Epic(n) = mode else { return };
        if let Ok(Some(mut epic)) = pilot_io::load_epic(project, n) {
            epic.iterations += 1;
            let _ = pilot_io::save_epic(project, &epic);
        }
    }

    fn set_gate(&self, project: &Path, gate: PilotGate) {
        if let Ok(Some(mut proj)) = pilot_io::load_project(project) {
            proj.gate = Some(gate);
            let _ = pilot_io::save_project(project, &proj);
        }
    }

    /// A run's pane died before reaching a terminal state. Mark an
    /// in-flight epic `interrupted` (planning that already flipped the
    /// project to `active` is a normal completion, not a crash).
    fn on_session_died(&self, project: &Path, mode: RunMode) {
        if let RunMode::Epic(n) = mode {
            if let Ok(Some(mut epic)) = pilot_io::load_epic(project, n) {
                if epic.status == EpicStatus::Active {
                    epic.status = EpicStatus::Interrupted;
                    let _ = pilot_io::save_epic(project, &epic);
                }
            }
        }
        self.emit(project);
    }
}

/// The per-run polling loop. Tails the session transcript and drives it.
async fn poll_loop(
    inner: Arc<Inner>,
    project: PathBuf,
    mode: RunMode,
    pane_id: PaneId,
    paused: Arc<AtomicBool>,
) {
    let spawn_time = SystemTime::now();
    let mut reader: Option<TranscriptReader> = None;
    let mut last_activity = Instant::now();
    let mut last_sentinel: Option<Sentinel> = None;
    let mut pending_continue = false;
    let mut finished = false;
    let mut holding_wake = false;

    loop {
        tokio::time::sleep(TICK).await;

        // Pane gone → the session ended.
        let pane_alive = {
            let tm = inner.app.state::<TerminalManager>();
            tm.list().iter().any(|p| p.id == pane_id)
        };
        if !pane_alive {
            if !finished {
                inner.on_session_died(&project, mode);
            }
            break;
        }

        // Bind to the transcript once Claude has created it.
        if reader.is_none() {
            match claude::find_session_for_project(&project, spawn_time) {
                Some(path) => {
                    if let Some(sid) = path.file_stem().and_then(|s| s.to_str()) {
                        inner.record_session_id(&project, mode, sid);
                    }
                    reader = Some(TranscriptReader::new(path));
                }
                None => continue,
            }
        }
        let events = match reader.as_mut().unwrap().poll() {
            Ok(ev) => ev,
            Err(err) => {
                tracing::warn!(error = %err, "pilot: transcript poll failed");
                Vec::new()
            }
        };
        if !events.is_empty() {
            last_activity = Instant::now();
        }

        for ev in events {
            match ev {
                TranscriptEvent::Todos(todos) => inner.apply_todos(&project, mode, &todos),
                TranscriptEvent::UserMessage(_) => {
                    // A user reply un-parks a NEEDS_INPUT wait.
                    if last_sentinel == Some(Sentinel::NeedsInput) {
                        last_sentinel = None;
                    }
                    inner.emit(&project);
                }
                TranscriptEvent::AssistantTurn { sentinel, .. } => {
                    last_sentinel = sentinel;
                    match sentinel {
                        Some(Sentinel::TaskDone) => pending_continue = true,
                        Some(Sentinel::EpicDone) => {
                            finished = true;
                            if let RunMode::Epic(n) = mode {
                                let title = pilot_io::load_epic(&project, n)
                                    .ok()
                                    .flatten()
                                    .map(|e| e.title)
                                    .unwrap_or_default();
                                let repo = project.clone();
                                let msg = format!("epic {n:02}: {title}");
                                let _ = tokio::task::spawn_blocking(move || {
                                    git_commit_push(&repo, &msg);
                                })
                                .await;
                                inner.finish_epic(&project, n);
                            }
                        }
                        Some(Sentinel::GateReqs) => inner.set_gate(&project, PilotGate::Reqs),
                        Some(Sentinel::GatePrd) => inner.set_gate(&project, PilotGate::Prd),
                        Some(Sentinel::GateEpics) => inner.set_gate(&project, PilotGate::Epics),
                        Some(Sentinel::NeedsInput) | None => {}
                    }
                    inner.emit(&project);
                }
            }
        }

        if finished {
            break;
        }

        // Send a withheld `continue` once unpaused.
        if pending_continue && !paused.load(Ordering::Relaxed) {
            pending_continue = false;
            last_sentinel = None;
            if inner.write_pane(&pane_id, "continue\r").is_ok() {
                inner.bump_iteration(&project, mode);
                last_activity = Instant::now();
                inner.emit(&project);
            }
        }

        // Idle with no actionable sentinel → treat as blocked on the user.
        if last_sentinel.is_none()
            && !pending_continue
            && reader.is_some()
            && last_activity.elapsed() > IDLE_AFTER
        {
            last_sentinel = Some(Sentinel::NeedsInput);
            inner.emit(&project);
        }

        // Reconcile the wake lock: hold it only while actively working.
        let parked = paused.load(Ordering::Relaxed)
            || pending_continue
            || matches!(
                last_sentinel,
                Some(Sentinel::NeedsInput)
                    | Some(Sentinel::GateReqs)
                    | Some(Sentinel::GatePrd)
                    | Some(Sentinel::GateEpics)
            );
        if !parked && !holding_wake {
            inner.wake_acquire();
            holding_wake = true;
        } else if parked && holding_wake {
            inner.wake_release();
            holding_wake = false;
        }
    }

    if holding_wake {
        inner.wake_release();
    }
    // Best-effort: close the pane if it outlived the loop.
    {
        let tm = inner.app.state::<TerminalManager>();
        let _ = tm.close(&pane_id);
    }
    // Drop the run handle, but only if it is still this loop's run — an
    // auto-advance may have already replaced it with the next epic's run.
    {
        let mut runs = inner.runs.lock().unwrap();
        let key = project_key(&project);
        if runs.get(&key).map(|r| r.pane_id == pane_id).unwrap_or(false) {
            runs.remove(&key);
        }
    }
    inner.emit(&project);
}

// ---------- helpers ----------

/// Canonical-path key for the `runs` map.
fn project_key(project: &Path) -> String {
    project
        .canonicalize()
        .unwrap_or_else(|_| project.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

fn run_title(mode: RunMode) -> String {
    match mode {
        RunMode::Planning => "Atlas Pilot — planning".to_string(),
        RunMode::Epic(n) => format!("Atlas Pilot — epic {n:02}"),
    }
}

/// `git add -A` + commit + push (push only when a remote exists). All steps
/// are best-effort: a no-op commit or a missing remote is not an error.
fn git_commit_push(repo: &Path, message: &str) {
    let git = |args: &[&str]| {
        Command::new("git")
            .current_dir(repo)
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
    };

    git(&["add", "-A"]);
    if git(&["commit", "-m", message]).is_none() {
        tracing::info!("pilot: nothing to commit for '{message}'");
    }

    let has_remote = Command::new("git")
        .current_dir(repo)
        .arg("remote")
        .output()
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false);
    if has_remote && git(&["push"]).is_none() {
        tracing::warn!("pilot: push failed for '{message}'");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_title_formats_epic_number() {
        assert_eq!(run_title(RunMode::Planning), "Atlas Pilot — planning");
        assert_eq!(run_title(RunMode::Epic(3)), "Atlas Pilot — epic 03");
        assert_eq!(run_title(RunMode::Epic(12)), "Atlas Pilot — epic 12");
    }

    #[test]
    fn project_key_is_stable() {
        let tmp = std::env::temp_dir();
        assert_eq!(project_key(&tmp), project_key(&tmp));
    }
}
