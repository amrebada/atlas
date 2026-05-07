//! Pluggable AI session providers (Claude Code, Codex CLI, OpenCode CLI, …).
//!
//! Each provider implements [`SessionProvider`] for discovery + resume.
//! [`ProvidersRegistry`] owns the collection and is shared as Tauri-managed
//! state; commands fan out across enabled providers via the registry.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::storage::types::{Session, SessionStatus};

pub mod claude;
pub mod codex;
pub mod opencode;
pub mod shared;

// ---------- Identity ----------

/// String id stored in settings + on every Session DTO.
pub type ProviderId = String;

pub const ID_CLAUDE: &str = "claude";
pub const ID_CODEX: &str = "codex";
pub const ID_OPENCODE: &str = "opencode";

// ---------- DTOs ----------

/// In-memory representation of a single parsed session (any provider).
/// Converts into the public [`Session`] DTO on demand.
#[derive(Debug, Clone)]
pub struct ParsedSession {
    pub provider: ProviderId,
    pub id: String,
    pub title: String,
    pub when: DateTime<Utc>,
    pub turns: u32,
    pub duration: String,
    pub status: SessionStatus,
    pub last: String,
    pub model: Option<String>,
    pub branch: Option<String>,
    pub cwd: Option<String>,
    /// Path of the underlying log file - useful for debugging + cache keying.
    pub source_path: Option<PathBuf>,
}

impl ParsedSession {
    pub fn into_session(self, project_path: String) -> Session {
        Session {
            id: self.id,
            provider: self.provider,
            project_path,
            title: self.title,
            when: self.when.to_rfc3339(),
            turns: self.turns as i64,
            duration: self.duration,
            status: self.status,
            last: self.last,
            model: self.model.unwrap_or_default(),
            branch: self.branch,
        }
    }

    pub fn detail(&self) -> SessionDetail {
        SessionDetail {
            provider: self.provider.clone(),
            id: self.id.clone(),
            cwd: self.cwd.clone(),
            model: self.model.clone(),
            branch: self.branch.clone(),
            source_path: self
                .source_path
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned()),
        }
    }
}

/// Supplementary info kept in [`crate::sessions::SessionsManager`]'s cache.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDetail {
    pub provider: ProviderId,
    pub id: String,
    pub cwd: Option<String>,
    pub model: Option<String>,
    pub branch: Option<String>,
    pub source_path: Option<String>,
}

/// Argv + cwd a Tauri command should hand to the terminal pane / shell.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct ResumeInvocation {
    pub provider: ProviderId,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: String,
}

/// What the Settings panel + Sessions tab need to know about a provider.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(
    export,
    export_to = "../../src/types/rust.ts",
    rename_all = "camelCase"
)]
pub struct ProviderInfo {
    pub id: ProviderId,
    pub label: String,
    pub binary_name: String,
    /// Binary discovered on PATH at the time of the call.
    pub available: bool,
    /// Reflects the user's setting (defaults to true when unset).
    pub enabled: bool,
    /// True when the user picked this as the default for "+ new session".
    pub is_default: bool,
}

// ---------- Trait ----------

pub trait SessionProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn label(&self) -> &'static str;
    fn binary_name(&self) -> &'static str;

    /// Lookup the binary on `PATH`. Default impl uses [`shared::which`].
    fn is_available(&self) -> bool {
        shared::which(self.binary_name()).is_some()
    }

    /// Discover sessions belonging to a project. May return an empty Vec
    /// (e.g. provider unconfigured).
    fn list_for_project(&self, project_path: &Path) -> anyhow::Result<Vec<ParsedSession>>;

    /// argv to resume a known session. Falls back to `new_invocation` when
    /// the provider has no native resume command.
    fn resume_invocation(&self, detail: &SessionDetail) -> ResumeInvocation;

    /// argv to spawn a fresh session in `project_path`.
    fn new_invocation(&self, project_path: &Path) -> ResumeInvocation;
}

// ---------- Registry ----------

pub struct ProvidersRegistry {
    providers: Vec<Arc<dyn SessionProvider>>,
}

impl ProvidersRegistry {
    pub fn with_defaults() -> Self {
        Self {
            providers: vec![
                Arc::new(claude::ClaudeProvider),
                Arc::new(codex::CodexProvider),
                Arc::new(opencode::OpenCodeProvider),
            ],
        }
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn SessionProvider>> {
        self.providers.iter().find(|p| p.id() == id).cloned()
    }

    pub fn enabled<'a>(
        &'a self,
        settings: &'a crate::storage::types::ProvidersSettings,
    ) -> Vec<Arc<dyn SessionProvider>> {
        self.providers
            .iter()
            .filter(|p| settings.is_enabled(p.id()))
            .cloned()
            .collect()
    }

    /// Build [`ProviderInfo`] entries for every registered provider.
    pub fn describe_all(
        &self,
        settings: &crate::storage::types::ProvidersSettings,
    ) -> Vec<ProviderInfo> {
        self.providers
            .iter()
            .map(|p| ProviderInfo {
                id: p.id().to_string(),
                label: p.label().to_string(),
                binary_name: p.binary_name().to_string(),
                available: p.is_available(),
                enabled: settings.is_enabled(p.id()),
                is_default: settings.default_id == p.id(),
            })
            .collect()
    }
}
