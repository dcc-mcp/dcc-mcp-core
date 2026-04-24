//! MCP Prompts primitive (issues #351, #355).
//!
//! Exposes skill-authored prompt templates and workflow-derived prompts over
//! the MCP 2025-03-26 `prompts/list` + `prompts/get` JSON-RPC methods. The
//! capability is advertised only when [`crate::McpHttpConfig::enable_prompts`]
//! is `true`.
//!
//! # Sibling-file pattern (issue #356)
//!
//! Prompts live **outside** `SKILL.md`. A skill opts in by setting
//! `metadata.dcc-mcp.prompts: prompts.yaml` in its SKILL.md frontmatter;
//! `prompts.yaml` is a sibling file describing zero or more prompts and,
//! optionally, a list of workflows whose execution summary should be
//! rendered as an auto-generated prompt.
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

#[path = "prompts_spec.rs"]
mod spec;

#[path = "prompts_template.rs"]
mod template;

#[path = "prompts_loader.rs"]
mod loader;

#[cfg(test)]
#[path = "prompts_tests.rs"]
mod tests;

pub use loader::{PromptEntry, PromptSource};
pub use spec::{
    PromptArgumentSpec, PromptError, PromptResult, PromptSpec, PromptsSpec, WorkflowPromptRef,
};
pub use template::render_template;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::RwLock;

use self::loader::load_prompts_from_reference;
use crate::protocol::{GetPromptResult, McpPrompt, McpPromptContent, McpPromptMessage};

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
    /// Names of the loaded skills this cache was built from. Used to
    /// short-circuit rebuilds — swapping this out atomically invalidates.
    loaded_skills: HashSet<String>,
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
    pub fn invalidate(&self) {
        let mut g = self.inner.write();
        g.entries.clear();
        g.loaded_skills.clear();
    }

    /// List every prompt known to the registry.
    pub fn list<F>(&self, walk_loaded: F) -> Vec<McpPrompt>
    where
        F: FnOnce(&mut dyn FnMut(&dcc_mcp_models::SkillMetadata)),
    {
        if !self.is_enabled() {
            return Vec::new();
        }
        self.refresh_if_needed(walk_loaded);
        self.inner
            .read()
            .entries
            .values()
            .map(PromptEntry::to_mcp)
            .collect()
    }

    /// Look up + render a single prompt.
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
            g.entries
                .values()
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
            if g.loaded_skills == loaded_now && !g.entries.is_empty() {
                return;
            }
            // Empty cache + empty loaded set → still fine, mark set.
            if g.loaded_skills == loaded_now && loaded_now.is_empty() {
                return;
            }
        }

        // Slow path — rebuild.
        let mut new_entries: BTreeMap<(String, String), PromptEntry> = BTreeMap::new();
        for md in &metadatas {
            if let Some(rel) = md.prompts_file.as_deref() {
                let skill_path = PathBuf::from(&md.skill_path);
                for entry in load_prompts_from_reference(&skill_path, rel, &md.name) {
                    new_entries.insert((md.name.clone(), entry.name.clone()), entry);
                }
            }
        }
        let mut g = self.inner.write();
        g.entries = new_entries;
        g.loaded_skills = loaded_now;
    }
}
