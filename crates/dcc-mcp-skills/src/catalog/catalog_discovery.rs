use super::*;

impl SkillCatalog {
    /// Create a new, empty catalog backed by the given registry.
    pub fn new(registry: Arc<ActionRegistry>) -> Self {
        Self {
            entries: DashMap::new(),
            loaded: DashSet::new(),
            registry,
            dispatcher: None,
            script_executor: RwLock::new(None),
            active_groups: DashSet::new(),
        }
    }

    /// Create a catalog with an attached dispatcher for Skills-First execution.
    pub fn new_with_dispatcher(
        registry: Arc<ActionRegistry>,
        dispatcher: Arc<ActionDispatcher>,
    ) -> Self {
        Self {
            entries: DashMap::new(),
            loaded: DashSet::new(),
            registry,
            dispatcher: Some(dispatcher),
            script_executor: RwLock::new(None),
            active_groups: DashSet::new(),
        }
    }

    /// Attach a dispatcher after construction (builder-style).
    pub fn with_dispatcher(mut self, dispatcher: Arc<ActionDispatcher>) -> Self {
        self.dispatcher = Some(dispatcher);
        self
    }

    /// Register an **in-process** script executor (builder-style).
    pub fn with_in_process_executor<F>(self, executor: F) -> Self
    where
        F: Fn(
                String,
                serde_json::Value,
                crate::catalog::execute::ScriptExecutionContext,
            ) -> Result<serde_json::Value, String>
            + Send
            + Sync
            + 'static,
    {
        *self.script_executor.write() = Some(Arc::new(executor));
        self
    }

    /// Replace the in-process executor after construction.
    ///
    /// Unlike the builder-style [`with_in_process_executor`](Self::with_in_process_executor),
    /// this method works on a shared `Arc<SkillCatalog>` (issue #464) — DCC
    /// adapters can call it between construction and the first `load_skill()`.
    pub fn set_in_process_executor<F>(&self, executor: F)
    where
        F: Fn(
                String,
                serde_json::Value,
                crate::catalog::execute::ScriptExecutionContext,
            ) -> Result<serde_json::Value, String>
            + Send
            + Sync
            + 'static,
    {
        *self.script_executor.write() = Some(Arc::new(executor));
    }

    /// Replace the in-process executor with a pre-boxed `Arc<ScriptExecutorFn>`.
    ///
    /// Useful when the executor is already in `Arc` form (e.g. from PyO3 bindings).
    pub fn set_in_process_executor_arc(&self, executor: Arc<ScriptExecutorFn>) {
        *self.script_executor.write() = Some(executor);
    }

    /// Remove the in-process executor, reverting to subprocess execution.
    pub fn clear_in_process_executor(&self) {
        *self.script_executor.write() = None;
    }

    /// Discover skills from the standard scan paths.
    pub fn discover(&self, extra_paths: Option<&[String]>, dcc_name: Option<&str>) -> usize {
        let result = match loader::scan_and_load_lenient(extra_paths, dcc_name) {
            Ok(result) => result,
            Err(err) => {
                tracing::error!("SkillCatalog: discovery failed: {err}");
                return 0;
            }
        };

        let mut new_count = 0;
        for skill in result.skills {
            let name = skill.name.clone();
            if !self.entries.contains_key(&name) {
                self.entries.insert(
                    name,
                    SkillEntry {
                        metadata: skill,
                        state: SkillState::Discovered,
                        registered_tools: Vec::new(),
                        scope: SkillScope::Repo,
                    },
                );
                new_count += 1;
            }
        }

        if !result.skipped.is_empty() {
            tracing::warn!(
                count = result.skipped.len(),
                skipped = ?result.skipped,
                "SkillCatalog: skipped invalid or non-compliant skill directories during discovery"
            );
        }

        tracing::info!(
            "SkillCatalog: discovered {} new skill(s), total {}",
            new_count,
            self.entries.len()
        );
        new_count
    }

    /// Add a single skill to the catalog (e.g. from SkillWatcher).
    pub fn add_skill(&self, metadata: SkillMetadata) {
        let name = metadata.name.clone();
        if let Some(mut entry) = self.entries.get_mut(&name) {
            if entry.state != SkillState::Loaded {
                entry.metadata = metadata;
                entry.state = SkillState::Discovered;
            }
        } else {
            self.entries.insert(
                name,
                SkillEntry {
                    metadata,
                    state: SkillState::Discovered,
                    registered_tools: Vec::new(),
                    scope: SkillScope::Repo,
                },
            );
        }
    }

    /// Discover skills from paths grouped by [`SkillScope`].
    pub fn discover_scoped(
        &self,
        scoped_paths: &[(SkillScope, Vec<String>)],
        dcc_name: Option<&str>,
    ) -> usize {
        let mut total_new = 0;
        for (scope, paths) in scoped_paths {
            let result =
                match crate::loader::scan_and_load_lenient(Some(paths.as_slice()), dcc_name) {
                    Ok(result) => result,
                    Err(err) => {
                        tracing::error!(
                            "SkillCatalog::discover_scoped: scan failed for scope={scope}: {err}"
                        );
                        continue;
                    }
                };

            if !result.skipped.is_empty() {
                tracing::warn!(
                    scope = %scope,
                    count = result.skipped.len(),
                    skipped = ?result.skipped,
                    "SkillCatalog::discover_scoped: skipped invalid or non-compliant skill directories during discovery"
                );
            }

            for skill in result.skills {
                let name = skill.name.clone();
                if !self.entries.contains_key(&name) {
                    self.entries.insert(
                        name,
                        SkillEntry {
                            metadata: skill,
                            state: SkillState::Discovered,
                            registered_tools: Vec::new(),
                            scope: *scope,
                        },
                    );
                    total_new += 1;
                }
            }
        }
        tracing::info!(
            "SkillCatalog::discover_scoped: {} new skill(s) across {} scope(s)",
            total_new,
            scoped_paths.len()
        );
        total_new
    }

    /// Discover user-level and team-level accumulated skills from environment variables.
    pub fn discover_user_and_team(&self, dcc_name: Option<&str>) -> usize {
        use crate::paths::{
            get_app_team_skill_paths_from_env, get_app_user_skill_paths_from_env,
            get_team_skill_paths_from_env, get_user_skill_paths_from_env,
        };

        let mut scoped_paths: Vec<(SkillScope, Vec<String>)> = Vec::new();

        let user_paths = if let Some(dcc) = dcc_name {
            get_app_user_skill_paths_from_env(dcc)
        } else {
            get_user_skill_paths_from_env()
        };
        if !user_paths.is_empty() {
            scoped_paths.push((SkillScope::User, user_paths));
        }

        let team_paths = if let Some(dcc) = dcc_name {
            get_app_team_skill_paths_from_env(dcc)
        } else {
            get_team_skill_paths_from_env()
        };
        if !team_paths.is_empty() {
            scoped_paths.push((SkillScope::Team, team_paths));
        }

        if scoped_paths.is_empty() {
            return 0;
        }

        self.discover_scoped(&scoped_paths, dcc_name)
    }
}
