use super::*;

impl SkillCatalog {
    /// Create a new, empty catalog backed by the given registry.
    ///
    /// Without a dispatcher, `load_skill` only registers action metadata.
    /// Use [`with_dispatcher`](Self::with_dispatcher) to also auto-register
    /// script handlers for the Skills-First workflow.
    pub fn new(registry: Arc<ActionRegistry>) -> Self {
        Self {
            entries: DashMap::new(),
            loaded: DashSet::new(),
            registry,
            dispatcher: None,
            script_executor: None,
            active_groups: DashSet::new(),
        }
    }

    /// Create a catalog with an attached dispatcher for Skills-First execution.
    ///
    /// When a dispatcher is attached, calling `load_skill` automatically
    /// registers a handler for every script in the skill.
    /// Agents can then call `tools/call` and have scripts actually execute.
    pub fn new_with_dispatcher(
        registry: Arc<ActionRegistry>,
        dispatcher: Arc<ActionDispatcher>,
    ) -> Self {
        Self {
            entries: DashMap::new(),
            loaded: DashSet::new(),
            registry,
            dispatcher: Some(dispatcher),
            script_executor: None,
            active_groups: DashSet::new(),
        }
    }

    /// Attach a dispatcher after construction (builder-style).
    pub fn with_dispatcher(mut self, dispatcher: Arc<ActionDispatcher>) -> Self {
        self.dispatcher = Some(dispatcher);
        self
    }

    /// Register an **in-process** script executor (builder-style).
    ///
    /// When set, skill scripts are executed directly in the current Python
    /// interpreter rather than being spawned as child processes.  This is the
    /// correct approach for DCC applications (Maya, Blender, Houdini, …) where
    /// the host DCC already provides its own Python interpreter with all DCC
    /// modules pre-imported.
    ///
    /// # Example (from a DCC adapter)
    /// ```no_run
    /// use dcc_mcp_skills::catalog::SkillCatalog;
    /// use dcc_mcp_actions::ActionRegistry;
    /// use std::sync::Arc;
    ///
    /// let registry = Arc::new(ActionRegistry::new());
    /// let catalog = SkillCatalog::new(registry)
    ///     .with_in_process_executor(|script_path, params| {
    ///         // Execute script_path inside the DCC's Python environment
    ///         // (implemented by the DCC adapter via PyO3 or cffi)
    ///         Ok(serde_json::json!({"success": true}))
    ///     });
    /// ```
    pub fn with_in_process_executor<F>(mut self, executor: F) -> Self
    where
        F: Fn(String, serde_json::Value) -> Result<serde_json::Value, String>
            + Send
            + Sync
            + 'static,
    {
        self.script_executor = Some(Arc::new(executor));
        self
    }

    /// Replace the in-process executor after construction.
    pub fn set_in_process_executor<F>(&mut self, executor: F)
    where
        F: Fn(String, serde_json::Value) -> Result<serde_json::Value, String>
            + Send
            + Sync
            + 'static,
    {
        self.script_executor = Some(Arc::new(executor));
    }

    /// Remove the in-process executor, reverting to subprocess execution.
    pub fn clear_in_process_executor(&mut self) {
        self.script_executor = None;
    }

    /// Discover skills from the standard scan paths.
    ///
    /// Uses `scan_and_load_lenient` internally so skills with missing
    /// dependencies are skipped rather than causing an error.
    ///
    /// Returns the number of newly discovered skills.
    pub fn discover(&self, extra_paths: Option<&[String]>, dcc_name: Option<&str>) -> usize {
        let result = match loader::scan_and_load_lenient(extra_paths, dcc_name) {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("SkillCatalog: discovery failed: {e}");
                return 0;
            }
        };

        let mut new_count = 0;
        for skill in result.skills {
            let name = skill.name.clone();
            // Only insert if not already known (don't overwrite loaded state)
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
            tracing::debug!("SkillCatalog: skipped {} directories", result.skipped.len());
        }

        tracing::info!(
            "SkillCatalog: discovered {} new skill(s), total {}",
            new_count,
            self.entries.len()
        );
        new_count
    }

    /// Add a single skill to the catalog (e.g. from SkillWatcher).
    ///
    /// If the skill is already in the catalog and loaded, it is not replaced.
    /// If it exists but is discovered, the metadata is updated.
    pub fn add_skill(&self, metadata: SkillMetadata) {
        let name = metadata.name.clone();
        if let Some(mut entry) = self.entries.get_mut(&name) {
            // Only update metadata if not loaded
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
    ///
    /// Like [`discover`](Self::discover) but lets the caller tag each set of
    /// paths with a trust level so tools like `list_skills` can surface scope
    /// information to AI agents.
    ///
    /// ```no_run
    /// # use dcc_mcp_skills::catalog::SkillCatalog;
    /// # use dcc_mcp_models::SkillScope;
    /// # use dcc_mcp_actions::ActionRegistry;
    /// # use std::sync::Arc;
    /// # let registry = Arc::new(ActionRegistry::new());
    /// # let catalog = SkillCatalog::new(registry);
    /// let count = catalog.discover_scoped(
    ///     &[
    ///         (SkillScope::Repo,   vec!["./.dcc_skills".to_string()]),
    ///         (SkillScope::User,   vec!["~/.dcc_mcp/skills".to_string()]),
    ///         (SkillScope::System, vec!["/usr/share/dcc_mcp/skills".to_string()]),
    ///     ],
    ///     Some("maya"),
    /// );
    /// ```
    pub fn discover_scoped(
        &self,
        scoped_paths: &[(SkillScope, Vec<String>)],
        dcc_name: Option<&str>,
    ) -> usize {
        let mut total_new = 0;
        for (scope, paths) in scoped_paths {
            let result =
                match crate::loader::scan_and_load_lenient(Some(paths.as_slice()), dcc_name) {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::error!(
                            "SkillCatalog::discover_scoped: scan failed for scope={scope}: {e}"
                        );
                        continue;
                    }
                };

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

    /// Load a skill by name — registers its tools into ActionRegistry and,
    /// if a dispatcher is attached, auto-registers script execution handlers.
    ///
    /// This is the Skills-First path: agents can call `load_skill` and then
    /// immediately use `tools/call` without any extra handler registration.
    ///
    /// **Script lookup order** for each action:
    /// 1. `ToolDeclaration.source_file` (explicit mapping)
    /// 2. `scripts/<tool_name>.<ext>` (name-matched script)
    /// 3. The first script in the skill if only one script exists
    /// 4. No handler registered (tool visible but not executable)
    ///
    /// Returns the list of action names that were registered, or an error
    /// description if the skill could not be loaded.
    pub fn load_skill(&self, skill_name: &str) -> Result<Vec<String>, String> {
        // Check if already loaded
        if self.loaded.contains(skill_name) {
            let actions = self
                .entries
                .get(skill_name)
                .map(|e| e.registered_tools.clone())
                .unwrap_or_default();
            return Ok(actions);
        }

        // Get the skill entry
        let metadata = {
            self.entries
                .get(skill_name)
                .map(|e| e.metadata.clone())
                .ok_or_else(|| format!("Skill '{skill_name}' not found in catalog"))
        }?;

        // Register tools from the skill
        let mut registered = Vec::new();
        let skill_base = metadata.name.replace('-', "_");
        let skill_path = std::path::Path::new(&metadata.skill_path);

        // Seed active_groups from default-active entries declared in the SKILL.md
        for group in &metadata.groups {
            if group.default_active {
                self.active_groups.insert(group.name.clone());
            }
        }

        for tool_decl in &metadata.tools {
            let action_name = if tool_decl.name.contains("__") {
                tool_decl.name.clone()
            } else {
                format!("{}__{}", skill_base, tool_decl.name.replace('-', "_"))
            };

            // Resolve the script that backs this tool declaration
            let script_path = resolve_tool_script(tool_decl, &metadata.scripts, skill_path);

            let meta = ActionMeta {
                name: action_name.clone(),
                description: if tool_decl.description.is_empty() {
                    format!("[{}] {}", metadata.name, metadata.description)
                } else {
                    tool_decl.description.clone()
                },
                category: metadata.tags.first().cloned().unwrap_or_default(),
                tags: metadata.tags.clone(),
                dcc: metadata.dcc.clone(),
                version: metadata.version.clone(),
                input_schema: if tool_decl.input_schema.is_null() {
                    serde_json::json!({"type": "object"})
                } else {
                    tool_decl.input_schema.clone()
                },
                output_schema: tool_decl.output_schema.clone(),
                source_file: script_path.clone(),
                skill_name: Some(skill_name.to_string()),
                group: tool_decl.group.clone(),
                // Disable at registration when the declared group is not
                // default-active; default groups (empty group name or an
                // explicitly default-active group) stay enabled.
                enabled: group_default_active(&metadata.groups, &tool_decl.group),
                required_capabilities: tool_decl.required_capabilities.clone(),
                execution: tool_decl.execution,
                timeout_hint_secs: tool_decl.timeout_hint_secs,
                thread_affinity: tool_decl.thread_affinity,
                annotations: tool_decl.annotations.clone(),
                next_tools: helpers::sanitize_next_tools(&tool_decl.next_tools, skill_name, &action_name),
            };

            self.registry.register_action(meta);

            // Auto-register handler if dispatcher is attached.
            // Prefer in-process execution (DCC adapters) over subprocess.
            if let (Some(dispatcher), Some(sp)) = (&self.dispatcher, script_path) {
                let sp_owned = sp.clone();
                let name_clone = action_name.clone();
                let dcc_owned = metadata.dcc.clone();
                if let Some(executor) = &self.script_executor {
                    let executor = Arc::clone(executor);
                    dispatcher.register_handler(&name_clone, move |params| {
                        executor(sp_owned.clone(), params)
                    });
                } else {
                    dispatcher.register_handler(&name_clone, move |params| {
                        execute_script(&sp_owned, params, Some(dcc_owned.as_str()))
                    });
                }
            }

            registered.push(action_name);
        }

        // Script-only path: no explicit tool declarations → one action per script
        if metadata.tools.is_empty() {
            for script_path in &metadata.scripts {
                let stem = std::path::Path::new(script_path)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown");
                let action_name = format!("{}__{}", skill_base, stem.replace('-', "_"));

                let meta = ActionMeta {
                    name: action_name.clone(),
                    description: format!("[{}] {}", metadata.name, metadata.description),
                    category: metadata.tags.first().cloned().unwrap_or_default(),
                    tags: metadata.tags.clone(),
                    dcc: metadata.dcc.clone(),
                    version: metadata.version.clone(),
                    input_schema: serde_json::json!({"type": "object"}),
                    output_schema: serde_json::Value::Null,
                    source_file: Some(script_path.clone()),
                    skill_name: Some(skill_name.to_string()),
                    group: String::new(),
                    enabled: true,
                    required_capabilities: Vec::new(),
                    execution: dcc_mcp_models::ExecutionMode::Sync,
                    timeout_hint_secs: None,
                    thread_affinity: dcc_mcp_models::ThreadAffinity::Any,
                    annotations: dcc_mcp_models::ToolAnnotations::default(),
                    next_tools: dcc_mcp_models::NextTools::default(),
                };

                self.registry.register_action(meta);

                // Auto-register handler; prefer in-process execution for DCC hosts.
                if let Some(dispatcher) = &self.dispatcher {
                    let sp = script_path.clone();
                    let name_clone = action_name.clone();
                    let dcc_owned = metadata.dcc.clone();
                    if let Some(executor) = &self.script_executor {
                        let executor = Arc::clone(executor);
                        dispatcher.register_handler(&name_clone, move |params| {
                            executor(sp.clone(), params)
                        });
                    } else {
                        dispatcher.register_handler(&name_clone, move |params| {
                            execute_script(&sp, params, Some(dcc_owned.as_str()))
                        });
                    }
                }

                registered.push(action_name);
            }
        }

        // Update catalog state
        if let Some(mut entry) = self.entries.get_mut(skill_name) {
            entry.state = SkillState::Loaded;
            entry.registered_tools = registered.clone();
        }
        self.loaded.insert(skill_name.to_string());

        let handler_mode = match (&self.dispatcher, &self.script_executor) {
            (Some(_), Some(_)) => "in-process",
            (Some(_), None) => "subprocess",
            (None, _) => "none",
        };
        tracing::info!(
            "SkillCatalog: loaded skill '{}' ({} tools registered, handlers: {})",
            skill_name,
            registered.len(),
            handler_mode,
        );

        Ok(registered)
    }

    /// Load multiple skills at once.
    ///
    /// Returns a map of skill_name -> Ok(action_names) or Err(error_msg).
    pub fn load_skills(
        &self,
        skill_names: &[String],
    ) -> std::collections::HashMap<String, Result<Vec<String>, String>> {
        let mut results = std::collections::HashMap::new();
        for name in skill_names {
            results.insert(name.clone(), self.load_skill(name));
        }
        results
    }

    /// Unload a skill — removes its tools from ActionRegistry and dispatcher.
    ///
    /// Returns the number of actions that were unregistered.
    pub fn unload_skill(&self, skill_name: &str) -> Result<usize, String> {
        if !self.loaded.contains(skill_name) {
            return Err(format!("Skill '{skill_name}' is not loaded"));
        }

        // Collect action names before unregistering
        let action_names: Vec<String> = self
            .entries
            .get(skill_name)
            .map(|e| e.registered_tools.clone())
            .unwrap_or_default();

        // Remove handlers from dispatcher
        if let Some(dispatcher) = &self.dispatcher {
            for name in &action_names {
                dispatcher.remove_handler(name);
            }
        }

        let count = self.registry.unregister_skill(skill_name);

        // Update catalog state
        if let Some(mut entry) = self.entries.get_mut(skill_name) {
            entry.state = SkillState::Discovered;
            entry.registered_tools.clear();
        }
        self.loaded.remove(skill_name);

        tracing::info!(
            "SkillCatalog: unloaded skill '{}' ({} tools removed)",
            skill_name,
            count
        );

        Ok(count)
    }

    /// Search for skills matching the given criteria.
    ///
    /// The `tags` and `dcc` filters are applied first (AND semantics). If a
    /// non-empty `query` is provided, the remaining skills are ranked with a
    /// BM25-lite scorer that tokenises name, tags, search_hint, description,
    /// sibling `tools.yaml` entries (tool names + descriptions) and `dcc`.
    /// See [`scoring`] for weights, tie-breaks and the exact-name fast path.
    ///
    /// When `query` is `None` or empty the pre-filter result is returned in
    /// a deterministic order (scope descending, then alphabetical name), so
    /// callers don't observe `DashMap` iteration order.
    pub fn find_skills(
        &self,
        query: Option<&str>,
        tags: &[&str],
        dcc: Option<&str>,
    ) -> Vec<SkillSummary> {
        // ── 1. Pre-filter by tags/dcc (AND semantics) ──
        // Collect to owned entries so we can borrow them for the ranker and
        // also produce a deterministic iteration order independent of DashMap.
        let mut prefiltered: Vec<SkillEntry> = self
            .entries
            .iter()
            .filter(|entry| {
                let meta = &entry.value().metadata;

                if !tags.is_empty() {
                    for tag in tags {
                        if !meta.tags.iter().any(|t| t.eq_ignore_ascii_case(tag)) {
                            return false;
                        }
                    }
                }

                if let Some(dcc_filter) = dcc {
                    if !dcc_filter.is_empty() && !meta.dcc.eq_ignore_ascii_case(dcc_filter) {
                        return false;
                    }
                }

                true
            })
            .map(|entry| entry.value().clone())
            .collect();

        // ── 2. No query → deterministic order, no ranking ──
        let q_trim = query.map(str::trim).unwrap_or("");
        if q_trim.is_empty() {
            prefiltered.sort_by(|a, b| {
                b.scope
                    .cmp(&a.scope)
                    .then_with(|| a.metadata.name.cmp(&b.metadata.name))
            });
            return prefiltered.iter().map(helpers::skill_entry_to_summary).collect();
        }

        // ── 3. BM25-lite scoring ──
        let metas: Vec<&SkillMetadata> = prefiltered.iter().map(|e| &e.metadata).collect();
        let scopes: Vec<SkillScope> = prefiltered.iter().map(|e| e.scope).collect();
        let scored = scoring::score_skills(q_trim, &metas, &scopes);

        scored
            .into_iter()
            .map(|s| helpers::skill_entry_to_summary(&prefiltered[s.index]))
            .collect()
    }

    /// Unified skill discovery — superset of [`find_skills`](Self::find_skills).
    ///
    /// Combines the filters previously split across `find_skills` and
    /// `search_skills`. Issue #340 (`find_skills` → `search_skills` merge).
    ///
    /// Behaviour:
    /// - `query` / `tags` / `dcc` are AND-ed through [`find_skills`] — ranking
    ///   and scoring (including the #343 BM25-lite ranker) are reused as-is.
    /// - `scope` restricts the result to one [`SkillScope`] level. The filter
    ///   is applied post-ranking so high-scoring skills from other scopes
    ///   don't shuffle the order.
    /// - Empty `query` with no other filters returns the whole catalog
    ///   sorted by scope precedence (Admin > System > User > Repo) then
    ///   alphabetical name — the "discovery mode" entry point for agents.
    /// - `limit` caps the number of summaries returned; `None` means no cap.
    pub fn search_skills(
        &self,
        query: Option<&str>,
        tags: &[&str],
        dcc: Option<&str>,
        scope: Option<SkillScope>,
        limit: Option<usize>,
    ) -> Vec<SkillSummary> {
        let ranked = self.find_skills(query, tags, dcc);

        let filtered: Vec<SkillSummary> = match scope {
            None => ranked,
            Some(scope_filter) => {
                let label = scope_filter.label();
                ranked
                    .into_iter()
                    .filter(|s| s.scope.eq_ignore_ascii_case(label))
                    .collect()
            }
        };

        match limit {
            None => filtered,
            Some(n) => filtered.into_iter().take(n).collect(),
        }
    }

    /// List all skills with their load status.
    pub fn list_skills(&self, status: Option<&str>) -> Vec<SkillSummary> {
        self.entries
            .iter()
            .filter(|entry| {
                let state = &entry.value().state;
                match status {
                    Some("loaded") => state == &SkillState::Loaded,
                    Some("unloaded") | Some("discovered") => state == &SkillState::Discovered,
                    Some("error") => matches!(state, SkillState::Error(_)),
                    _ => true, // "all" or None
                }
            })
            .map(|entry| helpers::skill_entry_to_summary(entry.value()))
            .collect()
    }

    /// Get detailed information about a specific skill.
    pub fn get_skill_info(&self, skill_name: &str) -> Option<SkillDetail> {
        self.entries.get(skill_name).map(|entry| {
            let e = entry.value();
            SkillDetail {
                name: e.metadata.name.clone(),
                description: e.metadata.description.clone(),
                tags: e.metadata.tags.clone(),
                dcc: e.metadata.dcc.clone(),
                version: e.metadata.version.clone(),
                depends: e.metadata.depends.clone(),
                skill_path: e.metadata.skill_path.clone(),
                scripts: e.metadata.scripts.clone(),
                tools: e.metadata.tools.clone(),
                state: e.state.to_string(),
                registered_tools: e.registered_tools.clone(),
                scope: e.scope.label().to_string(),
                implicit_invocation: e
                    .metadata
                    .policy
                    .as_ref()
                    .map(|p| p.is_implicit_invocation_allowed())
                    .unwrap_or(true),
                dependency_count: e
                    .metadata
                    .external_deps
                    .as_ref()
                    .map(|d| d.tools.len())
                    .unwrap_or(0),
            }
        })
    }

    /// Get the number of skills in the catalog.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true if the catalog is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get the number of loaded skills.
    #[must_use]
    pub fn loaded_count(&self) -> usize {
        self.loaded.len()
    }

    /// Check whether a specific skill is loaded.
    #[must_use]
    pub fn is_loaded(&self, skill_name: &str) -> bool {
        self.loaded.contains(skill_name)
    }

    /// Run a closure against every loaded skill's [`SkillMetadata`].
    ///
    /// Used by the MCP prompts primitive (issues #351, #355) to walk the
    /// currently-loaded skills and lazily parse their sibling
    /// `prompts.yaml` files on `prompts/list`. The closure is invoked
    /// while a read guard on the underlying `DashMap` shard is held, so
    /// it must not call back into the catalog (no `load_skill` /
    /// `unload_skill`) or deadlock is possible.
    pub fn for_each_loaded_metadata<F: FnMut(&dcc_mcp_models::SkillMetadata)>(&self, mut f: F) {
        for entry in self.entries.iter() {
            let e = entry.value();
            if e.state == SkillState::Loaded {
                f(&e.metadata);
            }
        }
    }

    /// Remove a skill from the catalog entirely.
    ///
    /// If the skill is loaded, it is unloaded first.
    pub fn remove_skill(&self, skill_name: &str) -> bool {
        if self.loaded.contains(skill_name) {
            let _ = self.unload_skill(skill_name);
        }
        self.entries.remove(skill_name).is_some()
    }

    /// Clear all skills from the catalog.
    ///
    /// Loaded skills are unloaded first.
    pub fn clear(&self) {
        let loaded_names: Vec<String> = self.loaded.iter().map(|r| r.key().clone()).collect();
        for name in loaded_names {
            let _ = self.unload_skill(&name);
        }
        self.entries.clear();
    }

    /// Get a reference to the underlying ActionRegistry.
    pub fn registry(&self) -> &Arc<ActionRegistry> {
        &self.registry
    }

    /// Get a reference to the attached dispatcher, if any.
    pub fn dispatcher(&self) -> Option<&Arc<ActionDispatcher>> {
        self.dispatcher.as_ref()
    }
}
