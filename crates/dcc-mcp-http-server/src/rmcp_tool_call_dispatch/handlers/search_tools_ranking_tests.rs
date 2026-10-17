//! Ranking regressions for `search_tools` — callable tools vs skill candidates.
//!
//! `search_tools` returns `tools` and `skill_candidates` as two separate lists.
//! The bug this covers: the two lists were truncated independently, each to its
//! own full `limit`, so a query matching many unloaded skills returned a page of
//! `skill_candidate` rows (every one needing a follow-up `load_skill`) and pushed
//! directly-callable tools off the first page. A caller that does not paginate
//! read that as "this tool does not exist".
//!
//! These tests run the real MCP `tools/call` path because the page assembly only
//! exists in the handler, not in the shared scorer.
//!
//! Split out of `rmcp_tool_call_dispatch/mod.rs` because that file sits right
//! under the 1500-line production Rust limit.

use std::sync::Arc;

use dcc_mcp_actions::dispatcher::ToolDispatcher;
use dcc_mcp_actions::registry::ToolRegistry;
use dcc_mcp_models::{SkillMetadata, ToolDeclaration};
use serde_json::json;

use crate::rmcp_tool_call_dispatch::dispatch_rmcp_tool_call;
use crate::rmcp_tool_call_dispatch::helpers::{ready_context, result_text_json};
use crate::server_state::ServerState;

/// Build a catalog with one loaded skill (callable tools) plus `unloaded_count`
/// unloaded skills that all match the same query.
///
/// Returns the registry and dispatcher alongside the catalog: the catalog
/// registers the loaded skill's tool into the registry it was built with, so
/// `ServerState` must be given those same instances for the tool to be visible.
///
/// The unloaded skills sort ahead of the callable tool by name, which is the
/// condition that used to push callable tools off the first page.
fn scene_catalog(
    unloaded_count: usize,
) -> (
    Arc<ToolRegistry>,
    Arc<ToolDispatcher>,
    Arc<dcc_mcp_skills::SkillCatalog>,
) {
    let registry = Arc::new(ToolRegistry::new());
    let dispatcher = Arc::new(ToolDispatcher::new((*registry).clone()));
    let catalog = Arc::new(dcc_mcp_skills::SkillCatalog::new_with_dispatcher(
        Arc::clone(&registry),
        Arc::clone(&dispatcher),
    ));

    catalog.add_skill(SkillMetadata {
        name: "maya-scene".to_string(),
        description: "Maya scene operations".to_string(),
        dcc: "maya".to_string(),
        tools: vec![ToolDeclaration {
            name: "inspect_scene".to_string(),
            description: "Inspect the current Maya scene".to_string(),
            ..Default::default()
        }],
        ..Default::default()
    });
    catalog
        .load_skill("maya-scene")
        .expect("loaded skill should expose its tool");

    for index in 0..unloaded_count {
        catalog.add_skill(SkillMetadata {
            name: format!("blender-scene-{index:02}"),
            description: "Blender scene utility".to_string(),
            dcc: "blender".to_string(),
            tools: vec![ToolDeclaration {
                name: format!("scene_helper_{index:02}"),
                description: "Blender scene helper".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        });
    }
    (registry, dispatcher, catalog)
}

/// The first page must expose a directly-callable tool even when unloaded skills
/// dominate the hit count, and the truncation signals must stay intact.
#[tokio::test]
async fn search_tools_first_page_surfaces_callable_tools_before_skill_candidates() {
    let (registry, dispatcher, catalog) = scene_catalog(40);
    let state = ServerState::builder(registry, dispatcher, catalog).build();

    let payload = result_text_json(
        &dispatch_rmcp_tool_call(
            &state,
            &ready_context(),
            None,
            "search_tools",
            Some(json!({"query": "scene", "limit": 25})),
            None,
        )
        .await
        .expect("search should dispatch"),
    );

    let tools = payload["tools"]
        .as_array()
        .expect("tools is an array")
        .iter()
        .map(|tool| tool["name"].as_str().unwrap_or_default().to_string())
        .collect::<Vec<_>>();
    let candidates = payload["skill_candidates"]
        .as_array()
        .expect("skill_candidates is an array");

    // Acceptance 1: the first page carries a directly-callable tool.
    assert!(
        tools.iter().any(|name| name.contains("inspect_scene")),
        "first page must surface a callable tool, got {tools:?}"
    );
    // Callable tools claim the shared budget before skill candidates.
    assert_eq!(
        tools.len() + candidates.len(),
        25,
        "page budget is shared and fully used"
    );
    assert!(
        !candidates.is_empty(),
        "unloaded skills should still fill the remaining budget"
    );

    // Acceptance 2: truncation signals are unchanged and not weakened.
    let total_matches = payload["total_matches"].as_u64().expect("total_matches");
    let returned = payload["returned"].as_u64().expect("returned");
    assert_eq!(returned, (tools.len() + candidates.len()) as u64);
    assert!(
        total_matches > returned,
        "result set should exceed the page ({total_matches} vs {returned})"
    );
    assert_eq!(payload["truncated"], true);
}

/// A page too small for every callable tool still prefers callable tools over
/// skill candidates rather than splitting the budget evenly.
#[tokio::test]
async fn search_tools_shared_budget_prefers_callable_tools_when_page_is_tight() {
    let registry = Arc::new(ToolRegistry::new());
    let dispatcher = Arc::new(ToolDispatcher::new((*registry).clone()));
    let catalog = Arc::new(dcc_mcp_skills::SkillCatalog::new_with_dispatcher(
        Arc::clone(&registry),
        Arc::clone(&dispatcher),
    ));
    catalog.add_skill(SkillMetadata {
        name: "houdini-scene".to_string(),
        description: "Houdini scene operations".to_string(),
        dcc: "houdini".to_string(),
        tools: vec![
            ToolDeclaration {
                name: "inspect_scene_a".to_string(),
                description: "Inspect the Houdini scene".to_string(),
                ..Default::default()
            },
            ToolDeclaration {
                name: "inspect_scene_b".to_string(),
                description: "Inspect the Houdini scene further".to_string(),
                ..Default::default()
            },
        ],
        ..Default::default()
    });
    catalog
        .load_skill("houdini-scene")
        .expect("loaded skill should expose its tools");
    for index in 0..10 {
        catalog.add_skill(SkillMetadata {
            name: format!("nuke-scene-{index:02}"),
            description: "Nuke scene utility".to_string(),
            dcc: "nuke".to_string(),
            tools: vec![ToolDeclaration {
                name: format!("scene_helper_{index:02}"),
                description: "Nuke scene helper".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        });
    }

    let state = ServerState::builder(registry, dispatcher, catalog).build();
    let payload = result_text_json(
        &dispatch_rmcp_tool_call(
            &state,
            &ready_context(),
            None,
            "search_tools",
            Some(json!({"query": "scene", "limit": 3})),
            None,
        )
        .await
        .expect("search should dispatch"),
    );

    let tools = payload["tools"].as_array().expect("tools is an array");
    let candidates = payload["skill_candidates"]
        .as_array()
        .expect("skill_candidates is an array");
    assert_eq!(tools.len(), 2, "both callable tools fit and are kept");
    assert_eq!(candidates.len(), 1, "candidates take the remaining slot");
    assert_eq!(payload["returned"], 3);
    assert_eq!(payload["total_matches"], 12);
    assert_eq!(payload["truncated"], true);
}
