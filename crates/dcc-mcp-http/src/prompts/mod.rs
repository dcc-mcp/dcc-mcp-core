//! MCP Prompts primitive (issues #351, #355).
//!
//! Exposes skill-authored prompt templates and workflow-derived prompts over
//! the MCP 2025-03-26 `prompts/list` + `prompts/get` JSON-RPC methods. The
//! capability is advertised only when [`crate::McpHttpConfig::enable_prompts`]
//! is `true`.
//!
//! # Sibling-file pattern (issue #356)
//!
//! Prompts live **outside** `SKILL.md`. A skill can opt in explicitly by
//! setting `metadata.dcc-mcp.prompts: prompts.yaml` in its SKILL.md
//! frontmatter; `prompts.yaml` is a sibling file describing zero or more
//! prompts and, optionally, a list of workflows whose execution summary should
//! be rendered as an auto-generated prompt. If no explicit prompts file is
//! declared, the registry can derive fallback prompts from
//! `metadata.dcc-mcp.examples`, `metadata.dcc-mcp.recipes`, or
//! `metadata.dcc-mcp.workflows`.
//!
//! ```yaml
//! # my-skill/prompts.yaml
//! prompts:
//!   - name: bake_and_export_animation
//!     description: Guide for baking a rig and exporting FBX
//!     arguments:
//!       - name: frame_range
//!         required: true
//!       - name: output_path
//!         required: true
//!     template: |
//!       Please bake the animation on the selected rig across frames
//!       {{frame_range}}. Then export to FBX at {{output_path}}.
//! workflows:
//!   - file: workflows/bake_and_export.yaml
//!     prompt_name: bake_and_export_workflow
//! ```
//!
//! # Template engine
//!
//! A minimal `{{name}}` substitutor (see [`render_template`]). Missing
//! arguments raise [`PromptError::MissingArg`] so `prompts/get` can
//! surface a descriptive JSON-RPC error naming the offending parameter.
//!
//! ## Maintainer layout
//!
//! This module is a **thin facade** that keeps `PromptRegistry` and
//! re-exports the public surface. Implementation is split across
//! sibling files:
//!
//! | File | Responsibility |
//! |------|----------------|
//! | `prompts_spec.rs`     | YAML-backed spec types + `PromptError` |
//! | `prompts_template.rs` | `render_template` + placeholder helpers |
//! | `prompts_loader.rs`   | `PromptEntry`, `PromptSource`, sibling-file / glob loader, workflow-derived prompt generator |
//! | `prompts_tests.rs`    | Unit-test suite |

mod loader;
mod spec;
mod template;

#[cfg(test)]
mod tests;

pub use loader::{
    PromptDiagnosticFailure, PromptDiagnosticSource, PromptDiagnostics, PromptEntry, PromptSource,
};
pub use spec::{
    PromptArgumentSpec, PromptError, PromptResult, PromptSpec, PromptsSpec, WorkflowPromptRef,
};
pub use template::render_template;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::RwLock;

use self::loader::{load_derived_prompts_from_skill, load_prompts_from_reference};
use dcc_mcp_jsonrpc::{GetPromptResult, McpPrompt, McpPromptContent, McpPromptMessage};

/// Thread-safe registry of prompts, rebuilt lazily on demand.
///
/// Owned by [`crate::handler::AppState`] via `Arc`. The registry caches the
/// set of loaded skills it last saw; when `prompts/list` or `prompts/get` is
/// called it rescans the [`dcc_mcp_skills::SkillCatalog`] (the heavy work —
/// actually opening and parsing sibling YAML files — happens once per skill
/// per cache invalidation).
#[derive(Clone, Default)]
pub struct PromptRegistry {
    inner: Arc<RwLock<PromptRegistryInner>>,
}

#[derive(Default)]
struct PromptRegistryInner {
    /// Prompts keyed by (skill_name, prompt_name) so duplicate names across
    /// skills don't collide.
    entries: BTreeMap<(String, String), PromptEntry>,
    /// Manually registered prompts (e.g. from Python). These survive
    /// cache invalidations and are merged into `list()` / `get()`.
    manual_entries: BTreeMap<(String, String), PromptEntry>,
    /// Names of the loaded skills this cache was built from. Used to
    /// short-circuit rebuilds — swapping this out atomically invalidates.
    loaded_skills: HashSet<String>,
    cache_initialized: bool,
    diagnostics: PromptDiagnostics,
    enabled: bool,
}

impl PromptRegistry {
    /// Create an empty registry.
    ///
    /// Pass `enabled = false` to disable the whole primitive — `list()` will
    /// always return an empty `Vec` and `get()` will return
    /// [`PromptError::NotFound`] for every name.
    pub fn new(enabled: bool) -> Self {
        Self {
            inner: Arc::new(RwLock::new(PromptRegistryInner {
                enabled,
                ..Default::default()
            })),
        }
    }

    /// Returns `true` when the Prompts primitive is advertised in
    /// `initialize` (mirrors `McpHttpConfig::enable_prompts`).
    pub fn is_enabled(&self) -> bool {
        self.inner.read().enabled
    }

    /// Force a rebuild on next `list` / `get`.
    ///
    /// Called by the server when a skill is loaded or unloaded — the cached
    /// entry set is cleared so the next request rescans all loaded skills.
    /// Manual entries are preserved.
    pub fn invalidate(&self) {
        let mut g = self.inner.write();
        g.entries.clear();
        g.loaded_skills.clear();
        g.cache_initialized = false;
        g.diagnostics = PromptDiagnostics {
            enabled: g.enabled,
            ..Default::default()
        };
    }

    /// Register a prompt manually (e.g. from Python embedding).
    ///
    /// `skill_name` is used as the namespace (use `"manual"` for adapter-
    /// registered prompts). Overwrites any existing entry with the same
    /// `(skill_name, name)` key.
    pub fn register_prompt(&self, skill_name: &str, entry: PromptEntry) {
        let mut g = self.inner.write();
        g.manual_entries
            .insert((skill_name.to_string(), entry.name.clone()), entry);
    }

    /// Remove all manually registered prompts for a given skill namespace.
    pub fn clear_manual_for_skill(&self, skill_name: &str) {
        let mut g = self.inner.write();
        g.manual_entries.retain(|(sn, _), _| sn != skill_name);
    }

    /// Clear every manually registered prompt.
    pub fn clear_all_manual(&self) {
        let mut g = self.inner.write();
        g.manual_entries.clear();
    }

    /// Remove a single manually registered prompt by (skill_name, name).
    pub fn unregister_prompt(&self, skill_name: &str, name: &str) {
        let mut g = self.inner.write();
        g.manual_entries
            .remove(&(skill_name.to_string(), name.to_string()));
    }

    /// List every prompt known to the registry (skill-loaded + manual).
    pub fn list<F>(&self, walk_loaded: F) -> Vec<McpPrompt>
    where
        F: FnOnce(&mut dyn FnMut(&dcc_mcp_models::SkillMetadata)),
    {
        if !self.is_enabled() {
            return Vec::new();
        }
        self.refresh_if_needed(walk_loaded);
        let g = self.inner.read();
        let mut by_name: BTreeMap<String, McpPrompt> = BTreeMap::new();
        for entry in g.entries.values() {
            by_name.insert(entry.name.clone(), entry.to_mcp());
        }
        for entry in g.manual_entries.values() {
            by_name.insert(entry.name.clone(), entry.to_mcp());
        }
        by_name.into_values().collect()
    }

    /// Build a diagnostic snapshot for empty or surprising `prompts/list`
    /// results. This uses the same lazy refresh path as `list()` / `get()`.
    pub fn diagnostics<F>(&self, walk_loaded: F) -> PromptDiagnostics
    where
        F: FnOnce(&mut dyn FnMut(&dcc_mcp_models::SkillMetadata)),
    {
        if !self.is_enabled() {
            return PromptDiagnostics {
                enabled: false,
                notes: vec!["Prompts are disabled by server configuration.".to_string()],
                ..Default::default()
            };
        }
        self.refresh_if_needed(walk_loaded);
        let g = self.inner.read();
        let manual_prompt_count = g.manual_entries.len();
        let mut diagnostics = g.diagnostics.clone();
        diagnostics.enabled = g.enabled;
        diagnostics.manual_prompt_count = manual_prompt_count;
        diagnostics.skill_prompt_count = g.entries.len();
        diagnostics.prompt_count = g.entries.len() + manual_prompt_count;
        if diagnostics.prompt_count == 0 && diagnostics.notes.is_empty() {
            if diagnostics.loaded_skill_count == 0 {
                diagnostics.notes.push(
                    "No skills are currently loaded; load a prompt-capable skill first."
                        .to_string(),
                );
            } else if diagnostics.prompt_capable_skill_count == 0 {
                diagnostics.notes.push(
                    "Loaded skills did not declare metadata.dcc-mcp.prompts, examples, recipes, or workflows."
                        .to_string(),
                );
            } else {
                diagnostics.notes.push(
                    "Prompt-capable skills were loaded, but no prompt entries could be produced; inspect failures."
                        .to_string(),
                );
            }
        }
        diagnostics
    }

    /// Look up + render a single prompt (skill-loaded or manual).
    pub fn get<F>(
        &self,
        name: &str,
        args: &HashMap<String, String>,
        walk_loaded: F,
    ) -> PromptResult<GetPromptResult>
    where
        F: FnOnce(&mut dyn FnMut(&dcc_mcp_models::SkillMetadata)),
    {
        if !self.is_enabled() {
            return Err(PromptError::NotFound(name.to_string()));
        }
        self.refresh_if_needed(walk_loaded);
        // Validate required args present before rendering so we fail fast
        // with a named-parameter error.
        let entry = {
            let g = self.inner.read();
            // Search manual entries first (they are explicit registrations),
            // then skill-loaded entries.
            g.manual_entries
                .values()
                .chain(g.entries.values())
                .find(|e| e.name == name)
                .cloned()
                .ok_or_else(|| PromptError::NotFound(name.to_string()))?
        };
        for arg in &entry.arguments {
            if arg.required && !args.contains_key(&arg.name) {
                return Err(PromptError::MissingArg(arg.name.clone()));
            }
        }
        let rendered = render_template(&entry.template, args)?;
        Ok(GetPromptResult {
            description: entry.description.clone(),
            messages: vec![McpPromptMessage {
                role: "user".to_string(),
                content: McpPromptContent::text(rendered),
            }],
        })
    }

    fn refresh_if_needed<F>(&self, walk_loaded: F)
    where
        F: FnOnce(&mut dyn FnMut(&dcc_mcp_models::SkillMetadata)),
    {
        // Collect loaded skills.
        let mut loaded_now: HashSet<String> = HashSet::new();
        let mut metadatas: Vec<dcc_mcp_models::SkillMetadata> = Vec::new();
        let mut cb = |md: &dcc_mcp_models::SkillMetadata| {
            loaded_now.insert(md.name.clone());
            metadatas.push(md.clone());
        };
        walk_loaded(&mut cb);

        // Fast path — loaded skill set unchanged and cache populated.
        {
            let g = self.inner.read();
            if g.cache_initialized && g.loaded_skills == loaded_now {
                return;
            }
        }

        // Slow path — rebuild.
        let mut new_entries: BTreeMap<(String, String), PromptEntry> = BTreeMap::new();
        let mut diagnostics = PromptDiagnostics {
            enabled: true,
            loaded_skill_count: metadatas.len(),
            ..Default::default()
        };
        for md in &metadatas {
            if let Some(rel) = md.prompts_file.as_deref() {
                let skill_path = PathBuf::from(&md.skill_path);
                diagnostics.explicit_skill_count += 1;
                let report = load_prompts_from_reference(&skill_path, rel, &md.name);
                if report.prompt_capable {
                    diagnostics.prompt_capable_skill_count += 1;
                }
                diagnostics.failures.extend(report.failures);
                for entry in report.entries {
                    diagnostics.sources.push(PromptDiagnosticSource {
                        skill: entry.skill.clone(),
                        prompt: entry.name.clone(),
                        source: entry.source.as_str().to_string(),
                    });
                    new_entries.insert((md.name.clone(), entry.name.clone()), entry);
                }
            } else {
                let report = load_derived_prompts_from_skill(md);
                if report.prompt_capable {
                    diagnostics.prompt_capable_skill_count += 1;
                }
                if !report.entries.is_empty() {
                    diagnostics.derived_skill_count += 1;
                }
                diagnostics.failures.extend(report.failures);
                for entry in report.entries {
                    diagnostics.sources.push(PromptDiagnosticSource {
                        skill: entry.skill.clone(),
                        prompt: entry.name.clone(),
                        source: entry.source.as_str().to_string(),
                    });
                    new_entries.insert((md.name.clone(), entry.name.clone()), entry);
                }
            }
        }
        diagnostics.skill_prompt_count = new_entries.len();
        diagnostics.prompt_count = new_entries.len();
        let mut g = self.inner.write();
        g.entries = new_entries;
        g.loaded_skills = loaded_now;
        g.cache_initialized = true;
        g.diagnostics = diagnostics;
    }
}
