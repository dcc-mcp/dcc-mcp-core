use super::*;
use serde_json::{Map, Value, json};

fn activate_skill_groups(catalog: &SkillCatalog, metadata: &SkillMetadata) {
    for group in &metadata.groups {
        if !group.name.is_empty() {
            catalog.activate_group(&group.name);
        }
    }
    for tool in &metadata.tools {
        if !tool.group.is_empty() {
            catalog.activate_group(&tool.group);
        }
    }
}

impl SkillCatalog {
    fn emit_skill_event(
        &self,
        event_name: &str,
        skill_name: &str,
        metadata: Option<&SkillMetadata>,
        attributes: Value,
    ) {
        if !self.event_bus.has_subscribers(event_name) {
            return;
        }

        let (source, attributes) = skill_event_payload(skill_name, metadata, attributes);

        let _ = self
            .event_bus
            .emit(event_name, source, Value::Object(Map::new()), attributes);
    }

    fn emit_vetoable_skill_event(
        &self,
        event_name: &str,
        skill_name: &str,
        metadata: Option<&SkillMetadata>,
        attributes: Value,
    ) -> Result<(), EventVeto> {
        let (source, attributes) = skill_event_payload(skill_name, metadata, attributes);
        let event = self.event_bus.before_event(
            event_name,
            source,
            Value::Object(Map::new()),
            attributes,
        )?;
        self.event_bus.publish_event(&event);
        Ok(())
    }

    fn apply_skill_load_transform(
        &self,
        skill_name: &str,
        metadata: SkillMetadata,
        activate_groups: bool,
    ) -> Result<SkillMetadata, String> {
        let Some(transform) = self.load_transform.read().clone() else {
            return Ok(metadata);
        };

        let original_name = metadata.name.clone();
        let transformed = transform(metadata).map_err(|reason| {
            let err = format!("Skill '{skill_name}' load transform vetoed: {reason}");
            self.emit_skill_event(
                "skill.validation_failed",
                skill_name,
                None,
                json!({
                    "error_kind": "load_transform_vetoed",
                    "error_message": err,
                    "veto_reason": reason,
                    "activate_groups": activate_groups,
                }),
            );
            err
        })?;

        if transformed.name != original_name || transformed.name != skill_name {
            let err = format!(
                "Skill '{skill_name}' load transform changed the skill name to '{}'; renaming during load is not supported",
                transformed.name
            );
            self.emit_skill_event(
                "skill.validation_failed",
                skill_name,
                Some(&transformed),
                json!({
                    "error_kind": "load_transform_renamed_skill",
                    "error_message": err,
                    "original_name": original_name,
                    "transformed_name": transformed.name,
                    "activate_groups": activate_groups,
                }),
            );
            return Err(err);
        }

        self.emit_skill_event(
            "skill.load_transform_applied",
            skill_name,
            Some(&transformed),
            json!({
                "activate_groups": activate_groups,
            }),
        );
        Ok(transformed)
    }

    fn notify_after_load_hook(
        &self,
        skill_name: &str,
        metadata: &SkillMetadata,
        registered: &[String],
    ) {
        let Some(hook) = self.after_load_hook.read().clone() else {
            return;
        };

        if let Err(reason) = hook(metadata, registered) {
            let err = format!("Skill '{skill_name}' after-load hook failed: {reason}");
            self.emit_skill_event(
                "skill.after_load_failed",
                skill_name,
                Some(metadata),
                json!({
                    "error_kind": "after_load_hook_failed",
                    "error_message": err,
                    "registered_tools": registered,
                    "registered_tool_count": registered.len(),
                }),
            );
            tracing::warn!(
                skill_name = skill_name,
                error = %reason,
                "SkillCatalog after-load hook failed"
            );
        }
    }

    /// Fire the after-unload observer (#1405). Mirrors
    /// [`Self::notify_after_load_hook`] — observer errors are logged
    /// and emitted as lifecycle events, never roll back the unload.
    fn notify_after_unload_hook(&self, skill_name: &str, unregistered: &[String]) {
        let Some(hook) = self.after_unload_hook.read().clone() else {
            return;
        };
        if let Err(reason) = hook(skill_name, unregistered) {
            let err = format!("Skill '{skill_name}' after-unload hook failed: {reason}");
            self.emit_skill_event(
                "skill.after_unload_failed",
                skill_name,
                None,
                json!({
                    "error_kind": "after_unload_hook_failed",
                    "error_message": err,
                    "unregistered_tools": unregistered,
                    "unregistered_tool_count": unregistered.len(),
                }),
            );
            tracing::warn!(
                skill_name = skill_name,
                error = %reason,
                "SkillCatalog after-unload hook failed"
            );
        }
    }

    /// Load a skill by name — registers its tools into ToolRegistry and,
    /// if a dispatcher is attached, auto-registers script execution handlers.
    pub fn load_skill(&self, skill_name: &str) -> Result<Vec<String>, String> {
        self.load_skill_with_options(skill_name, true)
    }

    /// Load a skill, optionally activating every declared tool group.
    pub fn load_skill_with_options(
        &self,
        skill_name: &str,
        activate_groups: bool,
    ) -> Result<Vec<String>, String> {
        let mut visiting = Vec::new();
        self.load_skill_recursive(skill_name, activate_groups, &mut visiting)
    }

    /// Load a caller-supplied skill metadata object through the normal catalog path.
    ///
    /// This is the adapter-facing escape hatch for runtime metadata policy
    /// changes. Adapters can call [`get_skill`](Self::get_skill), modify the
    /// returned copy, then pass it here so registration, dispatcher wiring,
    /// group activation, dependency checks, and lifecycle events remain
    /// centralized in core.
    pub fn load_skill_object(&self, metadata: SkillMetadata) -> Result<Vec<String>, String> {
        let skill_name = metadata.name.clone();
        if skill_name.trim().is_empty() {
            return Err("Cannot load a skill object with an empty name".to_string());
        }

        let (scope, path_source) = self
            .entries
            .get(&skill_name)
            .map(|entry| (entry.scope, entry.path_source))
            .unwrap_or((SkillScope::Repo, Default::default()));

        if self.loaded.contains(skill_name.as_str()) {
            self.unload_skill(&skill_name)?;
        }

        self.entries.insert(
            skill_name.clone(),
            SkillEntry {
                metadata,
                state: SkillState::Discovered,
                registered_tools: Vec::new(),
                scope,
                path_source,
            },
        );
        self.refresh_dependency_states();
        self.load_skill(&skill_name)
    }

    fn load_skill_recursive(
        &self,
        skill_name: &str,
        activate_groups: bool,
        visiting: &mut Vec<String>,
    ) -> Result<Vec<String>, String> {
        if self.loaded.contains(skill_name) {
            let actions = self
                .entries
                .get(skill_name)
                .map(|entry| {
                    if activate_groups {
                        activate_skill_groups(self, &entry.metadata);
                    }
                    entry.registered_tools.clone()
                })
                .unwrap_or_default();
            return Ok(actions);
        }

        if let Some(pos) = visiting.iter().position(|name| name == skill_name) {
            let mut cycle = visiting[pos..].to_vec();
            cycle.push(skill_name.to_string());
            let err = format!(
                "Cannot load skill '{skill_name}' because its dependencies contain a cycle: {}",
                cycle.join(" -> ")
            );
            self.emit_skill_event(
                "skill.validation_failed",
                skill_name,
                None,
                json!({
                    "error_kind": "dependency_cycle",
                    "error_message": err,
                    "cycle": cycle,
                }),
            );
            return Err(err);
        }

        let metadata = match self.entries.get(skill_name) {
            Some(entry) => entry.metadata.clone(),
            None => {
                let err = format!("Skill '{skill_name}' not found in catalog");
                self.emit_skill_event(
                    "skill.validation_failed",
                    skill_name,
                    None,
                    json!({
                        "error_kind": "not_found",
                        "error_message": err,
                    }),
                );
                return Err(err);
            }
        };
        let metadata = self.apply_skill_load_transform(skill_name, metadata, activate_groups)?;

        let known_names: std::collections::HashSet<String> = self
            .entries
            .iter()
            .map(|entry| entry.key().clone())
            .collect();
        let missing: Vec<String> = metadata
            .depends
            .iter()
            .filter(|dep| !known_names.contains(dep.as_str()))
            .cloned()
            .collect();
        if !missing.is_empty() {
            if let Some(mut entry) = self.entries.get_mut(skill_name) {
                entry.state = SkillState::PendingDeps {
                    missing: missing.clone(),
                };
            }
            let err = format!(
                "Skill '{skill_name}' is pending dependencies: missing {}. \
                 Discover or install the dependency skill(s), then retry load_skill('{skill_name}').",
                missing.join(", ")
            );
            self.emit_skill_event(
                "skill.validation_failed",
                skill_name,
                Some(&metadata),
                json!({
                    "error_kind": "missing_dependencies",
                    "error_message": err,
                    "missing_dependencies": missing,
                }),
            );
            return Err(err);
        }

        visiting.push(skill_name.to_string());
        for dep in &metadata.depends {
            if !self.loaded.contains(dep.as_str())
                && let Err(err) = self.load_skill_recursive(dep, activate_groups, visiting)
            {
                let err =
                    format!("Failed to load dependency '{dep}' for skill '{skill_name}': {err}");
                self.emit_skill_event(
                    "skill.validation_failed",
                    skill_name,
                    Some(&metadata),
                    json!({
                        "error_kind": "dependency_load_failed",
                        "error_message": err,
                        "dependency": dep,
                    }),
                );
                return Err(err);
            }
        }
        visiting.pop();

        if self.loaded.contains(skill_name) {
            return Ok(self
                .entries
                .get(skill_name)
                .map(|entry| entry.registered_tools.clone())
                .unwrap_or_default());
        }

        self.load_skill_metadata(skill_name, &metadata, activate_groups)
    }

    fn load_skill_metadata(
        &self,
        skill_name: &str,
        metadata: &SkillMetadata,
        activate_groups: bool,
    ) -> Result<Vec<String>, String> {
        let mut registered = Vec::new();
        let skill_base = metadata.name.replace('-', "_");
        let skill_path = std::path::Path::new(&metadata.skill_path);
        if let Err(veto) = self.emit_vetoable_skill_event(
            "skill.loading",
            skill_name,
            Some(metadata),
            json!({
                "activate_groups": activate_groups,
            }),
        ) {
            let err = format!(
                "Skill '{skill_name}' loading vetoed ({}): {}",
                veto.code, veto.reason
            );
            self.emit_skill_event(
                "skill.validation_failed",
                skill_name,
                Some(metadata),
                json!({
                    "error_kind": "event_vetoed",
                    "error_message": err,
                    "veto_code": veto.code,
                    "veto_reason": veto.reason,
                }),
            );
            return Err(err);
        }

        if activate_groups {
            activate_skill_groups(self, metadata);
        } else {
            for group in &metadata.groups {
                if group.default_active {
                    self.active_groups.insert(group.name.clone());
                }
            }
        }

        for tool_decl in &metadata.tools {
            let action_name = if tool_decl.name.contains("__") {
                tool_decl.name.clone()
            } else {
                format!("{}__{}", skill_base, tool_decl.name.replace('-', "_"))
            };

            let script_path = resolve_tool_script(tool_decl, &metadata.scripts, skill_path);
            let maybe_executor = self.script_executor.read().clone();
            if tool_decl.thread_affinity.is_main() && maybe_executor.is_none() {
                let err = format!(
                    "Tool '{}' requires thread_affinity='main', but no in-process executor is set. \
                     Ensure the DCC adapter calls set_in_process_executor() before loading skills.",
                    action_name
                );
                self.emit_skill_event(
                    "skill.validation_failed",
                    skill_name,
                    Some(metadata),
                    json!({
                        "error_kind": "missing_in_process_executor",
                        "error_message": err,
                        "tool_name": action_name,
                    }),
                );
                return Err(err);
            }

            // Generate input_schema: prefer tools.yaml if present, otherwise derive from Python signature
            let input_schema = if tool_decl.input_schema.is_null() {
                // Try to generate schema from Python script signature
                if let Some(ref script_path) = script_path {
                    crate::catalog::schema_gen::generate_input_schema(script_path, None)
                        .unwrap_or_else(|| serde_json::json!({"type": "object"}))
                } else {
                    serde_json::json!({"type": "object"})
                }
            } else {
                // Validate schema against Python signature if possible
                if let Some(ref script_path) = script_path {
                    crate::catalog::schema_gen::validate_schema_drift(
                        &action_name,
                        &tool_decl.input_schema,
                        Some(script_path.as_str()),
                    );
                }
                tool_decl.input_schema.clone()
            };

            let meta = ToolMeta {
                name: action_name.clone(),
                description: if tool_decl.description.is_empty() {
                    format!("[{}] {}", metadata.name, metadata.description)
                } else {
                    tool_decl.description.clone()
                },
                category: metadata.tags.first().cloned().unwrap_or_default(),
                tags: metadata.tags.clone(),
                search_aliases: merged_search_aliases(
                    &metadata.search_aliases,
                    &tool_decl.search_aliases,
                ),
                dcc: metadata.dcc.clone(),
                version: metadata.version.clone(),
                input_schema,
                output_schema: tool_decl.output_schema.clone(),
                source_file: script_path.clone(),
                skill_name: Some(skill_name.to_string()),
                group: tool_decl.group.clone(),
                enabled: activate_groups
                    || group_default_active(&metadata.groups, &tool_decl.group),
                required_capabilities: tool_decl.required_capabilities.clone(),
                execution: tool_decl.execution,
                timeout_hint_secs: tool_decl.timeout_hint_secs,
                thread_affinity: tool_decl.thread_affinity,
                enforce_thread_affinity: tool_decl.enforce_thread_affinity,
                annotations: tool_decl.annotations.clone(),
                next_tools: helpers::sanitize_next_tools(
                    &tool_decl.next_tools,
                    skill_name,
                    &action_name,
                ),
            };

            self.registry.register_action(meta);

            if let (Some(dispatcher), Some(script_path)) = (&self.dispatcher, script_path) {
                let script_path_owned = script_path.clone();
                let action_name_clone = action_name.clone();
                let dcc_owned = metadata.dcc.clone();
                if let Some(executor) = maybe_executor {
                    let context = execute::ScriptExecutionContext {
                        action_name: action_name.clone(),
                        skill_name: Some(skill_name.to_string()),
                        thread_affinity: tool_decl.thread_affinity,
                        enforce_thread_affinity: tool_decl.enforce_thread_affinity,
                        execution: tool_decl.execution,
                        timeout_hint_secs: tool_decl.timeout_hint_secs,
                    };
                    dispatcher.register_handler(&action_name_clone, move |params| {
                        executor(script_path_owned.clone(), params, context.clone())
                    });
                } else {
                    dispatcher.register_handler(&action_name_clone, move |params| {
                        execute_script(&script_path_owned, params, Some(dcc_owned.as_str()))
                    });
                }
            }

            registered.push(action_name);
        }

        if metadata.tools.is_empty() {
            for script_path in &metadata.scripts {
                let stem = std::path::Path::new(script_path.as_str())
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown");
                let action_name = format!("{}__{}", skill_base, stem.replace('-', "_"));

                // Try to generate schema from Python script signature
                let input_schema =
                    crate::catalog::schema_gen::generate_input_schema(script_path, None)
                        .unwrap_or_else(|| serde_json::json!({"type": "object"}));

                let meta = ToolMeta {
                    name: action_name.clone(),
                    description: format!("[{}] {}", metadata.name, metadata.description),
                    category: metadata.tags.first().cloned().unwrap_or_default(),
                    tags: metadata.tags.clone(),
                    search_aliases: metadata.search_aliases.clone(),
                    dcc: metadata.dcc.clone(),
                    version: metadata.version.clone(),
                    input_schema,
                    output_schema: serde_json::Value::Null,
                    source_file: Some(script_path.clone()),
                    skill_name: Some(skill_name.to_string()),
                    group: String::new(),
                    enabled: true,
                    required_capabilities: Vec::new(),
                    execution: dcc_mcp_models::ExecutionMode::Sync,
                    timeout_hint_secs: None,
                    thread_affinity: dcc_mcp_models::ThreadAffinity::Any,
                    enforce_thread_affinity: false,
                    annotations: dcc_mcp_models::ToolAnnotations::default(),
                    next_tools: dcc_mcp_models::NextTools::default(),
                };

                self.registry.register_action(meta);

                if let Some(dispatcher) = &self.dispatcher {
                    let script_path_owned = script_path.clone();
                    let action_name_clone = action_name.clone();
                    let dcc_owned = metadata.dcc.clone();
                    let maybe_executor = self.script_executor.read().clone();
                    if let Some(executor) = maybe_executor {
                        let context = execute::ScriptExecutionContext {
                            action_name: action_name.clone(),
                            skill_name: Some(skill_name.to_string()),
                            thread_affinity: dcc_mcp_models::ThreadAffinity::Any,
                            enforce_thread_affinity: false,
                            execution: dcc_mcp_models::ExecutionMode::Sync,
                            timeout_hint_secs: None,
                        };
                        dispatcher.register_handler(&action_name_clone, move |params| {
                            executor(script_path_owned.clone(), params, context.clone())
                        });
                    } else {
                        dispatcher.register_handler(&action_name_clone, move |params| {
                            execute_script(&script_path_owned, params, Some(dcc_owned.as_str()))
                        });
                    }
                }

                registered.push(action_name);
            }
        }

        if let Some(mut entry) = self.entries.get_mut(skill_name) {
            entry.metadata = metadata.clone();
            entry.state = SkillState::Loaded;
            entry.registered_tools = registered.clone();
        }
        self.loaded.insert(skill_name.to_string());
        self.notify_after_load_hook(skill_name, metadata, &registered);

        let handler_mode = match (&self.dispatcher, &*self.script_executor.read()) {
            (Some(_), Some(_)) => "in-process",
            (Some(_), None) => "subprocess",
            (None, _) => "none",
        };
        let registered_tool_count = registered.len();
        self.emit_skill_event(
            "skill.loaded",
            skill_name,
            Some(metadata),
            json!({
                "registered_tools": registered.clone(),
                "registered_tool_count": registered_tool_count,
                "handler_mode": handler_mode,
            }),
        );
        tracing::info!(
            "SkillCatalog: loaded skill '{}' ({} tools registered, handlers: {})",
            skill_name,
            registered.len(),
            handler_mode,
        );

        Ok(registered)
    }

    /// Load multiple skills at once.
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

    /// Unload a skill — removes its tools from ToolRegistry and dispatcher.
    pub fn unload_skill(&self, skill_name: &str) -> Result<usize, String> {
        if !self.loaded.contains(skill_name) {
            return Err(format!("Skill '{skill_name}' is not loaded"));
        }
        let metadata = self
            .entries
            .get(skill_name)
            .map(|entry| entry.metadata.clone());

        let action_names: Vec<String> = self
            .entries
            .get(skill_name)
            .map(|entry| entry.registered_tools.clone())
            .unwrap_or_default();

        if let Some(dispatcher) = &self.dispatcher {
            for name in &action_names {
                dispatcher.remove_handler(name);
            }
        }

        let count = self.registry.unregister_skill(skill_name);

        if let Some(mut entry) = self.entries.get_mut(skill_name) {
            entry.state = SkillState::Discovered;
            entry.registered_tools.clear();
        }
        self.loaded.remove(skill_name);

        self.emit_skill_event(
            "skill.unloaded",
            skill_name,
            metadata.as_ref(),
            json!({
                "registered_tools": action_names,
                "removed_tool_count": count,
            }),
        );
        self.notify_after_unload_hook(skill_name, &action_names);
        tracing::info!(
            "SkillCatalog: unloaded skill '{}' ({} tools removed)",
            skill_name,
            count
        );

        Ok(count)
    }

    /// Remove a skill from the catalog entirely.
    pub fn remove_skill(&self, skill_name: &str) -> bool {
        if self.loaded.contains(skill_name) {
            let _ = self.unload_skill(skill_name);
        }
        let removed = self.entries.remove(skill_name).is_some();
        if removed {
            self.refresh_dependency_states();
        }
        removed
    }

    /// Clear all skills from the catalog.
    pub fn clear(&self) {
        let loaded_names: Vec<String> = self
            .loaded
            .iter()
            .map(|entry| entry.key().clone())
            .collect();
        for name in loaded_names {
            let _ = self.unload_skill(&name);
        }
        self.entries.clear();
    }

    /// Replay a persisted set of loaded skills + active groups (#1405).
    ///
    /// Walks `state.skills` in order and attempts to re-load each one
    /// using `load_skill_with_options(name, false)` so the catalog's own
    /// default-active group computation does not interfere with the
    /// explicit `state.active_groups` set replayed afterwards.
    ///
    /// The `policy` argument decides how to treat a record whose on-disk
    /// version differs from the one that was persisted:
    ///
    /// * [`crate::catalog::LoadReplayPolicy::SkipOnDrift`] (default) — the
    ///   record is added to [`ReplayReport::skipped_drift`] with a warning.
    /// * [`crate::catalog::LoadReplayPolicy::RequireExactVersion`] — same
    ///   as above. Embedders who want a fatal startup can inspect the
    ///   returned report and refuse to serve traffic.
    /// * [`crate::catalog::LoadReplayPolicy::IgnoreVersion`] — drift is
    ///   ignored and the on-disk version is loaded.
    ///
    /// During the catalog calls, the existing after-load /
    /// after-group-change hooks **fire as usual**. Hosts that want to
    /// avoid an immediate re-persistence round-trip should temporarily
    /// clear those hooks (or use a guard) before calling this method.
    pub fn replay_loaded(
        &self,
        state: &super::persistence::PersistedCatalogState,
        policy: super::persistence::LoadReplayPolicy,
    ) -> super::persistence::ReplayReport {
        use super::persistence::{DriftRecord, FailedRecord, LoadReplayPolicy, ReplayReport};

        let mut report = ReplayReport::default();
        for record in &state.skills {
            let Some(entry) = self.entries.get(&record.name).map(|e| e.value().clone()) else {
                tracing::warn!(
                    skill = %record.name,
                    "SkillCatalog::replay_loaded: skipping — skill not found in catalog"
                );
                report.missing.push(record.name.clone());
                continue;
            };

            let current_version = entry.metadata.version.clone();
            let drift_detected = match (&record.version, policy) {
                (Some(persisted), LoadReplayPolicy::SkipOnDrift)
                | (Some(persisted), LoadReplayPolicy::RequireExactVersion) => {
                    persisted.as_str() != current_version.as_str()
                }
                _ => false,
            };

            if drift_detected {
                tracing::warn!(
                    skill = %record.name,
                    persisted = ?record.version,
                    current = %current_version,
                    "SkillCatalog::replay_loaded: skipping — persisted version differs"
                );
                report.skipped_drift.push(DriftRecord {
                    name: record.name.clone(),
                    persisted_version: record.version.clone(),
                    current_version,
                });
                continue;
            }

            match self.load_skill_with_options(&record.name, false) {
                Ok(_) => report.loaded.push(record.name.clone()),
                Err(err) => {
                    tracing::warn!(
                        skill = %record.name,
                        error = %err,
                        "SkillCatalog::replay_loaded: load_skill returned an error"
                    );
                    report.failed.push(FailedRecord {
                        name: record.name.clone(),
                        error: err,
                    });
                }
            }
        }

        // Replay catalog-wide active group selection. Groups declared by
        // skills that failed to load are best-effort no-ops (the
        // registry has no tools tagged with them).
        for group in &state.active_groups {
            self.activate_group(group);
            report.activated_groups.push(group.clone());
        }

        tracing::info!(
            loaded = report.loaded.len(),
            missing = report.missing.len(),
            skipped_drift = report.skipped_drift.len(),
            failed = report.failed.len(),
            activated_groups = report.activated_groups.len(),
            "SkillCatalog::replay_loaded: complete"
        );
        report
    }
}

fn merged_search_aliases(skill_aliases: &[String], tool_aliases: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(skill_aliases.len() + tool_aliases.len());
    let mut seen = std::collections::HashSet::new();
    for alias in skill_aliases.iter().chain(tool_aliases.iter()) {
        let trimmed = alias.trim();
        if trimmed.is_empty() {
            continue;
        }
        let key = trimmed.to_ascii_lowercase();
        if seen.insert(key) {
            out.push(trimmed.to_string());
        }
    }
    out.truncate(24);
    out
}

fn skill_event_payload(
    skill_name: &str,
    metadata: Option<&SkillMetadata>,
    attributes: Value,
) -> (Value, Value) {
    let mut source = Map::new();
    let mut attrs = attributes.as_object().cloned().unwrap_or_default();
    attrs.insert("skill_name".to_string(), json!(skill_name));

    if let Some(metadata) = metadata {
        if !metadata.dcc.is_empty() {
            source.insert("dcc_type".to_string(), json!(metadata.dcc));
            attrs.insert("dcc_type".to_string(), json!(metadata.dcc));
        }
        if !metadata.version.is_empty() {
            attrs.insert("version".to_string(), json!(metadata.version));
        }
        if !metadata.skill_path.is_empty() {
            attrs.insert("skill_path".to_string(), json!(metadata.skill_path));
        }
        attrs.insert(
            "declared_tool_count".to_string(),
            json!(metadata.tools.len()),
        );
    }

    (Value::Object(source), Value::Object(attrs))
}
