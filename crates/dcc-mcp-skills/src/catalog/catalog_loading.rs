use super::*;

impl SkillCatalog {
    /// Load a skill by name — registers its tools into ActionRegistry and,
    /// if a dispatcher is attached, auto-registers script execution handlers.
    pub fn load_skill(&self, skill_name: &str) -> Result<Vec<String>, String> {
        if self.loaded.contains(skill_name) {
            let actions = self
                .entries
                .get(skill_name)
                .map(|entry| entry.registered_tools.clone())
                .unwrap_or_default();
            return Ok(actions);
        }

        let metadata = {
            self.entries
                .get(skill_name)
                .map(|entry| entry.metadata.clone())
                .ok_or_else(|| format!("Skill '{skill_name}' not found in catalog"))
        }?;

        let mut registered = Vec::new();
        let skill_base = metadata.name.replace('-', "_");
        let skill_path = std::path::Path::new(&metadata.skill_path);

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
                enabled: group_default_active(&metadata.groups, &tool_decl.group),
                required_capabilities: tool_decl.required_capabilities.clone(),
                execution: tool_decl.execution,
                timeout_hint_secs: tool_decl.timeout_hint_secs,
                thread_affinity: tool_decl.thread_affinity,
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
                if let Some(executor) = &self.script_executor {
                    let executor = Arc::clone(executor);
                    dispatcher.register_handler(&action_name_clone, move |params| {
                        executor(script_path_owned.clone(), params)
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

                if let Some(dispatcher) = &self.dispatcher {
                    let script_path_owned = script_path.clone();
                    let action_name_clone = action_name.clone();
                    let dcc_owned = metadata.dcc.clone();
                    if let Some(executor) = &self.script_executor {
                        let executor = Arc::clone(executor);
                        dispatcher.register_handler(&action_name_clone, move |params| {
                            executor(script_path_owned.clone(), params)
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
    pub fn unload_skill(&self, skill_name: &str) -> Result<usize, String> {
        if !self.loaded.contains(skill_name) {
            return Err(format!("Skill '{skill_name}' is not loaded"));
        }

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
        self.entries.remove(skill_name).is_some()
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
}
