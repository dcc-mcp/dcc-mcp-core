#[cfg(test)]
use super::*;

mod issue_317_tests {
    //! Issues #317 and #344 — `execution` / `timeout_hint_secs` / annotation plumbing.
    use super::*;
    use dcc_mcp_actions::registry::ActionMeta;
    use dcc_mcp_models::{ExecutionMode, ToolAnnotations};

    fn empty_eligible() -> std::collections::HashSet<(String, String)> {
        std::collections::HashSet::new()
    }

    #[test]
    fn sync_action_without_annotations_omits_both_fields() {
        // Issue #344 — tools with no declared annotations omit the spec
        // `annotations` field entirely. `deferred_hint` is a dcc-mcp-core
        // extension that rides in `_meta` (never in the spec `annotations`
        // map) and for a sync tool it is simply absent. Issue #588 added
        // the `_meta.dcc.incompleteSchema` marker, which only surfaces
        // when the author skipped `inputSchema`; declare a real schema
        // here so this test stays focused on the annotations contract.
        let meta = ActionMeta {
            name: "quick".into(),
            description: "Fast".into(),
            execution: ExecutionMode::Sync,
            input_schema: serde_json::json!({"type": "object"}),
            ..Default::default()
        };
        let tool = action_meta_to_mcp_tool(&meta, false, &empty_eligible(), &[]);
        assert!(
            tool.annotations.is_none(),
            "tools without declared annotations must omit the field"
        );
        assert!(tool.meta.is_none(), "sync, no timeout → no _meta");
    }

    #[test]
    fn async_action_surfaces_deferred_hint_in_meta_only() {
        // deferred_hint MUST land in _meta["dcc.deferred_hint"] and NEVER
        // inside the spec `annotations` map (issue #344).
        let meta = ActionMeta {
            name: "render".into(),
            description: "Render".into(),
            execution: ExecutionMode::Async,
            timeout_hint_secs: Some(600),
            ..Default::default()
        };
        let tool = action_meta_to_mcp_tool(&meta, false, &empty_eligible(), &[]);
        let v = serde_json::to_value(&tool).unwrap();

        assert_eq!(
            v.pointer("/_meta/dcc/deferred_hint")
                .and_then(|x| x.as_bool()),
            Some(true),
            "deferred_hint must surface in _meta",
        );
        assert_eq!(
            v.pointer("/_meta/dcc/timeoutHintSecs")
                .and_then(|x| x.as_u64()),
            Some(600),
        );
        assert!(
            v.pointer("/annotations/deferredHint").is_none(),
            "deferredHint must never appear inside spec annotations",
        );
    }

    #[test]
    fn timeout_hint_emitted_even_when_sync() {
        let meta = ActionMeta {
            name: "measured".into(),
            description: "Sync with timeout hint".into(),
            execution: ExecutionMode::Sync,
            timeout_hint_secs: Some(30),
            ..Default::default()
        };
        let tool = action_meta_to_mcp_tool(&meta, false, &empty_eligible(), &[]);
        let m = tool.meta.as_ref().unwrap();
        assert_eq!(
            m.get("dcc")
                .and_then(|v| v.get("timeoutHintSecs"))
                .and_then(|v| v.as_u64()),
            Some(30),
        );
        // No deferred_hint in _meta for sync with no explicit async flag.
        assert!(m.get("dcc").and_then(|v| v.get("deferred_hint")).is_none(),);
    }

    #[test]
    fn declared_annotations_surface_as_camelcase_with_spec_keys_only() {
        // Issue #344 — skill-author-declared annotations surface on
        // `tools/list` with spec-compliant camelCase keys. `deferred_hint`
        // from the declaration is routed into `_meta` and MUST NOT
        // contaminate the spec `annotations` map.
        let meta = ActionMeta {
            name: "delete_keyframes".into(),
            description: "danger".into(),
            execution: ExecutionMode::Sync,
            annotations: ToolAnnotations {
                title: Some("Delete Keyframes".into()),
                read_only_hint: Some(false),
                destructive_hint: Some(true),
                idempotent_hint: Some(true),
                open_world_hint: Some(false),
                deferred_hint: Some(true),
            },
            ..Default::default()
        };
        let tool = action_meta_to_mcp_tool(&meta, false, &empty_eligible(), &[]);
        let v = serde_json::to_value(&tool).unwrap();

        assert_eq!(
            v.pointer("/annotations/destructiveHint")
                .and_then(|x| x.as_bool()),
            Some(true)
        );
        assert_eq!(
            v.pointer("/annotations/readOnlyHint")
                .and_then(|x| x.as_bool()),
            Some(false)
        );
        assert_eq!(
            v.pointer("/annotations/idempotentHint")
                .and_then(|x| x.as_bool()),
            Some(true)
        );
        assert_eq!(
            v.pointer("/annotations/openWorldHint")
                .and_then(|x| x.as_bool()),
            Some(false)
        );
        assert_eq!(
            v.pointer("/annotations/title").and_then(|x| x.as_str()),
            Some("Delete Keyframes")
        );
        assert!(
            v.pointer("/annotations/deferredHint").is_none(),
            "deferredHint must live in _meta, not spec annotations"
        );
        assert_eq!(
            v.pointer("/_meta/dcc/deferred_hint")
                .and_then(|x| x.as_bool()),
            Some(true),
        );
    }

    #[test]
    fn partial_annotations_only_emit_declared_keys() {
        // Undeclared hints are omitted entirely — not defaulted to false.
        let meta = ActionMeta {
            name: "get_keyframes".into(),
            description: "read only".into(),
            annotations: ToolAnnotations {
                read_only_hint: Some(true),
                idempotent_hint: Some(true),
                ..Default::default()
            },
            ..Default::default()
        };
        let tool = action_meta_to_mcp_tool(&meta, false, &empty_eligible(), &[]);
        let v = serde_json::to_value(&tool).unwrap();
        assert_eq!(
            v.pointer("/annotations/readOnlyHint")
                .and_then(|x| x.as_bool()),
            Some(true)
        );
        assert_eq!(
            v.pointer("/annotations/idempotentHint")
                .and_then(|x| x.as_bool()),
            Some(true)
        );
        assert!(v.pointer("/annotations/destructiveHint").is_none());
        assert!(v.pointer("/annotations/openWorldHint").is_none());
    }
}

mod issue_588_input_schema_marker {
    //! Issue #588 — surface `_meta.dcc.incompleteSchema` when the catalog
    //! had to fall back to the permissive `{"type": "object"}` placeholder.
    use super::*;
    use dcc_mcp_actions::registry::ActionMeta;

    fn empty_eligible() -> std::collections::HashSet<(String, String)> {
        std::collections::HashSet::new()
    }

    #[test]
    fn null_input_schema_emits_incomplete_schema_marker_and_hint() {
        let meta = ActionMeta {
            name: "execute_python".into(),
            description: "Run python in the host DCC".into(),
            input_schema: serde_json::Value::Null,
            ..Default::default()
        };
        let tool = action_meta_to_mcp_tool(&meta, false, &empty_eligible(), &[]);
        let v = serde_json::to_value(&tool).unwrap();

        assert_eq!(
            v.pointer("/inputSchema/type").and_then(|x| x.as_str()),
            Some("object"),
            "fallback inputSchema must remain `{{type: object}}` for backwards compatibility",
        );
        assert_eq!(
            v.pointer("/_meta/dcc/incompleteSchema")
                .and_then(|x| x.as_bool()),
            Some(true),
            "incompleteSchema must surface in _meta when the author skipped inputSchema",
        );
        let hint = v
            .pointer("/_meta/dcc/schemaHint")
            .and_then(|x| x.as_str())
            .unwrap_or_default();
        assert!(
            hint.contains("did not declare an input schema"),
            "schemaHint must explain the situation to the agent: got {hint:?}",
        );
    }

    #[test]
    fn declared_input_schema_skips_marker() {
        let meta = ActionMeta {
            name: "create_sphere".into(),
            description: "Make a sphere".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["radius"],
                "properties": {"radius": {"type": "number"}},
            }),
            ..Default::default()
        };
        let tool = action_meta_to_mcp_tool(&meta, false, &empty_eligible(), &[]);
        let v = serde_json::to_value(&tool).unwrap();

        assert!(
            v.pointer("/_meta/dcc/incompleteSchema").is_none(),
            "tools with a real input schema must not carry the incomplete-schema marker",
        );
        assert!(
            v.pointer("/_meta/dcc/schemaHint").is_none(),
            "schemaHint pairs with incompleteSchema and must be absent here",
        );
    }
}
