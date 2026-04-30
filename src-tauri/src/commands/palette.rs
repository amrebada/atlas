//! query surface; `recents.push` / `recents.list` moved to a dedicated

#![allow(dead_code)]

use tauri::State;

use crate::storage::planner_io;
use crate::storage::types::{MilestoneStatus, PaletteItem, ProjectFilter};
use crate::storage::{AppContext, Db};

/// Default palette cap. Matches the prototype's "6 + recents" layout
const DEFAULT_LIMIT: u32 = 20;

/// `palette.query` - merged FTS + recents + action catalog. Empty
#[tauri::command]
pub async fn palette_query(
    db: State<'_, Db>,
    ctx: State<'_, AppContext>,
    query: String,
    limit: Option<u32>,
) -> Result<Vec<PaletteItem>, String> {
    let lim = limit.unwrap_or(DEFAULT_LIMIT).max(1) as usize;

    // Existing FTS-driven items (projects, notes, actions, recents).
    let mut out = db
        .palette_source(&query, lim as u32)
        .await
        .map_err(|e: anyhow::Error| e.to_string())?;

    // Planner extras — milestones + routines. Substring match against
    // the title; scored by match offset (lower = better, matching the
    // bm25 convention used elsewhere). Skip on empty query so the
    // recent-projects screen stays unchanged.
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Ok(out);
    }

    let projects = db
        .list_projects(ProjectFilter::default())
        .await
        .map_err(|e| e.to_string())?;

    for p in &projects {
        let ms = db.milestones_list(&p.id).await.unwrap_or_default();
        for m in ms {
            if matches!(m.status, MilestoneStatus::Cancelled) {
                continue;
            }
            if let Some(score) = match_score(&m.title, &q) {
                out.push(PaletteItem::Milestone {
                    project_id: p.id.clone(),
                    project_name: p.name.clone(),
                    milestone_id: m.id,
                    title: m.title,
                    deadline: m.deadline,
                    priority: m.priority,
                    status: m.status,
                    score,
                });
            }
        }
    }

    let routines = planner_io::load_routines(&ctx.app_data_dir).unwrap_or_default();
    let project_lookup: std::collections::HashMap<String, String> =
        projects.iter().map(|p| (p.id.clone(), p.name.clone())).collect();
    for r in routines {
        if let Some(score) = match_score(&r.title, &q) {
            let project_name = r
                .project_id
                .as_deref()
                .and_then(|pid| project_lookup.get(pid).cloned());
            out.push(PaletteItem::Routine {
                routine_id: r.id,
                project_id: r.project_id,
                project_name,
                title: r.title,
                rrule: r.rrule,
                priority: r.priority,
                score,
            });
        }
    }

    // Stable secondary sort: keep the FTS ranking for entries scored
    // there, then surface the lowest-scoring planner extras after them.
    // We achieve this by leaving FTS items in place and appending
    // planner extras at the tail; truncation respects the limit.
    out.truncate(lim);
    Ok(out)
}

/// Score `q`'s match against `haystack`. `None` if no match. Lower
/// scores rank earlier (mirrors bm25 convention used by FTS).
fn match_score(haystack: &str, q: &str) -> Option<f32> {
    let lc = haystack.to_lowercase();
    let idx = lc.find(q)?;
    // 0.0 for prefix match, growing with offset; tie-break by length.
    Some(idx as f32 + (haystack.len() as f32 / 1000.0))
}
