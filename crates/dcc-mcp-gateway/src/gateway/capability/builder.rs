//! Build a per-instance slice of [`CapabilityRecord`] from the raw
//! backend `tools/list` payload.
//!
//! The builder is a **pure function** of the input — no I/O happens
//! here, so it can be unit-tested with synthesised JSON payloads
//! without spinning up a real MCP backend. The REST/MCP fetch side
//! lives in [`super::refresh`].
//!
//! # Wire type relocation (issue #845)
//!
//! [`BuildOutcome`] was migrated to
//! [`dcc_mcp_gateway_core::capability::builder`] so diagnostics and
//! tests can inspect builder output without depending on this
//! crate's gateway-side runtime. The struct is intentionally inert
//! (no methods), so moving it is a clean field-for-field migration.
//! [`BuildInput`] stays here because it borrows
//! `&[dcc_mcp_jsonrpc::McpTool]` — the type contract belongs in the
//! crate that already depends on `dcc-mcp-jsonrpc`.

use serde_json::Value;
use uuid::Uuid;

use dcc_mcp_gateway_core::capability::compute_fingerprint;
use dcc_mcp_gateway_core::naming::{
    decode_skill_tool_name, extract_bare_tool_name, is_core_tool, is_local_tool,
};
use dcc_mcp_jsonrpc::McpTool;

use super::record::{
    CapabilityAnnotations, CapabilityGroupInfo, CapabilityMetadata, CapabilityRecord,
    SCHEMA_AVAILABLE, is_valid_dcc_bucket, tool_slug,
};

pub use dcc_mcp_gateway_core::capability::builder::BuildOutcome;

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

        let (name_skill, _) = extract_skill_and_bare(&tool.name);
        let skill_name = skill_name_from_meta(tool.meta.as_ref()).or(name_skill);
        let callable_id = tool.name.clone();
        let tags = extract_tags(&tool.annotations, tool.meta.as_ref());
        let search_tokens = extract_search_tokens(tool);
        let has_schema =
            has_meaningful_schema(&tool.input_schema) || meta_declares_schema(tool.meta.as_ref());
        let summary = if tool.description.is_empty() && has_schema {
            // Keep the search text non-empty even when the backend
            // omitted the description — the input schema name still
            // gives `search_tools` something to score against.
            format!("{} ({SCHEMA_AVAILABLE})", callable_id)
        } else {
            tool.description.clone()
        };

        let slug = tool_slug(input.dcc_type, &input.instance_id, &callable_id);
        let tool_group = extract_tool_group_from_meta(tool.meta.as_ref());
        let available_groups = extract_available_groups_from_meta(tool.meta.as_ref());
        records.push(
            CapabilityRecord::new(
                slug,
                callable_id.clone(),
                callable_id,
                skill_name,
                &summary,
                tags,
                input.dcc_type.to_string(),
                input.instance_id,
                has_schema,
                true, // loaded: from live backend
                tool_group,
            )
            .with_surface_metadata(
                extract_annotations(&tool.annotations),
                extract_metadata(tool.meta.as_ref()),
            )
            .with_available_groups(available_groups)
            .with_search_tokens(search_tokens),
        );
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
    if dcc_mcp_naming::validate_tool_name(name).is_err() {
        return true;
    }
    // Skill stubs are discovery hints, not addressable actions.
    if name.starts_with("__skill__") || name.contains("__skill__") {
        return true;
    }
    // Gateway-local and skill-management tools are served directly.
    if is_local_tool(name) || is_core_tool(name) {
        return true;
    }
    false
}

/// Pull the `(skill_name, bare_tool)` pair out of a backend tool name.
///
/// Backends publish actions in two forms (see #258 / #307):
///
/// * `<skill>__<action>` — proactive skill namespacing.
/// * `<bare action>` — single-skill instance where the bare name is
///   unique.
///
/// We keep the original name when no skill can be extracted so the
/// slug stays stable across refresh cycles.
fn extract_skill_and_bare(name: &str) -> (Option<String>, String) {
    if let Some((skill, action)) = decode_skill_tool_name(name) {
        return (Some(skill.to_string()), action.to_string());
    }
    // Fall back to a raw split for backends that already register with
    // `<skill>__<action>` but contain characters the namespace decoder
    // conservatively rejects.
    if let Some((skill, action)) = name.split_once("__")
        && !skill.is_empty()
        && !action.is_empty()
    {
        return (Some(skill.to_string()), action.to_string());
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
    if let Some(m) = meta
        && let Some(t) = m.get("dcc.tags").and_then(Value::as_array)
    {
        for v in t {
            if let Some(s) = v.as_str() {
                tags.push(s.to_string());
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

fn meta_declares_schema(meta: Option<&serde_json::Map<String, Value>>) -> bool {
    meta.and_then(|meta| meta.get("dcc"))
        .and_then(Value::as_object)
        .and_then(|dcc| dcc.get("has_schema"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn extract_search_tokens(tool: &McpTool) -> Vec<String> {
    let mut tokens = Vec::new();
    if let Some(meta) = tool.meta.as_ref() {
        tokens.extend(meta_search_values(
            meta,
            &["searchAliases", "search_aliases", "aliases"],
            "alias:",
        ));
        tokens.extend(meta_search_values(
            meta,
            &["searchTokens", "search_tokens"],
            "",
        ));
    }
    tokens.extend(schema_search_tokens(&tool.input_schema));
    tokens
}

fn meta_search_values(
    meta: &serde_json::Map<String, Value>,
    keys: &[&str],
    nested_prefix: &str,
) -> Vec<String> {
    let Some(dcc) = meta.get("dcc").and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for key in keys {
        if let Some(value) = dcc.get(*key) {
            append_search_values(value, nested_prefix, &mut out);
        }
    }
    out
}

fn append_search_values(value: &Value, prefix: &str, out: &mut Vec<String>) {
    match value {
        Value::String(s) => {
            for item in s.split(',') {
                let item = item.trim();
                if !item.is_empty() {
                    out.push(prefixed(prefix, item));
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                if let Some(s) = item.as_str().map(str::trim).filter(|s| !s.is_empty()) {
                    out.push(prefixed(prefix, s));
                }
            }
        }
        _ => {}
    }
}

fn prefixed(prefix: &str, value: &str) -> String {
    if prefix.is_empty()
        || value.starts_with("alias:")
        || value.starts_with("schema:")
        || value.starts_with("required:")
    {
        value.to_string()
    } else {
        format!("{prefix}{value}")
    }
}

fn schema_search_tokens(schema: &Value) -> Vec<String> {
    let mut out = Vec::new();
    collect_schema_search_tokens(schema, 0, &mut out);
    out
}

fn collect_schema_search_tokens(schema: &Value, depth: usize, out: &mut Vec<String>) {
    if depth > 2 || out.len() >= 48 {
        return;
    }
    let Some(obj) = schema.as_object() else {
        return;
    };

    if let Some(required) = obj.get("required").and_then(Value::as_array) {
        for field in required.iter().filter_map(Value::as_str) {
            push_schema_token(out, "required:", field);
        }
    }

    let Some(props) = obj.get("properties").and_then(Value::as_object) else {
        return;
    };
    let mut names: Vec<&String> = props.keys().collect();
    names.sort();
    for name in names {
        push_schema_token(out, "schema:", name);
        let Some(prop) = props.get(name) else {
            continue;
        };
        if let Some(description) = prop.get("description").and_then(Value::as_str) {
            push_schema_token(out, "schema:", &short_description(description));
        }
        collect_schema_search_tokens(prop, depth + 1, out);
        if out.len() >= 48 {
            break;
        }
    }
}

fn push_schema_token(out: &mut Vec<String>, prefix: &str, value: &str) {
    let value = value.trim();
    if value.is_empty() {
        return;
    }
    out.push(prefixed(prefix, value));
}

fn short_description(description: &str) -> String {
    description
        .split_whitespace()
        .take(8)
        .collect::<Vec<_>>()
        .join(" ")
}

fn extract_annotations(
    annotations: &Option<dcc_mcp_jsonrpc::McpToolAnnotations>,
) -> Option<CapabilityAnnotations> {
    annotations.as_ref().map(|ann| CapabilityAnnotations {
        title: ann.title.clone(),
        read_only_hint: ann.read_only_hint,
        destructive_hint: ann.destructive_hint,
        idempotent_hint: ann.idempotent_hint,
        open_world_hint: ann.open_world_hint,
    })
}

fn extract_metadata(meta: Option<&serde_json::Map<String, Value>>) -> Option<CapabilityMetadata> {
    let dcc = meta?.get("dcc").and_then(Value::as_object)?;
    Some(CapabilityMetadata {
        affinity: dcc
            .get("affinity")
            .and_then(Value::as_str)
            .map(str::to_string),
        execution: dcc
            .get("execution")
            .and_then(Value::as_str)
            .map(str::to_string),
        timeout_hint_secs: dcc
            .get("timeoutHintSecs")
            .or_else(|| dcc.get("timeout_hint_secs"))
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
        enforce_thread_affinity: dcc
            .get("enforceThreadAffinity")
            .or_else(|| dcc.get("enforce_thread_affinity"))
            .and_then(Value::as_bool),
        risk: dcc.get("risk").and_then(Value::as_str).map(str::to_string),
        tool_role: dcc
            .get("toolRole")
            .or_else(|| dcc.get("tool_role"))
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

fn skill_name_from_meta(meta: Option<&serde_json::Map<String, Value>>) -> Option<String> {
    meta.and_then(|map| map.get("dcc"))
        .and_then(Value::as_object)
        .and_then(|dcc| {
            dcc.get("skill")
                .or_else(|| dcc.get("skillName"))
                .or_else(|| dcc.get("skill_name"))
        })
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

/// Extract the progressive tool group name this tool belongs to, if any.
fn extract_tool_group_from_meta(meta: Option<&serde_json::Map<String, Value>>) -> Option<String> {
    meta.and_then(|map| map.get("dcc"))
        .and_then(Value::as_object)
        .and_then(|dcc| {
            dcc.get("group")
                .or_else(|| dcc.get("tool_group"))
                .or_else(|| dcc.get("toolGroup"))
        })
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

/// Extract `available_groups` list from the tool's meta, if present.
fn extract_available_groups_from_meta(
    meta: Option<&serde_json::Map<String, Value>>,
) -> Vec<CapabilityGroupInfo> {
    let Some(groups_value) = meta
        .and_then(|map| map.get("dcc").and_then(Value::as_object))
        .and_then(|dcc| dcc.get("available_groups").or_else(|| dcc.get("groups")))
    else {
        return Vec::new();
    };
    serde_json::from_value(groups_value.clone()).unwrap_or_default()
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

    fn tool_with_meta(name: &str, desc: &str, schema: Value, meta: Value) -> McpTool {
        McpTool {
            name: name.to_string(),
            description: desc.to_string(),
            input_schema: schema,
            output_schema: None,
            annotations: None,
            meta: meta.as_object().cloned(),
        }
    }

    #[test]
    fn skips_skill_stubs_and_local_meta_tools() {
        let iid = Uuid::from_u128(1);
        let tools = vec![
            tool("__skill__hello-world", "stub", json!({"type": "object"})),
            tool("lease", "local", json!({"type": "object"})),
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
    fn extracts_skill_from_client_safe_skill_names() {
        let iid = Uuid::from_u128(2);
        let tools = vec![
            tool(
                "maya-animation__set_keyframe",
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
            by_tool["maya-animation__set_keyframe"]
                .skill_name
                .as_deref(),
            Some("maya-animation"),
        );
        assert_eq!(
            by_tool["maya-animation__set_keyframe"].callable_id,
            "maya-animation__set_keyframe",
        );
        assert_eq!(
            by_tool["hello_world__greet"].skill_name.as_deref(),
            Some("hello_world"),
        );
        assert_eq!(
            by_tool["hello_world__greet"].callable_id,
            "hello_world__greet"
        );
        assert_eq!(by_tool["standalone_action"].skill_name, None);
    }

    #[test]
    fn rest_metadata_skill_names_bare_loaded_actions() {
        let iid = Uuid::from_u128(3);
        let tools = vec![tool_with_meta(
            "create_sphere",
            "sphere",
            json!({"type": "object"}),
            json!({"dcc": {"skill": "maya-primitives"}}),
        )];
        let out = build_records_from_backend(BuildInput {
            instance_id: iid,
            dcc_type: "maya",
            backend_tools: &tools,
        });

        assert_eq!(out.records.len(), 1);
        assert_eq!(out.records[0].backend_tool, "create_sphere");
        assert_eq!(
            out.records[0].skill_name.as_deref(),
            Some("maya-primitives")
        );
    }

    #[test]
    fn indexes_aliases_and_schema_tokens_without_serializing_them() {
        let iid = Uuid::from_u128(33);
        let tools = vec![tool_with_meta(
            "photoshop-export__save_document",
            "Save the active Photoshop document.",
            json!({
                "type": "object",
                "properties": {
                    "destination_path": {
                        "type": "string",
                        "description": "Absolute output file path"
                    },
                    "flatten_layers": {"type": "boolean"}
                },
                "required": ["destination_path"]
            }),
            json!({
                "dcc": {
                    "skill": "photoshop-export",
                    "searchAliases": ["write file", "export image"],
                    "searchTokens": ["schema:existing_hint"]
                }
            }),
        )];
        let out = build_records_from_backend(BuildInput {
            instance_id: iid,
            dcc_type: "photoshop",
            backend_tools: &tools,
        });

        let record = &out.records[0];
        assert!(
            record
                .search_tokens
                .contains(&"alias:write file".to_string())
        );
        assert!(
            record
                .search_tokens
                .contains(&"alias:export image".to_string())
        );
        assert!(
            record
                .search_tokens
                .contains(&"required:destination_path".to_string())
        );
        assert!(
            record
                .search_tokens
                .contains(&"schema:destination_path".to_string())
        );
        assert!(
            record
                .search_tokens
                .contains(&"schema:existing_hint".to_string())
        );

        let serialized = serde_json::to_value(record).unwrap();
        assert!(
            serialized.get("search_tokens").is_none(),
            "search-only tokens must not become public gateway search fields"
        );
    }

    #[test]
    fn fingerprint_changes_when_search_tokens_change() {
        let iid = Uuid::from_u128(34);
        let schema = json!({"type": "object"});
        let before = vec![tool_with_meta(
            "custom-export__write",
            "Write custom payload",
            schema.clone(),
            json!({"dcc": {"searchAliases": ["old alias"]}}),
        )];
        let after = vec![tool_with_meta(
            "custom-export__write",
            "Write custom payload",
            schema,
            json!({"dcc": {"searchAliases": ["new alias"]}}),
        )];

        let fp_before = build_records_from_backend(BuildInput {
            instance_id: iid,
            dcc_type: "custom",
            backend_tools: &before,
        })
        .fingerprint;
        let fp_after = build_records_from_backend(BuildInput {
            instance_id: iid,
            dcc_type: "custom",
            backend_tools: &after,
        })
        .fingerprint;

        assert_ne!(fp_before, fp_after);
    }

    #[test]
    fn skips_invalid_backend_tool_names() {
        let iid = Uuid::from_u128(22);
        let tools = vec![
            tool(
                "maya-animation.set_keyframe",
                "legacy dotted tool",
                json!({"type": "object"}),
            ),
            tool(
                "maya-animation__set_keyframe",
                "client-safe tool",
                json!({"type": "object"}),
            ),
        ];
        let out = build_records_from_backend(BuildInput {
            instance_id: iid,
            dcc_type: "maya",
            backend_tools: &tools,
        });
        assert_eq!(out.skipped, 1);
        assert_eq!(out.records.len(), 1);
        assert_eq!(out.records[0].backend_tool, "maya-animation__set_keyframe");
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
    fn has_schema_honours_backend_search_marker() {
        let iid = Uuid::from_u128(33);
        let tools = vec![tool_with_meta(
            "deferred_schema",
            "",
            json!({"type": "object", "properties": {}}),
            json!({"dcc": {"has_schema": true}}),
        )];
        let out = build_records_from_backend(BuildInput {
            instance_id: iid,
            dcc_type: "maya",
            backend_tools: &tools,
        });
        assert!(out.records[0].has_schema);
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
        assert_eq!(
            rec.annotations.as_ref().and_then(|ann| ann.read_only_hint),
            Some(true)
        );
        assert_eq!(
            rec.annotations.as_ref().and_then(|ann| ann.idempotent_hint),
            Some(true)
        );
    }

    #[test]
    fn dcc_execution_metadata_surfaces_on_records() {
        let iid = Uuid::from_u128(8);
        let mut t = tool("app_ui__act", "act", json!({"type": "object"}));
        t.meta = Some(
            [(
                "dcc".to_string(),
                json!({
                    "affinity": "any",
                    "execution": "sync",
                    "timeoutHintSecs": 5,
                    "enforceThreadAffinity": false,
                    "risk": "mutation",
                }),
            )]
            .into_iter()
            .collect(),
        );
        let out = build_records_from_backend(BuildInput {
            instance_id: iid,
            dcc_type: "python",
            backend_tools: &[t],
        });
        let meta = out.records[0].metadata.as_ref().expect("metadata");
        assert_eq!(meta.affinity.as_deref(), Some("any"));
        assert_eq!(meta.execution.as_deref(), Some("sync"));
        assert_eq!(meta.timeout_hint_secs, Some(5));
        assert_eq!(meta.risk.as_deref(), Some("mutation"));
    }
}
