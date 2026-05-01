//! Cross-module scenarios that exercise the full capability pipeline
//! (builder → index → search) on realistic multi-instance fixtures.
//!
//! These complement the per-module unit tests and guard the
//! invariants agents rely on end-to-end.

use super::*;
use crate::protocol::McpTool;
use serde_json::json;
use uuid::Uuid;

fn tool(name: &str, desc: &str, input_schema: serde_json::Value) -> McpTool {
    McpTool {
        name: name.to_string(),
        description: desc.to_string(),
        input_schema,
        output_schema: None,
        annotations: None,
        meta: None,
    }
}

fn maya_toolbox() -> Vec<McpTool> {
    vec![
        tool(
            "maya-animation.set_keyframe",
            "Insert a keyframe at the current time",
            json!({"type": "object", "properties": {"time": {"type": "number"}}, "required": ["time"]}),
        ),
        tool(
            "maya-geometry.create_sphere",
            "Create a polygonal sphere at the origin",
            json!({"type": "object", "properties": {"radius": {"type": "number"}}, "required": ["radius"]}),
        ),
        tool(
            "maya-geometry.create_cube",
            "Create a polygonal cube",
            json!({"type": "object", "properties": {"size": {"type": "number"}}}),
        ),
        tool(
            "__skill__maya-animation",
            "Skill stub — not addressable directly",
            json!({"type": "object"}),
        ),
        tool("list_skills", "meta-tool", json!({"type": "object"})),
    ]
}

fn blender_toolbox() -> Vec<McpTool> {
    vec![
        tool(
            "blender-rendering.render",
            "Render the current scene",
            json!({"type": "object", "properties": {"frame": {"type": "integer"}}}),
        ),
        tool(
            "blender-geometry.add_material",
            "Attach a material to the selected object",
            json!({"type": "object", "properties": {"name": {"type": "string"}}}),
        ),
    ]
}

#[test]
fn end_to_end_two_backends_search_and_route() {
    let index = CapabilityIndex::new();
    let maya_id = Uuid::from_u128(0xaaaa_bbbb_0000_0000_0000_0000_0000_0001);
    let blender_id = Uuid::from_u128(0xcccc_dddd_0000_0000_0000_0000_0000_0001);

    // Phase 1: build and upsert both instances.
    let maya_tools = maya_toolbox();
    let blender_tools = blender_toolbox();
    let maya_out = build_records_from_backend(BuildInput {
        instance_id: maya_id,
        dcc_type: "maya",
        backend_tools: &maya_tools,
    });
    assert!(
        maya_out.skipped >= 2,
        "maya must skip the skill stub and list_skills; skipped={}",
        maya_out.skipped,
    );
    assert_eq!(maya_out.records.len(), 3);
    index.upsert_instance(maya_id, maya_out.records, maya_out.fingerprint);

    let blender_out = build_records_from_backend(BuildInput {
        instance_id: blender_id,
        dcc_type: "blender",
        backend_tools: &blender_tools,
    });
    assert_eq!(
        blender_out.skipped, 0,
        "blender fixture has no filter-eligible tools; skipped={}",
        blender_out.skipped,
    );
    assert_eq!(blender_out.records.len(), 2);
    index.upsert_instance(blender_id, blender_out.records, blender_out.fingerprint);

    let snap = index.snapshot();

    // Phase 2: the slim/rest tools/list token budget is bounded —
    // even with two backends, the capability index carries just the
    // actionable rows (5 total here after filtering), **not** the
    // full backend `tools/list` which would include skill stubs and
    // local meta-tools.
    assert_eq!(snap.records.len(), 5);
    assert!(
        snap.records
            .iter()
            .all(|r| !r.backend_tool.starts_with("__skill__")),
        "skill stubs must never appear in the capability index",
    );
    assert!(
        snap.records.iter().all(|r| r.backend_tool != "list_skills"),
        "gateway-local tools must be served by the gateway, not the index",
    );

    // Phase 3: `search_tools` narrows "sphere" to the one Maya action
    // without leaking any Blender rows.
    let hits = search(
        &snap,
        &SearchQuery {
            query: "sphere".into(),
            dcc_type: Some("maya".into()),
            ..Default::default()
        },
    );
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].record.backend_tool, "create_sphere");
    // The slug carries everything needed to route the call back to
    // the exact backend instance.
    let (dcc, id8, tool) = record::parse_slug(&hits[0].record.tool_slug).unwrap();
    assert_eq!(dcc, "maya");
    assert_eq!(id8, &maya_id.to_string().replace('-', "")[..8]);
    assert_eq!(tool, "create_sphere");

    // Phase 4: the same query without a dcc_type filter sees both
    // backends but still scores the Maya action first because its
    // name is an exact substring and it carries a schema.
    let hits = search(
        &snap,
        &SearchQuery {
            query: "sphere".into(),
            ..Default::default()
        },
    );
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].record.backend_tool, "create_sphere");

    // Phase 5: removing the Maya instance drops every Maya row in
    // one O(n) swap and leaves the Blender rows intact.
    assert!(index.remove_instance(maya_id));
    let snap = index.snapshot();
    assert!(
        snap.records.iter().all(|r| r.dcc_type == "blender"),
        "Maya records survived after remove_instance; got {:?}",
        snap.records
            .iter()
            .map(|r| &r.tool_slug)
            .collect::<Vec<_>>()
    );
}

#[test]
fn colliding_backend_tool_names_stay_addressable() {
    // #657 design note: two backends publishing the same action
    // name must both remain addressable via distinct slugs. This
    // guards against a naive HashMap<String, CapabilityRecord>
    // regression in the index.
    let index = CapabilityIndex::new();
    let a = Uuid::from_u128(0x1111_1111_0000_0000_0000_0000_0000_0001);
    let b = Uuid::from_u128(0x2222_2222_0000_0000_0000_0000_0000_0001);

    let tools = vec![tool(
        "export_fbx",
        "Export the scene",
        json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}),
    )];

    for (iid, dcc) in [(a, "maya"), (b, "blender")] {
        let out = build_records_from_backend(BuildInput {
            instance_id: iid,
            dcc_type: dcc,
            backend_tools: &tools,
        });
        index.upsert_instance(iid, out.records, out.fingerprint);
    }

    let snap = index.snapshot();
    assert_eq!(snap.records.len(), 2);
    let slugs: Vec<&str> = snap.records.iter().map(|r| r.tool_slug.as_str()).collect();
    assert_ne!(
        slugs[0], slugs[1],
        "colliding tool names must still produce distinct slugs",
    );
    // Exactly one hit per DCC bucket when the caller narrows.
    for dcc in ["maya", "blender"] {
        let hits = search(
            &snap,
            &SearchQuery {
                query: "export".into(),
                dcc_type: Some(dcc.to_string()),
                ..Default::default()
            },
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].record.dcc_type, dcc);
    }
}

#[test]
fn fingerprint_change_detects_skill_load_unload_cycle() {
    // Simulates the `tools/list_changed` path: a skill loads, a new
    // action appears, the index fingerprint bumps; the skill then
    // unloads and the fingerprint returns to the original value.
    let iid = Uuid::from_u128(0xfeed_0000_0000_0000_0000_0000_0000_0001);

    let before = vec![tool("list_scenes", "", json!({"type": "object"}))];
    let fp0 = build_records_from_backend(BuildInput {
        instance_id: iid,
        dcc_type: "maya",
        backend_tools: &before,
    })
    .fingerprint;

    let after_load = vec![
        tool("list_scenes", "", json!({"type": "object"})),
        tool(
            "render_farm.submit",
            "Submit render to farm",
            json!({"type": "object"}),
        ),
    ];
    let fp1 = build_records_from_backend(BuildInput {
        instance_id: iid,
        dcc_type: "maya",
        backend_tools: &after_load,
    })
    .fingerprint;
    assert_ne!(fp0, fp1, "adding an action must bump the fingerprint");

    let fp2 = build_records_from_backend(BuildInput {
        instance_id: iid,
        dcc_type: "maya",
        backend_tools: &before,
    })
    .fingerprint;
    assert_eq!(
        fp0, fp2,
        "unloading the skill must return the fingerprint to the pre-load value",
    );
}
