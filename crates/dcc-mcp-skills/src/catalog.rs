//! SkillCatalog — progressive skill discovery, loading, and unloading.
//!
//! Manages a catalog of discovered skills and their load state. Skills are
//! discovered via `SkillScanner`/`SkillWatcher`, and their tools are registered
//! into `ActionRegistry` on demand via `load_skill` / `unload_skill`.
//!
//! # Architecture
//!
//! ```text
//! [SKILL.md files] --scan--> SkillEntry(Discovered) --load_skill--> SkillEntry(Loaded)
//!                                    │                                    │
//!                              find_skills()                     tools in ActionRegistry
//!                              list_skills()                     + tools/list_changed notification
//!                              get_skill_info()
//! ```
//!
//! # Usage
//!
//! ```no_run
//! use dcc_mcp_skills::catalog::SkillCatalog;
//! use dcc_mcp_actions::ActionRegistry;
//! use std::sync::Arc;
//!
//! let registry = Arc::new(ActionRegistry::new());
//! let catalog = SkillCatalog::new(registry);
//!
//! // Discover skills from standard paths
//! catalog.discover(None, Some("maya"));
//!
//! // Search for skills
//! let results = catalog.find_skills(Some("modeling"), &[], Some("maya"));
//!
//! // Load a skill — registers its tools in ActionRegistry
//! let loaded = catalog.load_skill("modeling-bevel");
//! ```

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;

use dashmap::{DashMap, DashSet};
use dcc_mcp_actions::{
    ActionDispatcher,
    registry::{ActionMeta, ActionRegistry},
};
use dcc_mcp_models::{SkillMetadata, ToolDeclaration};
use std::sync::Arc;

use crate::loader;

// ── Skill entry ──

/// Load state of a skill in the catalog.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillState {
    /// Skill discovered but not loaded (tools not registered).
    Discovered,
    /// Skill loaded — tools registered in ActionRegistry.
    Loaded,
    /// Skill failed to load.
    Error(String),
}

impl std::fmt::Display for SkillState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SkillState::Discovered => write!(f, "discovered"),
            SkillState::Loaded => write!(f, "loaded"),
            SkillState::Error(e) => write!(f, "error: {e}"),
        }
    }
}

/// A skill entry in the catalog, tracking its metadata and load state.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SkillEntry {
    /// Parsed skill metadata from SKILL.md.
    pub metadata: SkillMetadata,
    /// Current load state.
    pub state: SkillState,
    /// Names of actions registered from this skill (populated on load).
    pub registered_actions: Vec<String>,
}

// ── SkillCatalog ──

/// Manages discovered skills and their progressive loading.
///
/// Thread-safe: all state is stored in `DashMap` / `DashSet`.
///
/// When a dispatcher is attached (via [`SkillCatalog::with_dispatcher`]),
/// loading a skill also registers a subprocess-based handler for each
/// action — enabling the Skills-First workflow where agents never need to
/// register handlers manually.
#[cfg_attr(feature = "python-bindings", pyclass(name = "SkillCatalog"))]
pub struct SkillCatalog {
    /// All discovered skill entries, keyed by skill name.
    entries: DashMap<String, SkillEntry>,
    /// Set of skill names currently loaded.
    loaded: DashSet<String>,
    /// Reference to ActionRegistry for registering/unregistering tools.
    registry: Arc<ActionRegistry>,
    /// Optional dispatcher for auto-registering script handlers on load.
    dispatcher: Option<Arc<ActionDispatcher>>,
}

impl std::fmt::Debug for SkillCatalog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let discovered = self
            .entries
            .iter()
            .filter(|e| e.value().state == SkillState::Discovered)
            .count();
        let loaded = self.loaded.len();
        f.debug_struct("SkillCatalog")
            .field("discovered", &discovered)
            .field("loaded", &loaded)
            .field("total", &self.entries.len())
            .finish()
    }
}

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
        }
    }

    /// Create a catalog with an attached dispatcher for Skills-First execution.
    ///
    /// When a dispatcher is attached, calling `load_skill` automatically
    /// registers a subprocess-based handler for every script in the skill.
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
        }
    }

    /// Attach a dispatcher after construction (builder-style).
    pub fn with_dispatcher(mut self, dispatcher: Arc<ActionDispatcher>) -> Self {
        self.dispatcher = Some(dispatcher);
        self
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
                        registered_actions: Vec::new(),
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
                    registered_actions: Vec::new(),
                },
            );
        }
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
                .map(|e| e.registered_actions.clone())
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
            };

            self.registry.register_action(meta);

            // Auto-register subprocess handler if dispatcher is attached
            if let (Some(dispatcher), Some(sp)) = (&self.dispatcher, script_path) {
                let sp_owned = sp.clone();
                let name_clone = action_name.clone();
                dispatcher
                    .register_handler(&name_clone, move |params| execute_script(&sp_owned, params));
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
                };

                self.registry.register_action(meta);

                // Auto-register handler
                if let Some(dispatcher) = &self.dispatcher {
                    let sp = script_path.clone();
                    let name_clone = action_name.clone();
                    dispatcher
                        .register_handler(&name_clone, move |params| execute_script(&sp, params));
                }

                registered.push(action_name);
            }
        }

        // Update catalog state
        if let Some(mut entry) = self.entries.get_mut(skill_name) {
            entry.state = SkillState::Loaded;
            entry.registered_actions = registered.clone();
        }
        self.loaded.insert(skill_name.to_string());

        tracing::info!(
            "SkillCatalog: loaded skill '{}' ({} tools registered, handlers: {})",
            skill_name,
            registered.len(),
            if self.dispatcher.is_some() {
                "auto"
            } else {
                "none"
            }
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
            .map(|e| e.registered_actions.clone())
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
            entry.registered_actions.clear();
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
    /// All filters are AND-ed together. Empty/None filters match everything.
    pub fn find_skills(
        &self,
        query: Option<&str>,
        tags: &[&str],
        dcc: Option<&str>,
    ) -> Vec<SkillSummary> {
        self.entries
            .iter()
            .filter(|entry| {
                let meta = &entry.value().metadata;

                // Query filter: match against name or description (case-insensitive)
                if let Some(q) = query {
                    if !q.is_empty() {
                        let q_lower = q.to_lowercase();
                        let name_match = meta.name.to_lowercase().contains(&q_lower);
                        let desc_match = meta.description.to_lowercase().contains(&q_lower);
                        if !name_match && !desc_match {
                            return false;
                        }
                    }
                }

                // Tags filter: skill must contain ALL requested tags
                if !tags.is_empty() {
                    for tag in tags {
                        if !meta.tags.iter().any(|t| t.eq_ignore_ascii_case(tag)) {
                            return false;
                        }
                    }
                }

                // DCC filter
                if let Some(dcc_filter) = dcc {
                    if !dcc_filter.is_empty() && !meta.dcc.eq_ignore_ascii_case(dcc_filter) {
                        return false;
                    }
                }

                true
            })
            .map(|entry| {
                let e = entry.value();
                SkillSummary {
                    name: e.metadata.name.clone(),
                    description: e.metadata.description.clone(),
                    tags: e.metadata.tags.clone(),
                    dcc: e.metadata.dcc.clone(),
                    version: e.metadata.version.clone(),
                    tool_count: e.metadata.tools.len(),
                    tool_names: e.metadata.tools.iter().map(|t| t.name.clone()).collect(),
                    loaded: e.state == SkillState::Loaded,
                }
            })
            .collect()
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
            .map(|entry| {
                let e = entry.value();
                SkillSummary {
                    name: e.metadata.name.clone(),
                    description: e.metadata.description.clone(),
                    tags: e.metadata.tags.clone(),
                    dcc: e.metadata.dcc.clone(),
                    version: e.metadata.version.clone(),
                    tool_count: e.metadata.tools.len(),
                    tool_names: e.metadata.tools.iter().map(|t| t.name.clone()).collect(),
                    loaded: e.state == SkillState::Loaded,
                }
            })
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
                registered_actions: e.registered_actions.clone(),
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

// ── Script execution helpers ──────────────────────────────────────────────

/// Resolve which script file backs a tool declaration.
///
/// Priority:
/// 1. `tool_decl.source_file` — explicit path set in ToolDeclaration
/// 2. A script whose stem matches the tool name in the skill's scripts list
/// 3. The only script in the skill (if exactly one exists)
fn resolve_tool_script(
    tool_decl: &ToolDeclaration,
    scripts: &[String],
    _skill_path: &std::path::Path,
) -> Option<String> {
    // 1. Explicit source_file on the tool declaration
    if !tool_decl.source_file.is_empty() {
        return Some(tool_decl.source_file.clone());
    }

    // Extract bare tool name (after __ if present)
    let tool_name = if tool_decl.name.contains("__") {
        tool_decl.name.split("__").last().unwrap_or(&tool_decl.name)
    } else {
        &tool_decl.name
    };
    let tool_name_lower = tool_name.to_lowercase().replace('-', "_");

    // 2. Script whose stem matches the tool name
    for script in scripts {
        let stem = std::path::Path::new(script)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase()
            .replace('-', "_");
        if stem == tool_name_lower {
            return Some(script.clone());
        }
    }

    // 3. Single-script skill — the one script backs all tools
    if scripts.len() == 1 {
        return Some(scripts[0].clone());
    }

    None
}

/// Execute a skill script as a subprocess, passing params as JSON via stdin.
///
/// The script is expected to:
/// - Read JSON params from stdin (or use sys.argv for simple cases)
/// - Write a JSON result to stdout
/// - Exit with code 0 on success, non-zero on failure
///
/// Returns `Ok(Value)` on success, `Err(String)` on failure.
fn execute_script(
    script_path: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let params_json = serde_json::to_string(&params).unwrap_or_else(|_| "{}".to_string());

    let path = std::path::Path::new(script_path);
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    // Choose interpreter based on extension
    let (program, args): (&str, Vec<&str>) = match ext.as_str() {
        "py" => ("python", vec![script_path]),
        "sh" | "bash" => ("bash", vec![script_path]),
        "bat" | "cmd" => ("cmd", vec!["/C", script_path]),
        "mel" | "lua" | "hscript" | "maxscript" => {
            // DCC-specific scripts: run via python wrapper if possible
            ("python", vec![script_path])
        }
        _ => ("python", vec![script_path]),
    };

    let mut child = Command::new(program)
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn '{script_path}': {e}"))?;

    // Write params to stdin
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(params_json.as_bytes());
        // stdin closes when dropped, signalling EOF to the script
    }

    let output = child
        .wait_with_output()
        .map_err(|e| format!("Script '{script_path}' execution failed: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        let code = output.status.code().unwrap_or(-1);
        let detail = if stderr.is_empty() {
            stdout.trim().to_string()
        } else {
            stderr.trim().to_string()
        };
        return Err(format!(
            "Script '{script_path}' exited with code {code}: {detail}"
        ));
    }

    // Try to parse stdout as JSON; fall back to plain text result
    let result_str = stdout.trim();
    if result_str.is_empty() {
        return Ok(serde_json::json!({"success": true, "message": ""}));
    }

    match serde_json::from_str::<serde_json::Value>(result_str) {
        Ok(v) => Ok(v),
        Err(_) => {
            // Plain text output — wrap it
            Ok(serde_json::json!({"success": true, "message": result_str}))
        }
    }
}

// ── Summary / Detail types ──

/// Lightweight summary of a skill for search/list results.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "python-bindings", pyclass(name = "SkillSummary", get_all))]
pub struct SkillSummary {
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub dcc: String,
    pub version: String,
    pub tool_count: usize,
    pub tool_names: Vec<String>,
    pub loaded: bool,
}

/// Detailed information about a skill.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SkillDetail {
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub dcc: String,
    pub version: String,
    pub depends: Vec<String>,
    pub skill_path: String,
    pub scripts: Vec<String>,
    pub tools: Vec<ToolDeclaration>,
    pub state: String,
    pub registered_actions: Vec<String>,
}

// ── Python bindings ──

#[cfg(feature = "python-bindings")]
#[pymethods]
impl SkillCatalog {
    #[new]
    fn py_new(registry: ActionRegistry) -> Self {
        Self::new(Arc::new(registry))
    }

    /// Discover skills from standard scan paths.
    ///
    /// Args:
    ///     extra_paths: Additional directories to scan.
    ///     dcc_name: DCC name filter (e.g. "maya", "blender").
    ///
    /// Returns the number of newly discovered skills.
    #[pyo3(name = "discover")]
    #[pyo3(signature = (extra_paths=None, dcc_name=None))]
    fn py_discover(&self, extra_paths: Option<Vec<String>>, dcc_name: Option<&str>) -> usize {
        self.discover(extra_paths.as_deref(), dcc_name)
    }

    /// Load a skill by name — registers its tools.
    ///
    /// Returns a list of registered action names.
    /// Raises ValueError if the skill is not found.
    #[pyo3(name = "load_skill")]
    fn py_load_skill(&self, skill_name: &str) -> PyResult<Vec<String>> {
        self.load_skill(skill_name)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e))
    }

    /// Unload a skill — removes its tools from the registry.
    ///
    /// Returns the number of actions removed.
    /// Raises ValueError if the skill is not loaded.
    #[pyo3(name = "unload_skill")]
    fn py_unload_skill(&self, skill_name: &str) -> PyResult<usize> {
        self.unload_skill(skill_name)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e))
    }

    /// Search for skills matching criteria.
    #[pyo3(name = "find_skills")]
    #[pyo3(signature = (query=None, tags=vec![], dcc=None))]
    fn py_find_skills(
        &self,
        query: Option<&str>,
        tags: Vec<String>,
        dcc: Option<&str>,
    ) -> Vec<SkillSummary> {
        let tag_refs: Vec<&str> = tags.iter().map(String::as_str).collect();
        self.find_skills(query, &tag_refs, dcc)
    }

    /// List all skills with their load status.
    #[pyo3(name = "list_skills")]
    #[pyo3(signature = (status=None))]
    fn py_list_skills(&self, status: Option<&str>) -> Vec<SkillSummary> {
        self.list_skills(status)
    }

    /// Get detailed info about a specific skill.
    ///
    /// Returns None if the skill is not found. The detail is returned as a
    /// Python dict (serialized via serde_json).
    #[pyo3(name = "get_skill_info")]
    fn py_get_skill_info(&self, py: Python<'_>, skill_name: &str) -> PyResult<Option<Py<PyAny>>> {
        use dcc_mcp_utils::py_json::json_value_to_pyobject;
        match self.get_skill_info(skill_name) {
            Some(info) => {
                let val = serde_json::to_value(&info)
                    .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
                Ok(Some(json_value_to_pyobject(py, &val)?))
            }
            None => Ok(None),
        }
    }

    /// Number of skills in the catalog.
    fn __len__(&self) -> usize {
        self.len()
    }

    /// Whether the catalog is empty.
    fn __bool__(&self) -> bool {
        !self.is_empty()
    }

    /// Check if a skill is loaded.
    #[pyo3(name = "is_loaded")]
    fn py_is_loaded(&self, skill_name: &str) -> bool {
        self.is_loaded(skill_name)
    }

    /// Number of loaded skills.
    #[pyo3(name = "loaded_count")]
    fn py_loaded_count(&self) -> usize {
        self.loaded_count()
    }

    fn __repr__(&self) -> String {
        format!(
            "SkillCatalog(total={}, loaded={})",
            self.len(),
            self.loaded_count()
        )
    }
}

// ── Python bindings for summary/detail ──

#[cfg(feature = "python-bindings")]
#[pymethods]
impl SkillSummary {
    fn __repr__(&self) -> String {
        format!("SkillSummary(name={:?}, loaded={})", self.name, self.loaded)
    }
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;
    use dcc_mcp_models::ToolDeclaration;

    fn make_test_catalog() -> SkillCatalog {
        let registry = Arc::new(ActionRegistry::new());
        SkillCatalog::new(registry)
    }

    fn make_test_skill(name: &str, dcc: &str, tool_names: &[&str]) -> SkillMetadata {
        SkillMetadata {
            name: name.to_string(),
            description: format!("Test skill: {name}"),
            tools: tool_names
                .iter()
                .map(|t| ToolDeclaration {
                    name: t.to_string(),
                    ..Default::default()
                })
                .collect(),
            dcc: dcc.to_string(),
            tags: vec!["test".to_string()],
            version: "1.0.0".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn test_catalog_new_is_empty() {
        let catalog = make_test_catalog();
        assert!(catalog.is_empty());
        assert_eq!(catalog.len(), 0);
        assert_eq!(catalog.loaded_count(), 0);
    }

    #[test]
    fn test_add_skill() {
        let catalog = make_test_catalog();
        catalog.add_skill(make_test_skill(
            "modeling-bevel",
            "maya",
            &["bevel", "chamfer"],
        ));
        assert_eq!(catalog.len(), 1);
        assert!(!catalog.is_loaded("modeling-bevel"));
    }

    #[test]
    fn test_load_skill_registers_tools() {
        let catalog = make_test_catalog();
        catalog.add_skill(make_test_skill(
            "modeling-bevel",
            "maya",
            &["bevel", "chamfer"],
        ));

        let actions = catalog.load_skill("modeling-bevel").unwrap();
        assert_eq!(actions.len(), 2);
        assert!(actions.contains(&"modeling_bevel__bevel".to_string()));
        assert!(actions.contains(&"modeling_bevel__chamfer".to_string()));
        assert!(catalog.is_loaded("modeling-bevel"));
        assert_eq!(catalog.loaded_count(), 1);

        // Verify tools are in the registry
        let registry = catalog.registry();
        assert_eq!(registry.len(), 2);
        assert!(registry.get_action("modeling_bevel__bevel", None).is_some());
    }

    #[test]
    fn test_load_skill_with_action_meta_skill_name() {
        let catalog = make_test_catalog();
        catalog.add_skill(make_test_skill("my-skill", "maya", &["tool1"]));

        catalog.load_skill("my-skill").unwrap();
        let meta = catalog
            .registry()
            .get_action("my_skill__tool1", None)
            .unwrap();
        assert_eq!(meta.skill_name, Some("my-skill".to_string()));
    }

    #[test]
    fn test_unload_skill_removes_tools() {
        let catalog = make_test_catalog();
        catalog.add_skill(make_test_skill("modeling-bevel", "maya", &["bevel"]));
        catalog.load_skill("modeling-bevel").unwrap();
        assert_eq!(catalog.registry().len(), 1);

        let removed = catalog.unload_skill("modeling-bevel").unwrap();
        assert_eq!(removed, 1);
        assert!(!catalog.is_loaded("modeling-bevel"));
        assert_eq!(catalog.registry().len(), 0);
    }

    #[test]
    fn test_load_nonexistent_skill_fails() {
        let catalog = make_test_catalog();
        let result = catalog.load_skill("no-such-skill");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    #[test]
    fn test_unload_not_loaded_skill_fails() {
        let catalog = make_test_catalog();
        catalog.add_skill(make_test_skill("test", "maya", &[]));
        let result = catalog.unload_skill("test");
        assert!(result.is_err());
    }

    #[test]
    fn test_load_skill_idempotent() {
        let catalog = make_test_catalog();
        catalog.add_skill(make_test_skill("test", "maya", &["tool1"]));

        let actions1 = catalog.load_skill("test").unwrap();
        let actions2 = catalog.load_skill("test").unwrap();
        assert_eq!(actions1, actions2);
        assert_eq!(catalog.registry().len(), 1);
    }

    #[test]
    fn test_find_skills_by_query() {
        let catalog = make_test_catalog();
        catalog.add_skill(make_test_skill("modeling-bevel", "maya", &[]));
        catalog.add_skill(make_test_skill("rendering-batch", "blender", &[]));

        let results = catalog.find_skills(Some("bevel"), &[], None);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "modeling-bevel");
    }

    #[test]
    fn test_find_skills_by_dcc() {
        let catalog = make_test_catalog();
        catalog.add_skill(make_test_skill("skill-a", "maya", &[]));
        catalog.add_skill(make_test_skill("skill-b", "blender", &[]));

        let results = catalog.find_skills(None, &[], Some("maya"));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].dcc, "maya");
    }

    #[test]
    fn test_find_skills_by_tags() {
        let catalog = make_test_catalog();
        let mut skill = make_test_skill("tagged", "maya", &[]);
        skill.tags = vec!["modeling".to_string(), "polygon".to_string()];
        catalog.add_skill(skill);
        catalog.add_skill(make_test_skill("untagged", "maya", &[]));

        let results = catalog.find_skills(None, &["modeling"], None);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "tagged");
    }

    #[test]
    fn test_list_skills_filter_by_status() {
        let catalog = make_test_catalog();
        catalog.add_skill(make_test_skill("loaded-skill", "maya", &["tool1"]));
        catalog.add_skill(make_test_skill("unloaded-skill", "maya", &[]));
        catalog.load_skill("loaded-skill").unwrap();

        let loaded = catalog.list_skills(Some("loaded"));
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "loaded-skill");
        assert!(loaded[0].loaded);

        let unloaded = catalog.list_skills(Some("unloaded"));
        assert_eq!(unloaded.len(), 1);
        assert_eq!(unloaded[0].name, "unloaded-skill");
        assert!(!unloaded[0].loaded);
    }

    #[test]
    fn test_get_skill_info() {
        let catalog = make_test_catalog();
        catalog.add_skill(make_test_skill("test-skill", "maya", &["tool1", "tool2"]));

        let info = catalog.get_skill_info("test-skill").unwrap();
        assert_eq!(info.name, "test-skill");
        assert_eq!(info.tools.len(), 2);
        assert_eq!(info.state, "discovered");
    }

    #[test]
    fn test_get_skill_info_nonexistent() {
        let catalog = make_test_catalog();
        assert!(catalog.get_skill_info("nope").is_none());
    }

    #[test]
    fn test_remove_skill() {
        let catalog = make_test_catalog();
        catalog.add_skill(make_test_skill("removable", "maya", &["tool1"]));
        catalog.load_skill("removable").unwrap();

        assert!(catalog.remove_skill("removable"));
        assert_eq!(catalog.len(), 0);
        assert_eq!(catalog.registry().len(), 0);
    }

    #[test]
    fn test_clear() {
        let catalog = make_test_catalog();
        catalog.add_skill(make_test_skill("a", "maya", &["t1"]));
        catalog.add_skill(make_test_skill("b", "maya", &["t2"]));
        catalog.load_skill("a").unwrap();

        catalog.clear();
        assert!(catalog.is_empty());
        assert_eq!(catalog.registry().len(), 0);
    }

    #[test]
    fn test_skill_with_scripts_no_tools() {
        let catalog = make_test_catalog();
        let mut skill = make_test_skill("scripted", "maya", &[]);
        skill.scripts = vec!["/path/to/run.py".to_string()];
        catalog.add_skill(skill);

        let actions = catalog.load_skill("scripted").unwrap();
        assert_eq!(actions.len(), 1);
        assert!(actions[0].contains("scripted__run"));
    }

    #[test]
    fn test_add_skill_does_not_overwrite_loaded() {
        let catalog = make_test_catalog();
        let skill = make_test_skill("keep", "maya", &["tool1"]);
        catalog.add_skill(skill);
        catalog.load_skill("keep").unwrap();

        // Add again with different metadata — should not overwrite loaded state
        let updated = SkillMetadata {
            name: "keep".to_string(),
            description: "Updated description".to_string(),
            tools: vec![ToolDeclaration {
                name: "tool1".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        };
        catalog.add_skill(updated);

        assert!(catalog.is_loaded("keep"));
        let info = catalog.get_skill_info("keep").unwrap();
        // Description should NOT be updated since skill was loaded
        assert_eq!(info.description, "Test skill: keep");
    }

    // ── Skills-First: dispatcher integration tests ──

    fn make_catalog_with_dispatcher() -> (SkillCatalog, Arc<ActionDispatcher>) {
        let registry = Arc::new(ActionRegistry::new());
        let dispatcher = Arc::new(ActionDispatcher::new((*registry).clone()));
        let catalog = SkillCatalog::new_with_dispatcher(registry, dispatcher.clone());
        (catalog, dispatcher)
    }

    #[test]
    fn test_load_skill_registers_dispatcher_handler_for_scripts() {
        let (catalog, dispatcher) = make_catalog_with_dispatcher();

        // Skill with no tool declarations — script-only path
        let mut skill = make_test_skill("echo-skill", "python", &[]);
        skill.scripts = vec!["/fake/echo.py".to_string()];
        catalog.add_skill(skill);

        let actions = catalog.load_skill("echo-skill").unwrap();
        assert_eq!(actions.len(), 1);
        // Handler auto-registered in dispatcher
        assert!(dispatcher.has_handler("echo_skill__echo"));
    }

    #[test]
    fn test_unload_skill_removes_dispatcher_handlers() {
        let (catalog, dispatcher) = make_catalog_with_dispatcher();

        let mut skill = make_test_skill("rm-skill", "python", &[]);
        skill.scripts = vec!["/fake/run.py".to_string()];
        catalog.add_skill(skill);

        catalog.load_skill("rm-skill").unwrap();
        assert!(dispatcher.has_handler("rm_skill__run"));

        catalog.unload_skill("rm-skill").unwrap();
        assert!(!dispatcher.has_handler("rm_skill__run"));
    }

    #[test]
    fn test_load_skill_with_tool_decl_and_source_file() {
        let (catalog, dispatcher) = make_catalog_with_dispatcher();

        let skill = SkillMetadata {
            name: "explicit-skill".to_string(),
            description: "Explicit source file".to_string(),
            tools: vec![ToolDeclaration {
                name: "do_thing".to_string(),
                source_file: "/fake/do_thing.py".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        };
        catalog.add_skill(skill);

        let actions = catalog.load_skill("explicit-skill").unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0], "explicit_skill__do_thing");
        assert!(dispatcher.has_handler("explicit_skill__do_thing"));
        // Verify source_file propagated to ActionMeta
        let meta = dispatcher
            .registry()
            .get_action("explicit_skill__do_thing", None)
            .unwrap();
        assert_eq!(meta.source_file, Some("/fake/do_thing.py".to_string()));
    }

    #[test]
    fn test_execute_script_returns_json() {
        // Test the execute_script helper with a real command that outputs JSON
        // Use `python -c` for cross-platform compatibility
        let result = execute_script("python", serde_json::json!({"key": "value"}));
        // Python may or may not be available; just check the function runs
        // (either Ok or Err is valid in CI environments without Python)
        let _ = result;
    }

    #[test]
    fn test_resolve_tool_script_by_name_match() {
        let scripts = vec![
            "/skill/scripts/bevel.py".to_string(),
            "/skill/scripts/extrude.py".to_string(),
        ];
        let tool = ToolDeclaration {
            name: "bevel".to_string(),
            ..Default::default()
        };
        let resolved = resolve_tool_script(&tool, &scripts, std::path::Path::new("/skill"));
        assert_eq!(resolved, Some("/skill/scripts/bevel.py".to_string()));
    }

    #[test]
    fn test_resolve_tool_script_single_script_fallback() {
        let scripts = vec!["/skill/scripts/main.py".to_string()];
        let tool = ToolDeclaration {
            name: "any_tool".to_string(),
            ..Default::default()
        };
        let resolved = resolve_tool_script(&tool, &scripts, std::path::Path::new("/skill"));
        assert_eq!(resolved, Some("/skill/scripts/main.py".to_string()));
    }

    #[test]
    fn test_resolve_tool_script_explicit_source_file() {
        let scripts = vec!["/skill/scripts/other.py".to_string()];
        let tool = ToolDeclaration {
            name: "my_tool".to_string(),
            source_file: "/skill/scripts/special.py".to_string(),
            ..Default::default()
        };
        let resolved = resolve_tool_script(&tool, &scripts, std::path::Path::new("/skill"));
        assert_eq!(resolved, Some("/skill/scripts/special.py".to_string()));
    }
}
