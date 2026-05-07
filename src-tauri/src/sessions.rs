//! Session manager — fan-out across [`crate::providers`] and a small cache
//! keyed by (provider, session_id) so `sessions.resume_info` can find a
//! session's cwd after the list call has populated the cache.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::providers::{ParsedSession, ProvidersRegistry, SessionDetail};
use crate::storage::types::{ProvidersSettings, Session};

pub struct SessionsManager {
    registry: Arc<ProvidersRegistry>,
    /// (provider_id, session_id) → cached resume detail.
    detail_cache: Mutex<HashMap<(String, String), SessionDetail>>,
}

impl SessionsManager {
    pub fn new(registry: Arc<ProvidersRegistry>) -> Self {
        Self {
            registry,
            detail_cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn registry(&self) -> &Arc<ProvidersRegistry> {
        &self.registry
    }

    /// List sessions for `project_path` across enabled providers, optionally
    /// filtered to a single provider id.
    pub fn list_for_project(
        &self,
        project_path: &Path,
        settings: &ProvidersSettings,
        provider_filter: Option<&str>,
    ) -> anyhow::Result<Vec<Session>> {
        let project_str = project_path.to_string_lossy().into_owned();
        let mut all: Vec<ParsedSession> = Vec::new();

        for provider in self.registry.enabled(settings) {
            if let Some(filter) = provider_filter {
                if provider.id() != filter {
                    continue;
                }
            }
            match provider.list_for_project(project_path) {
                Ok(parsed) => all.extend(parsed),
                Err(err) => {
                    tracing::warn!(
                        provider = provider.id(),
                        error = %err,
                        "list_for_project failed",
                    );
                }
            }
        }

        // Stash detail entries for resume lookups.
        {
            let mut cache = self.detail_cache.lock().unwrap();
            for p in &all {
                cache.insert((p.provider.clone(), p.id.clone()), p.detail());
            }
        }

        // Newest first.
        all.sort_by_key(|p| std::cmp::Reverse(p.when));
        Ok(all
            .into_iter()
            .map(|p| p.into_session(project_str.clone()))
            .collect())
    }

    /// Look up a previously-discovered session by id (any provider).
    pub fn session_detail(&self, session_id: &str) -> Option<SessionDetail> {
        let cache = self.detail_cache.lock().unwrap();
        for ((_, id), detail) in cache.iter() {
            if id == session_id {
                return Some(detail.clone());
            }
        }
        None
    }

    /// Look up by an explicit (provider, id) pair. Preferred when the caller
    /// already knows which provider the session belongs to (e.g. when the
    /// frontend sends the provider tag along with the resume request).
    pub fn session_detail_for(&self, provider: &str, session_id: &str) -> Option<SessionDetail> {
        self.detail_cache
            .lock()
            .unwrap()
            .get(&(provider.to_string(), session_id.to_string()))
            .cloned()
    }
}
