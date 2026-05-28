use super::*;

fn dependency_state_for(
    metadata: &SkillMetadata,
    names: &std::collections::HashSet<String>,
) -> SkillState {
    let missing: Vec<String> = metadata
        .depends
        .iter()
        .filter(|dep| !names.contains(dep.as_str()))
        .cloned()
        .collect();
    if missing.is_empty() {
        SkillState::Discovered
    } else {
        SkillState::PendingDeps { missing }
    }
}

impl SkillCatalog {
    pub(crate) fn refresh_dependency_states(&self) {
        let names: std::collections::HashSet<String> = self
            .entries
            .iter()
            .map(|entry| entry.key().clone())
            .collect();
        for mut entry in self.entries.iter_mut() {
            if self.loaded.contains(entry.key()) {
                entry.state = SkillState::Loaded;
                continue;
            }
            if matches!(entry.state, SkillState::Error(_)) {
                continue;
            }
            entry.state = dependency_state_for(&entry.metadata, &names);
        }
    }

    /// Create a new, empty catalog backed by the given registry.
    pub fn new(registry: Arc<ToolRegistry>) -> Self {
        Self {
            entries: DashMap::new(),
            loaded: DashSet::new(),
            registry,
            dispatcher: None,
            event_bus: EventBus::new(),
            script_executor: RwLock::new(None),
            load_transform: RwLock::new(None),
            after_load_hook: RwLock::new(None),
            after_unload_hook: RwLock::new(None),
            after_group_change_hook: RwLock::new(None),
            active_groups: DashSet::new(),
        }
    }

    /// Create a catalog with an attached dispatcher for Skills-First execution.
    pub fn new_with_dispatcher(
        registry: Arc<ToolRegistry>,
        dispatcher: Arc<ToolDispatcher>,
    ) -> Self {
        let event_bus = dispatcher.event_bus();
        Self {
            entries: DashMap::new(),
            loaded: DashSet::new(),
            registry,
            dispatcher: Some(dispatcher),
            event_bus,
            script_executor: RwLock::new(None),
            load_transform: RwLock::new(None),
            after_load_hook: RwLock::new(None),
            after_unload_hook: RwLock::new(None),
            after_group_change_hook: RwLock::new(None),
            active_groups: DashSet::new(),
        }
    }

    /// Attach a dispatcher after construction (builder-style).
    pub fn with_dispatcher(mut self, dispatcher: Arc<ToolDispatcher>) -> Self {
        self.event_bus = dispatcher.event_bus();
        self.dispatcher = Some(dispatcher);
        self
    }

    /// Attach a lifecycle event bus after construction (builder-style).
    pub fn with_event_bus(mut self, event_bus: EventBus) -> Self {
        self.event_bus = event_bus;
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

    /// Register an adapter-owned skill metadata transform.
    ///
    /// The transform runs for every skill load path before tools are
    /// registered. Returning `Err` vetoes the load with a structured
    /// validation event. The returned metadata must keep the original skill
    /// name so catalog keys, dependency tracking, and tool names remain stable.
    pub fn set_skill_load_transform<F>(&self, transform: F)
    where
        F: Fn(SkillMetadata) -> Result<SkillMetadata, String> + Send + Sync + 'static,
    {
        *self.load_transform.write() = Some(Arc::new(transform));
    }

    /// Register a pre-boxed skill-load transform.
    pub fn set_skill_load_transform_arc(&self, transform: Arc<SkillLoadTransformFn>) {
        *self.load_transform.write() = Some(transform);
    }

    /// Remove the adapter-owned skill metadata transform.
    pub fn clear_skill_load_transform(&self) {
        *self.load_transform.write() = None;
    }

    /// Register an observer that runs after a skill's tools are registered.
    ///
    /// Observer errors are emitted as lifecycle events and logged, but they do
    /// not roll back the load. Use the transform hook when an adapter needs to
    /// veto a load before registration.
    pub fn set_after_load_hook<F>(&self, hook: F)
    where
        F: Fn(&SkillMetadata, &[String]) -> Result<(), String> + Send + Sync + 'static,
    {
        *self.after_load_hook.write() = Some(Arc::new(hook));
    }

    /// Register a pre-boxed after-load observer.
    pub fn set_after_load_hook_arc(&self, hook: Arc<AfterSkillLoadFn>) {
        *self.after_load_hook.write() = Some(hook);
    }

    /// Remove the after-load observer.
    pub fn clear_after_load_hook(&self) {
        *self.after_load_hook.write() = None;
    }

    /// Register an observer that runs after a skill is unloaded (#1405).
    ///
    /// Symmetric to [`Self::set_after_load_hook`] — the catalog calls the
    /// hook with the unloaded skill name and the list of tool names that
    /// were removed from the registry. Used by the persistence layer to
    /// evict the row from the on-disk store.
    pub fn set_after_unload_hook<F>(&self, hook: F)
    where
        F: Fn(&str, &[String]) -> Result<(), String> + Send + Sync + 'static,
    {
        *self.after_unload_hook.write() = Some(Arc::new(hook));
    }

    /// Remove the after-unload observer.
    pub fn clear_after_unload_hook(&self) {
        *self.after_unload_hook.write() = None;
    }

    /// Register an observer that runs after [`Self::activate_group`] /
    /// [`Self::deactivate_group`] (#1405). `activated` is `true` for
    /// activate and `false` for deactivate; the persistence layer uses
    /// it to mirror catalog-wide group state on disk.
    pub fn set_after_group_change_hook<F>(&self, hook: F)
    where
        F: Fn(&str, bool) -> Result<(), String> + Send + Sync + 'static,
    {
        *self.after_group_change_hook.write() = Some(Arc::new(hook));
    }

    /// Remove the after-group-change observer.
    pub fn clear_after_group_change_hook(&self) {
        *self.after_group_change_hook.write() = None;
    }

    /// Discover skills from the standard scan paths.
    pub fn discover(&self, extra_paths: Option<&[String]>, dcc_name: Option<&str>) -> usize {
        let result = match loader::scan_and_load_lenient_with_sources(extra_paths, dcc_name) {
            Ok(result) => result,
            Err(err) => {
                tracing::error!("SkillCatalog: discovery failed: {err}");
                return 0;
            }
        };

        let mut new_count = 0;
        for (skill, path_source) in result.skills {
            let name = skill.name.clone();
            if !self.entries.contains_key(&name) {
                self.entries.insert(
                    name,
                    SkillEntry {
                        metadata: skill,
                        state: SkillState::Discovered,
                        registered_tools: Vec::new(),
                        scope: SkillScope::Repo,
                        path_source,
                    },
                );
                new_count += 1;
            }
        }
        self.refresh_dependency_states();

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

    /// Re-scan the current search roots and make the catalog match disk.
    ///
    /// Unlike [`discover`](Self::discover), this removes catalog entries that
    /// are no longer present in the scan result. It is intended for explicit
    /// refresh flows such as the Admin skill-path panel.
    pub fn rediscover(&self, extra_paths: Option<&[String]>, dcc_name: Option<&str>) -> usize {
        let result = match loader::scan_and_load_lenient_with_sources(extra_paths, dcc_name) {
            Ok(result) => result,
            Err(err) => {
                tracing::error!("SkillCatalog: rediscovery failed: {err}");
                return 0;
            }
        };

        let mut seen = std::collections::HashSet::new();
        let mut added = 0usize;

        for (skill, path_source) in result.skills {
            let name = skill.name.clone();
            seen.insert(name.clone());
            if let Some(mut entry) = self.entries.get_mut(&name) {
                entry.metadata = skill;
                entry.path_source = path_source;
                if entry.state != SkillState::Loaded {
                    entry.state = SkillState::Discovered;
                }
            } else {
                self.entries.insert(
                    name,
                    SkillEntry {
                        metadata: skill,
                        state: SkillState::Discovered,
                        registered_tools: Vec::new(),
                        scope: SkillScope::Repo,
                        path_source,
                    },
                );
                added += 1;
            }
        }

        let existing: Vec<String> = self
            .entries
            .iter()
            .map(|entry| entry.key().clone())
            .collect();
        let mut removed = 0usize;
        for name in existing {
            if !seen.contains(&name) && self.remove_skill(&name) {
                removed += 1;
            }
        }
        self.refresh_dependency_states();

        if !result.skipped.is_empty() {
            tracing::warn!(
                count = result.skipped.len(),
                skipped = ?result.skipped,
                "SkillCatalog: skipped invalid or non-compliant skill directories during rediscovery"
            );
        }

        tracing::info!(
            "SkillCatalog: rediscovered skills (added {}, removed {}, total {})",
            added,
            removed,
            self.entries.len()
        );
        added + removed
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
                    path_source: Default::default(),
                },
            );
        }
        self.refresh_dependency_states();
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
                            path_source: Default::default(),
                        },
                    );
                    total_new += 1;
                }
            }
        }
        self.refresh_dependency_states();
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
