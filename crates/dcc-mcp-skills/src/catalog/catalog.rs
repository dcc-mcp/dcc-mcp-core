use super::*;
use dcc_mcp_gateway_search::{SearchQuery as RankingQuery, SearchRecord, rank_all};

#[derive(Clone)]
struct SkillSearchRecord<'a> {
    index: usize,
    metadata: &'a SkillMetadata,
    scope: SkillScope,
    path_source: scoring::SkillPathSource,
    search_tokens: Vec<String>,
}

impl SearchRecord for SkillSearchRecord<'_> {
    fn tool_slug(&self) -> &str {
        &self.metadata.name
    }

    fn backend_tool(&self) -> &str {
        &self.metadata.name
    }

    fn summary(&self) -> &str {
        &self.metadata.description
    }

    fn skill_name(&self) -> Option<&str> {
        Some(&self.metadata.name)
    }

    fn tags(&self) -> &[String] {
        &self.metadata.tags
    }

    fn search_tokens(&self) -> &[String] {
        &self.search_tokens
    }

    fn search_aliases(&self) -> &[String] {
        &self.metadata.search_aliases
    }

    fn dcc_type(&self) -> &str {
        &self.metadata.dcc
    }

    fn instance_id(&self) -> uuid::Uuid {
        uuid::Uuid::nil()
    }

    fn loaded(&self) -> bool {
        true
    }

    fn rank_layer(&self) -> Option<&str> {
        self.metadata.layer.as_deref()
    }

    fn rank_path_source(&self) -> Option<&str> {
        Some(self.path_source.label())
    }

    fn rank_scope(&self) -> u8 {
        self.scope as u8
    }
}

fn ranking_tokens(metadata: &SkillMetadata, fields: &scoring::FieldTokens) -> Vec<String> {
    fields
        .hint
        .iter()
        .chain(&fields.tool_names)
        .chain(&fields.tool_aliases)
        .chain(&fields.tool_descriptions)
        .cloned()
        .chain(std::iter::once(metadata.dcc.clone()))
        .collect()
}

#[path = "catalog_discovery.rs"]
mod discovery_impl;
#[path = "catalog_loading.rs"]
mod loading_impl;

impl SkillCatalog {
    /// Unified skill discovery (issue #340).
    ///
    /// Behaviour:
    /// - `query` / `tags` / `dcc` are AND-ed through the shared fuzzy scorer that
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
        // ── 0. dcc shard fast-path (PIP-2470) ──
        //
        // When `dcc` is specified, use the per-dcc shard to narrow the
        // entry scan to only skills in the matching shard.  If the shard
        // doesn't exist (no skills for that dcc), return early.
        let dcc_key = dcc
            .filter(|d| !d.is_empty())
            .map(|d| d.to_ascii_lowercase());

        let q_trim = query.map(str::trim).unwrap_or("");
        let shard = match &dcc_key {
            Some(key) => match self.dcc_shards.get(key) {
                Some(shard) => Some(shard),
                None => return Vec::new(),
            },
            None => None,
        };

        let matches_filters = |name: &str, meta: &SkillMetadata| {
            if shard.as_ref().is_some_and(|names| !names.contains(name)) {
                return false;
            }
            !tags.iter().any(|tag| {
                !meta
                    .tags
                    .iter()
                    .any(|value| value.eq_ignore_ascii_case(tag))
            })
        };

        // ── 1. No query → project summaries directly, then sort ──
        if q_trim.is_empty() {
            let mut summaries: Vec<_> = self
                .entries
                .iter()
                .filter(|entry| matches_filters(entry.key(), &entry.value().metadata))
                .filter(|entry| scope.is_none_or(|value| value == entry.value().scope))
                .map(|entry| {
                    (
                        entry.value().scope,
                        helpers::skill_entry_to_summary(entry.value()),
                    )
                })
                .collect();
            summaries.sort_by(|(scope_a, summary_a), (scope_b, summary_b)| {
                scope_b
                    .cmp(scope_a)
                    .then_with(|| summary_a.name.cmp(&summary_b.name))
            });
            return summaries
                .into_iter()
                .map(|(_, summary)| summary)
                .take(limit.unwrap_or(usize::MAX))
                .collect();
        }

        // ── 2. Pre-filter by tags/dcc ──
        // Fuzzy matching cannot safely use the exact-token inverted index:
        // pruning first would discard the typo/prefix candidates that the
        // shared scorer is specifically responsible for recalling.
        let mut prefiltered = Vec::new();
        for entry in self.entries.iter() {
            let meta = &entry.value().metadata;
            if !matches_filters(entry.key(), meta) {
                continue;
            }
            // Retain the DashMap guard instead of deep-cloning SkillEntry.
            prefiltered.push(entry);
        }

        // ── 3. Shared scoring (with central rank policy, #1398/#2184) ──
        //
        // When the caller filters by a known layer name through `tags`,
        // they have explicitly asked to browse that layer — bypass the
        // policy so the raw scorer order is honoured inside the filtered
        // slice. Otherwise apply the penalty so infrastructure / example
        // skills behave as the documented "fallback" tier.
        let layer_filter_explicit = tags.iter().any(|t| {
            let l = t.to_ascii_lowercase();
            matches!(
                l.as_str(),
                scoring::LAYER_DOMAIN
                    | scoring::LAYER_THIN_HARNESS
                    | scoring::LAYER_INFRASTRUCTURE
                    | scoring::LAYER_EXAMPLE
            )
        });

        let records: Vec<SkillSearchRecord<'_>> = prefiltered
            .iter()
            .enumerate()
            .map(|(index, entry)| SkillSearchRecord {
                index,
                metadata: &entry.value().metadata,
                scope: entry.value().scope,
                path_source: entry.value().path_source,
                search_tokens: ranking_tokens(&entry.value().metadata, &entry.value().field_tokens),
            })
            .collect();
        let ranked: Vec<SkillSummary> = rank_all(
            &records,
            &RankingQuery {
                query: q_trim.to_string(),
                tags: if layer_filter_explicit {
                    tags.iter().map(|tag| (*tag).to_string()).collect()
                } else {
                    Vec::new()
                },
                ..RankingQuery::default()
            },
        )
        .into_iter()
        .map(|hit| helpers::skill_entry_to_summary(prefiltered[hit.record.index].value()))
        .collect();

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
                    Some("unloaded") => !matches!(state, SkillState::Loaded | SkillState::Error(_)),
                    Some("discovered") => state == &SkillState::Discovered,
                    Some("pending_deps") => matches!(state, SkillState::PendingDeps { .. }),
                    Some("error") => matches!(state, SkillState::Error(_)),
                    _ => true, // "all" or None
                }
            })
            .map(|entry| helpers::skill_entry_to_summary(entry.value()))
            .collect()
    }

    /// Return safe diagnostics for skill directories that were scanned but
    /// rejected by the loader.
    pub fn skipped_skill_diagnostics(
        &self,
        query: Option<&str>,
        limit: Option<usize>,
    ) -> Vec<SkippedSkillDiagnostic> {
        let mut diagnostics: Vec<SkippedSkillDiagnostic> = self
            .skipped
            .iter()
            .filter(|entry| entry.value().matches_query(query))
            .map(|entry| entry.value().clone())
            .collect();
        diagnostics.sort_by(|a, b| a.skill_name.cmp(&b.skill_name));
        if let Some(limit) = limit {
            diagnostics.truncate(limit);
        }
        diagnostics
    }

    /// Return a skipped diagnostic for an exact skill name or directory name.
    pub fn skipped_skill_diagnostic(&self, skill_name: &str) -> Option<SkippedSkillDiagnostic> {
        if let Some(entry) = self.skipped.get(skill_name) {
            return Some(entry.value().clone());
        }
        let wanted = skill_name.trim();
        self.skipped
            .iter()
            .find(|entry| entry.directory_name.eq_ignore_ascii_case(wanted))
            .map(|entry| entry.value().clone())
    }

    /// Number of skipped skill diagnostics currently retained by the catalog.
    #[must_use]
    pub fn skipped_count(&self) -> usize {
        self.skipped.len()
    }

    /// Get detailed information about a specific skill.
    pub fn get_skill_info(&self, skill_name: &str) -> Option<SkillDetail> {
        self.entries.get(skill_name).map(|entry| {
            let e = entry.value();
            let runtimes = dcc_mcp_models::resolve_runtime_reports(&e.metadata.runtimes);
            let runtime = (!runtimes.is_empty())
                .then(|| dcc_mcp_models::summarize_runtime_reports(&runtimes));
            let skill_md_path = (!e.metadata.skill_path.is_empty()).then(|| {
                std::path::Path::new(&e.metadata.skill_path)
                    .join(crate::constants::SKILL_METADATA_FILE)
            });
            let markdown = skill_md_path
                .as_ref()
                .and_then(|path| std::fs::read_to_string(path).ok());
            SkillDetail {
                name: e.metadata.name.clone(),
                description: e.metadata.description.clone(),
                tags: e.metadata.tags.clone(),
                search_aliases: e.metadata.search_aliases.clone(),
                dcc: e.metadata.dcc.clone(),
                version: e.metadata.version.clone(),
                depends: e.metadata.depends.clone(),
                skill_path: e.metadata.skill_path.clone(),
                skill_md_path: skill_md_path.map(|path| path.to_string_lossy().to_string()),
                markdown,
                scripts: e.metadata.scripts.clone(),
                tools: e.metadata.tools.clone(),
                groups: e.metadata.groups.clone(),
                state: e.state.to_string(),
                missing_dependencies: e.state.missing_dependencies(),
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
                runtimes,
                runtime,
            }
        })
    }

    /// Get a mutable-by-copy skill metadata object for adapter-side policy changes.
    ///
    /// The returned [`SkillMetadata`] is detached from the catalog. Mutating it
    /// does not affect discovery state or registered tools until the caller
    /// passes it back through [`load_skill_object`](Self::load_skill_object).
    pub fn get_skill(&self, skill_name: &str) -> Option<SkillMetadata> {
        self.entries
            .get(skill_name)
            .map(|entry| entry.metadata.clone())
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

    /// Get a reference to the underlying ToolRegistry.
    pub fn registry(&self) -> &Arc<ToolRegistry> {
        &self.registry
    }

    /// Get a reference to the attached dispatcher, if any.
    pub fn dispatcher(&self) -> Option<&Arc<ToolDispatcher>> {
        self.dispatcher.as_ref()
    }

    /// Return the in-process lifecycle event bus.
    #[must_use]
    pub fn event_bus(&self) -> EventBus {
        self.event_bus.clone()
    }
}
