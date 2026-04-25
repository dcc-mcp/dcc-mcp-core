use super::*;

#[path = "catalog_discovery.rs"]
mod discovery_impl;
#[path = "catalog_loading.rs"]
mod loading_impl;

impl SkillCatalog {
    /// Unified skill discovery (issue #340).
    ///
    /// Behaviour:
    /// - `query` / `tags` / `dcc` are AND-ed through a BM25-lite scorer that
    ///   tokenises name, tags, search_hint, description, sibling `tools.yaml`
    ///   entries (tool names + descriptions) and `dcc`.
    ///   See [`scoring`] for weights, tie-breaks and the exact-name fast path.
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
        // ── 1. Pre-filter by tags/dcc (AND semantics) ──
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
        let ranked: Vec<SkillSummary> = if q_trim.is_empty() {
            prefiltered.sort_by(|a, b| {
                b.scope
                    .cmp(&a.scope)
                    .then_with(|| a.metadata.name.cmp(&b.metadata.name))
            });
            prefiltered
                .iter()
                .map(helpers::skill_entry_to_summary)
                .collect()
        } else {
            // ── 3. BM25-lite scoring ──
            let metas: Vec<&SkillMetadata> = prefiltered.iter().map(|e| &e.metadata).collect();
            let scopes: Vec<SkillScope> = prefiltered.iter().map(|e| e.scope).collect();
            let scored = scoring::score_skills(q_trim, &metas, &scopes);
            scored
                .into_iter()
                .map(|s| helpers::skill_entry_to_summary(&prefiltered[s.index]))
                .collect()
        };

        // ── 4. Scope filter (post-ranking) ──
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

        // ── 5. Limit ──
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

    /// Get a reference to the underlying ActionRegistry.
    pub fn registry(&self) -> &Arc<ActionRegistry> {
        &self.registry
    }

    /// Get a reference to the attached dispatcher, if any.
    pub fn dispatcher(&self) -> Option<&Arc<ActionDispatcher>> {
        self.dispatcher.as_ref()
    }
}
