//! Atlas Pilot IPC commands.
//!
//! Thin wrappers over `PilotManager` (the orchestrator) and `pilot_io` (the
//! on-disk state). The Pilot window calls these; it refetches `pilot_get` /
//! `pilot_history` whenever it sees a `pilot:changed` event.

#![allow(dead_code)]

use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use ts_rs::TS;

use crate::pilot::PilotManager;
use crate::storage::types::{Epic, HistoryEntry, PilotProject, PilotStatus};
use crate::storage::{pilot_io, AppContext};

/// Tauri window label for the Pilot window.
pub const PILOT_WINDOW_LABEL: &str = "pilot";

/// The atlas skill files, embedded at compile time so installation works
/// identically in `tauri dev` and bundled builds (no resource-path lookup).
const SKILL_MD: &str = include_str!("../../resources/skills/atlas/SKILL.md");
const SKILL_PLAN_MD: &str = include_str!("../../resources/skills/atlas/plan.md");
const SKILL_EPIC_MD: &str = include_str!("../../resources/skills/atlas/epic.md");

// =====================================================================
// DTOs.
// =====================================================================

/// One row in the Pilot window's project list.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../src/types/rust.ts")]
pub struct PilotSummary {
    pub path: String,
    pub name: String,
    pub status: PilotStatus,
}

/// One conversational message extracted from a session transcript.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../src/types/rust.ts")]
pub struct ChatMessage {
    /// `"user"` or `"assistant"`.
    pub role: String,
    pub text: String,
}

/// Everything the Pilot window renders for one project.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../src/types/rust.ts")]
pub struct PilotDetail {
    pub path: String,
    pub project: PilotProject,
    pub epics: Vec<Epic>,
    /// Whether a `claude` session is live for this project right now.
    pub running: bool,
    /// Whether that run is currently paused.
    pub paused: bool,
    /// PTY pane id of the live session — used to embed the terminal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<String>,
}

// =====================================================================
// Commands.
// =====================================================================

/// Create a new pilot project: make the folder, `git init`, write the
/// `.atlas/pilot/` skeleton, register it, and start the planning session.
/// Returns the absolute project path.
#[tauri::command]
pub async fn pilot_create(
    ctx: State<'_, AppContext>,
    pilot: State<'_, PilotManager>,
    parent: String,
    name: String,
) -> Result<String, String> {
    let parent = PathBuf::from(&parent);
    if !parent.is_dir() {
        return Err(format!("parent directory not found: {}", parent.display()));
    }
    let path = parent.join(slugify(&name));
    if path.exists() {
        return Err(format!("path already exists: {}", path.display()));
    }
    std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;

    let git_ok = std::process::Command::new("git")
        .arg("init")
        .current_dir(&path)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !git_ok {
        return Err("git init failed".into());
    }

    let project = PilotProject {
        name: name.clone(),
        status: PilotStatus::Draft,
        gate: None,
        auto_advance: true,
        planning_session_id: None,
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    pilot_io::init_pilot(&path, &project).map_err(|e| e.to_string())?;
    pilot_io::register(&ctx.app_data_dir, &path).map_err(|e| e.to_string())?;
    pilot.start_planning(&path).map_err(|e| e.to_string())?;

    Ok(path.to_string_lossy().into_owned())
}

/// List every known pilot project.
#[tauri::command]
pub async fn pilot_list(ctx: State<'_, AppContext>) -> Result<Vec<PilotSummary>, String> {
    let mut out = Vec::new();
    for path in pilot_io::load_registry(&ctx.app_data_dir) {
        if let Ok(Some(project)) = pilot_io::load_project(&path) {
            out.push(PilotSummary {
                path: path.to_string_lossy().into_owned(),
                name: project.name,
                status: project.status,
            });
        }
    }
    Ok(out)
}

/// Full detail for one pilot project.
#[tauri::command]
pub async fn pilot_get(
    pilot: State<'_, PilotManager>,
    path: String,
) -> Result<PilotDetail, String> {
    let p = PathBuf::from(&path);
    let project = pilot_io::load_project(&p)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "not a pilot project".to_string())?;
    let epics = pilot_io::load_epics(&p).map_err(|e| e.to_string())?;
    Ok(PilotDetail {
        path,
        project,
        epics,
        running: pilot.is_running(&p),
        paused: pilot.is_paused(&p),
        pane_id: pilot.pane_id(&p),
    })
}

/// One epic's `history.jsonl`, parsed.
#[tauri::command]
pub async fn pilot_history(path: String, number: i64) -> Result<Vec<HistoryEntry>, String> {
    pilot_io::load_history(&PathBuf::from(&path), number).map_err(|e| e.to_string())
}

/// The project's current session transcript as a conversational thread.
#[tauri::command]
pub async fn pilot_transcript(path: String) -> Result<Vec<ChatMessage>, String> {
    use crate::pilot::transcript::{TranscriptEvent, TranscriptReader};
    let project = PathBuf::from(&path);
    let Some(file) = crate::providers::claude::find_session_for_project(
        &project,
        std::time::SystemTime::UNIX_EPOCH,
    ) else {
        return Ok(Vec::new());
    };
    let mut reader = TranscriptReader::new(file);
    let events = reader.poll().map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for ev in events {
        match ev {
            TranscriptEvent::AssistantTurn { text, .. } => {
                out.push(ChatMessage {
                    role: "assistant".into(),
                    text,
                });
            }
            TranscriptEvent::UserMessage(text) => {
                out.push(ChatMessage {
                    role: "user".into(),
                    text,
                });
            }
            TranscriptEvent::Todos(_) => {}
        }
    }
    Ok(out)
}

/// Read a planning gate artifact (`kind` = `"requirements"` | `"prd"`).
#[tauri::command]
pub async fn pilot_artifact_read(path: String, kind: String) -> Result<String, String> {
    let file = artifact_path(&path, &kind)?;
    match std::fs::read_to_string(&file) {
        Ok(s) => Ok(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e.to_string()),
    }
}

/// Overwrite a planning gate artifact (used when the user edits it before
/// approving the gate).
#[tauri::command]
pub async fn pilot_artifact_write(
    path: String,
    kind: String,
    content: String,
) -> Result<(), String> {
    let file = artifact_path(&path, &kind)?;
    crate::storage::json::write_atomic(&file, content.as_bytes()).map_err(|e| e.to_string())
}

/// Approve the current planning gate (types `continue` into the session).
#[tauri::command]
pub async fn pilot_approve_gate(
    pilot: State<'_, PilotManager>,
    path: String,
) -> Result<(), String> {
    pilot
        .approve_gate(&PathBuf::from(&path))
        .map_err(|e| e.to_string())
}

/// Send a message (question answer or mid-epic modification) to the run.
#[tauri::command]
pub async fn pilot_send_message(
    pilot: State<'_, PilotManager>,
    path: String,
    text: String,
) -> Result<(), String> {
    pilot
        .send_message(&PathBuf::from(&path), &text)
        .map_err(|e| e.to_string())
}

/// Pause the run — withhold the next `continue`.
#[tauri::command]
pub async fn pilot_pause(pilot: State<'_, PilotManager>, path: String) -> Result<(), String> {
    pilot.pause(&PathBuf::from(&path)).map_err(|e| e.to_string())
}

/// Resume a paused run.
#[tauri::command]
pub async fn pilot_resume(pilot: State<'_, PilotManager>, path: String) -> Result<(), String> {
    pilot
        .resume_paused(&PathBuf::from(&path))
        .map_err(|e| e.to_string())
}

/// Hard interrupt (ESC) the run.
#[tauri::command]
pub async fn pilot_interrupt(pilot: State<'_, PilotManager>, path: String) -> Result<(), String> {
    pilot
        .interrupt(&PathBuf::from(&path))
        .map_err(|e| e.to_string())
}

/// Start a fresh planning session for a draft project (e.g. after the app
/// restarted and the previous session was lost).
#[tauri::command]
pub async fn pilot_start_planning(
    pilot: State<'_, PilotManager>,
    path: String,
) -> Result<(), String> {
    pilot
        .start_planning(&PathBuf::from(&path))
        .map_err(|e| e.to_string())
}

/// Manually (re)start a specific epic.
#[tauri::command]
pub async fn pilot_start_epic(
    pilot: State<'_, PilotManager>,
    path: String,
    number: i64,
) -> Result<(), String> {
    pilot
        .start_epic(&PathBuf::from(&path), number)
        .map_err(|e| e.to_string())
}

/// Resume an interrupted run after a crash (`claude --resume`).
#[tauri::command]
pub async fn pilot_resume_run(
    pilot: State<'_, PilotManager>,
    path: String,
) -> Result<(), String> {
    pilot.resume(&PathBuf::from(&path)).map_err(|e| e.to_string())
}

/// Open (or focus) the dedicated Atlas Pilot window.
#[tauri::command]
pub async fn pilot_open_window(app: AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window(PILOT_WINDOW_LABEL) {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
        return Ok(());
    }
    #[allow(unused_mut)]
    let mut builder = tauri::WebviewWindowBuilder::new(
        &app,
        PILOT_WINDOW_LABEL,
        tauri::WebviewUrl::App("index.html".into()),
    )
    .title("Atlas Pilot")
    .inner_size(1180.0, 800.0)
    .min_inner_size(900.0, 600.0)
    .resizable(true);
    // Match the main window's chrome: overlaid traffic lights, no title text.
    #[cfg(target_os = "macos")]
    {
        builder = builder
            .title_bar_style(tauri::TitleBarStyle::Overlay)
            .hidden_title(true);
    }
    builder.build().map_err(|e| e.to_string())?;
    Ok(())
}

/// Install the atlas skill into `~/.claude/skills/atlas/`. Returns the
/// install directory. Overwrites any previous copy.
#[tauri::command]
pub async fn pilot_install_skill() -> Result<String, String> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| "could not resolve home directory".to_string())?;
    let dir = PathBuf::from(home)
        .join(".claude")
        .join("skills")
        .join("atlas");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    for (name, body) in [
        ("SKILL.md", SKILL_MD),
        ("plan.md", SKILL_PLAN_MD),
        ("epic.md", SKILL_EPIC_MD),
    ] {
        std::fs::write(dir.join(name), body).map_err(|e| e.to_string())?;
    }
    Ok(dir.to_string_lossy().into_owned())
}

// =====================================================================
// Helpers.
// =====================================================================

fn artifact_path(project: &str, kind: &str) -> Result<PathBuf, String> {
    let p = PathBuf::from(project);
    match kind {
        "requirements" => Ok(pilot_io::requirements_file(&p)),
        "prd" => Ok(pilot_io::prd_file(&p)),
        other => Err(format!("unknown artifact kind: {other}")),
    }
}

/// Turn a display name into a filesystem-safe folder name.
fn slugify(name: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "pilot-project".to_string()
    } else {
        trimmed
    }
}

#[cfg(test)]
mod tests {
    use super::slugify;

    #[test]
    fn slugify_makes_safe_folder_names() {
        assert_eq!(slugify("My Cool App"), "my-cool-app");
        assert_eq!(slugify("  Trim/Me!! "), "trim-me");
        assert_eq!(slugify("***"), "pilot-project");
        assert_eq!(slugify("a---b"), "a-b");
    }
}
