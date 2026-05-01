//! Build a per-instance slice of [`CapabilityRecord`] from the raw
//! backend `tools/list` payload.
//!
//! The builder is a **pure function** of the input — no I/O happens
//! here, so it can be unit-tested with synthesised JSON payloads
//! without spinning up a real MCP backend. The REST/MCP fetch side
//! lives in [`super::refresh`].

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use serde_json::Value;
use uuid::Uuid;

use crate::gateway::namespace::{decode_skill_tool_name, extract_bare_tool_name};
use dcc_mcp_jsonrpc::McpTool;

use super::index::InstanceFingerprint;
use super::record::{CapabilityRecord, SCHEMA_AVAILABLE, is_valid_dcc_bucket, tool_slug};

/// Everything the builder needs from a single live backend to emit
/// one instance-worth of capability records.
///
/// Passing these as a struct keeps the public signature stable as
/// later phases add more lightweight metadata (e.g. skill manifest
/// push from maya#163).
pub struct BuildInput<'a> {
    /// UUID of the owning backend.
    pub instance_id: Uuid,
    /// DCC type bucket (e.g. `"maya"`). Must validate through
    /// [`is_valid_dcc_bucket`]; otherwise every record generated for
    /// this instance is rejected because the slug format would break.
    pub dcc_type: &'a str,
    /// Raw `tools/list` response as an array of [`McpTool`].
    pub backend_tools: &'a [McpTool],
}

/// Output of [`build_records_from_backend`].
#[derive(Debug, Clone, Default)]
pub struct BuildOutcome {
    /// Records ready to be stored in the index. Sorted by `tool_slug`
    /// so the merge inside `CapabilityIndex::snapshot` stays cheap.
    pub records: Vec<CapabilityRecord>,
    /// Stable fingerprint of the input tool list; feed this straight
    /// into [`crate::gateway::capability::CapabilityIndex::upsert_instance`].
    pub fingerprint: InstanceFingerprint,
    /// Number of input tools rejected (e.g. missing name, skill stub
    /// filtered out). Diagnostics-only.
    pub skipped: usize,
}

/// Build the instance slice from a backend `tools/list` response.
///
/// Filtering rules:
///
/// 1. **Skill stubs** (`__skill__*`) are skipped — they describe
///    loadable skills, not addressable actions, and belong to the
///    existing `list_skills` surface instead.
/// 2. **Gateway meta-tools** and **skill-management tools** are
///    skipped — those are always served directly by the gateway, so
///    forwarding them through the capability index would double up
///    the surface.
/// 3. Tools whose names are empty strings are skipped defensively
///    (the backend should never produce these, but the index keeps
///    the guarantee that every record has a non-empty slug).
pub fn build_records_from_backend(input: BuildInput<'_>) -> BuildOutcome {
    if !is_valid_dcc_bucket(input.dcc_type) {
        tracing::warn!(
            dcc = input.dcc_type,
            instance = %input.instance_id,
            "capability index: refusing to build records for DCC bucket that is not cursor-safe",
        );
        return BuildOutcome::default();
    }

    let mut records = Vec::with_capacity(input.backend_tools.len());
    let mut skipped = 0usize;

    for tool in input.backend_tools {
        if should_skip(&tool.name) {
            skipped += 1;
            continue;
        }

        let (skill_name, backend_tool) = extract_skill_and_bare(&tool.name);
        let tags = extract_tags(&tool.annotations, tool.meta.as_ref());
        let has_schema = has_meaningful_schema(&tool.input_schema);
        let summary = if tool.description.is_empty() && has_schema {
            // Keep the search text non-empty even when the backend
            // omitted the description — the input schema name still
            // gives `search_tools` something to score against.
            format!("{} ({SCHEMA_AVAILABLE})", backend_tool)
        } else {
            tool.description.clone()
        };

        let slug = tool_slug(input.dcc_type, &input.instance_id, &backend_tool);
        records.push(CapabilityRecord::new(
            slug,
            backend_tool,
            skill_name,
            &summary,
            tags,
            input.dcc_type.to_string(),
            input.instance_id,
            has_schema,
        ));
    }

    // Sort by slug so the per-instance slice is deterministic. The
    // index relies on this ordering to keep snapshots stable across
    // otherwise-identical refresh cycles.
    records.sort_by(|a, b| a.tool_slug.cmp(&b.tool_slug));

    let fingerprint = compute_fingerprint(&records);
    BuildOutcome {
        records,
        fingerprint,
        skipped,
    }
}

/// Names the index never carries. Centralising the predicate here
/// keeps the refresh path and the tests in lock-step.
fn should_skip(name: &str) -> bool {
    if name.is_empty() {
        return true;
    }
    // Skill stubs are discovery hints, not addressable actions.
    if name.starts_with("__skill__") || name.contains(".__skill__") {
        return true;
    }
    // Gateway-local and skill-management tools are served directly.
    if crate::gateway::namespace::is_local_tool(name)
        || crate::gateway::namespace::is_core_tool(name)
    {
        return true;
    }
    false
}

/// Pull the `(skill_name, bare_tool)` pair out of a backend tool name.
///
/// Backends publish actions in two forms (see #258 / #307):
///
/// * `<skill>.<action>` — proactive skill namespacing.
/// * `<bare action>` — single-skill instance where the bare name is
///   unique.
///
/// We keep the original name when no skill can be extracted so the
/// slug stays stable across refresh cycles.
fn extract_skill_and_bare(name: &str) -> (Option<String>, String) {
    if let Some((skill, action)) = decode_skill_tool_name(name) {
        return (Some(skill.to_string()), action.to_string());
    }
    // Fall back to the double-underscore convention used internally
    // (`<skill>__<action>`) for backends that register with the
    // underscore form — we still want the skill dimension available
    // for search ranking.
    if let Some((skill, action)) = name.split_once("__") {
        if !skill.is_empty() && !action.is_empty() {
            return (Some(skill.to_string()), action.to_string());
        }
    }
    (None, extract_bare_tool_name("", name).to_string())
}

/// Pull search-friendly tags out of `annotations.title` and the
/// `_meta` map. Tags are intentionally additive — anything the
/// backend populates becomes search-visible without needing a schema
/// change.
fn extract_tags(
    annotations: &Option<dcc_mcp_jsonrpc::McpToolAnnotations>,
    meta: Option<&serde_json::Map<String, Value>>,
) -> Vec<String> {
    let mut tags: Vec<String> = Vec::new();
    if let Some(ann) = annotations {
        if let Some(title) = ann.title.as_deref() {
            tags.extend(tokenise(title));
        }
        if ann.read_only_hint == Some(true) {
            tags.push("read-only".to_string());
        }
        if ann.idempotent_hint == Some(true) {
            tags.push("idempotent".to_string());
        }
        if ann.destructive_hint == Some(true) {
            tags.push("destructive".to_string());
        }
    }
    if let Some(m) = meta {
        if let Some(t) = m.get("dcc.tags").and_then(Value::as_array) {
            for v in t {
                if let Some(s) = v.as_str() {
                    tags.push(s.to_string());
                }
            }
        }
    }
    // De-duplicate while preserving order so the wire representation
    // stays small and search scoring does not double-count.
    let mut seen = std::collections::HashSet::new();
    tags.retain(|t| seen.insert(t.clone()));
    tags
}

fn tokenise(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty() && s.len() > 2)
        .map(|s| s.to_ascii_lowercase())
        .collect()
}

fn has_meaningful_schema(schema: &Value) -> bool {
    // A truly schemaless tool emits `{"type": "object", "properties": {}}`.
    // Anything with a non-empty `properties` object or a `required`
    // array deserves the `has_schema` flag.
    let Some(obj) = schema.as_object() else {
        return false;
    };
    let props_ok = obj
        .get("properties")
        .and_then(Value::as_object)
        .is_some_and(|p| !p.is_empty());
    let required_ok = obj
        .get("required")
        .and_then(Value::as_array)
        .is_some_and(|r| !r.is_empty());
    props_ok || required_ok
}

fn compute_fingerprint(records: &[CapabilityRecord]) -> InstanceFingerprint {
    let mut hasher = DefaultHasher::new();
    for r in records {
        r.tool_slug.hash(&mut hasher);
        r.has_schema.hash(&mut hasher);
        r.summary.hash(&mut hasher);
        for t in &r.tags {
            t.hash(&mut hasher);
        }
    }
    InstanceFingerprint(hasher.finish())
}

// Tiny re-import helper removed: `idempotent` tags now consistently
// go through `.to_string()`.

#[cfg(test)]
mod unit_tests {
    use super::*;
    use serde_json::json;

    fn tool(name: &str, desc: &str, schema: Value) -> McpTool {
        McpTool {
            name: name.to_string(),
            description: desc.to_string(),
            input_schema: schema,
            output_schema: None,
            annotations: None,
            meta: None,
        }
    }

    #[test]
    fn skips_skill_stubs_and_local_meta_tools() {
        let iid = Uuid::from_u128(1);
        let tools = vec![
            tool("__skill__hello-world", "stub", json!({"type": "object"})),
            tool("list_dcc_instances", "local", json!({"type": "object"})),
            tool("list_skills", "mgmt", json!({"type": "object"})),
            tool("create_sphere", "make a sphere", json!({"type": "object"})),
        ];
        let out = build_records_from_backend(BuildInput {
            instance_id: iid,
            dcc_type: "maya",
            backend_tools: &tools,
        });
        assert_eq!(out.records.len(), 1);
        assert_eq!(out.skipped, 3);
        assert_eq!(out.records[0].backend_tool, "create_sphere");
    }

    #[test]
    fn extracts_skill_from_dotted_and_underscore_forms() {
        let iid = Uuid::from_u128(2);
        let tools = vec![
            tool(
                "maya-animation.set_keyframe",
                "keyframe",
                json!({"type": "object"}),
            ),
            tool("hello_world__greet", "greet", json!({"type": "object"})),
            tool("standalone_action", "no skill", json!({"type": "object"})),
        ];
        let out = build_records_from_backend(BuildInput {
            instance_id: iid,
            dcc_type: "maya",
            backend_tools: &tools,
        });
        let by_tool: std::collections::HashMap<_, _> = out
            .records
            .iter()
            .map(|r| (r.backend_tool.as_str(), r))
            .collect();
        assert_eq!(
            by_tool["set_keyframe"].skill_name.as_deref(),
            Some("maya-animation"),
        );
        assert_eq!(by_tool["greet"].skill_name.as_deref(), Some("hello_world"),);
        assert_eq!(by_tool["standalone_action"].skill_name, None);
    }

    #[test]
    fn has_schema_reflects_real_input_requirements() {
        let iid = Uuid::from_u128(3);
        let tools = vec![
            tool(
                "needs_radius",
                "",
                json!({"type": "object", "properties": {"radius": {"type": "number"}}, "required": ["radius"]}),
            ),
            tool(
                "optional_radius",
                "",
                json!({"type": "object", "properties": {"radius": {"type": "number"}}}),
            ),
            tool("no_params", "", json!({"type": "object", "properties": {}})),
        ];
        let out = build_records_from_backend(BuildInput {
            instance_id: iid,
            dcc_type: "maya",
            backend_tools: &tools,
        });
        let by_tool: std::collections::HashMap<_, _> = out
            .records
            .iter()
            .map(|r| (r.backend_tool.as_str(), r))
            .collect();
        assert!(by_tool["needs_radius"].has_schema);
        assert!(by_tool["optional_radius"].has_schema);
        assert!(!by_tool["no_params"].has_schema);
    }

    #[test]
    fn fingerprint_is_deterministic_across_calls() {
        let iid = Uuid::from_u128(4);
        let tools = vec![
            tool("create_sphere", "make a sphere", json!({"type": "object"})),
            tool("open", "open a file", json!({"type": "object"})),
        ];
        let a = build_records_from_backend(BuildInput {
            instance_id: iid,
            dcc_type: "maya",
            backend_tools: &tools,
        });
        let b = build_records_from_backend(BuildInput {
            instance_id: iid,
            dcc_type: "maya",
            backend_tools: &tools,
        });
        assert_eq!(a.fingerprint, b.fingerprint);
    }

    #[test]
    fn fingerprint_changes_when_a_tool_is_added() {
        let iid = Uuid::from_u128(5);
        let base = vec![tool("a", "", json!({"type": "object"}))];
        let more = vec![
            tool("a", "", json!({"type": "object"})),
            tool("b", "", json!({"type": "object"})),
        ];
        let fp_a = build_records_from_backend(BuildInput {
            instance_id: iid,
            dcc_type: "maya",
            backend_tools: &base,
        })
        .fingerprint;
        let fp_b = build_records_from_backend(BuildInput {
            instance_id: iid,
            dcc_type: "maya",
            backend_tools: &more,
        })
        .fingerprint;
        assert_ne!(fp_a, fp_b);
    }

    #[test]
    fn invalid_dcc_bucket_yields_empty_outcome() {
        // An empty DCC type or one that contains `.` would produce an
        // ambiguous slug; we refuse to index anything in that case.
        let iid = Uuid::from_u128(6);
        let tools = vec![tool("create_sphere", "", json!({"type": "object"}))];
        let out = build_records_from_backend(BuildInput {
            instance_id: iid,
            dcc_type: "",
            backend_tools: &tools,
        });
        assert!(out.records.is_empty());
        let out = build_records_from_backend(BuildInput {
            instance_id: iid,
            dcc_type: "has.dot",
            backend_tools: &tools,
        });
        assert!(out.records.is_empty());
    }

    #[test]
    fn annotation_hints_surface_as_tags() {
        let iid = Uuid::from_u128(7);
        let mut t = tool("read_scene", "read", json!({"type": "object"}));
        t.annotations = Some(dcc_mcp_jsonrpc::McpToolAnnotations {
            title: Some("Scene Reader".to_string()),
            read_only_hint: Some(true),
            idempotent_hint: Some(true),
            ..Default::default()
        });
        let out = build_records_from_backend(BuildInput {
            instance_id: iid,
            dcc_type: "maya",
            backend_tools: &[t],
        });
        let rec = &out.records[0];
        assert!(rec.tags.iter().any(|t| t == "read-only"));
        assert!(rec.tags.iter().any(|t| t == "idempotent"));
        assert!(rec.tags.iter().any(|t| t == "scene"));
        assert!(rec.tags.iter().any(|t| t == "reader"));
    }
}
