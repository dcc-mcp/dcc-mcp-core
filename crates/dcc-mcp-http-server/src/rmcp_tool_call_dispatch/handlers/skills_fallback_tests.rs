//! End-to-end tests for the `dcc-cua` fallback on the MCP surface (PIP-3702).
//!
//! The routing contract is unit-tested in `dcc-mcp-gateway-search`; what lives
//! here is the part that unit tests cannot reach — that `fallback` is actually
//! serialized into an MCP `tools/call` response and reaches the caller.
//!
//! Split out of `rmcp_tool_call_dispatch/mod.rs` because that file sits right
//! under the 1500-line production Rust limit.

use std::sync::Arc;

use dcc_mcp_actions::registry::ToolRegistry;
use dcc_mcp_models::{SkillMetadata, ToolDeclaration};
use serde_json::json;

use crate::rmcp_tool_call_dispatch::dispatch_rmcp_tool_call;
use crate::rmcp_tool_call_dispatch::helpers::{ready_context, result_text, result_text_json};
use crate::server_state::ServerState;

/// An unanswerable request must reach the caller carrying the route, not as an
/// empty array with no next step.
#[tokio::test]
async fn search_skills_returns_the_cua_route_when_nothing_can_serve_it() {
    let registry = Arc::new(ToolRegistry::new());
    let dispatcher = Arc::new(dcc_mcp_actions::dispatcher::ToolDispatcher::new(
        (*registry).clone(),
    ));
    let catalog = Arc::new(dcc_mcp_skills::SkillCatalog::new_with_dispatcher(
        Arc::clone(&registry),
        Arc::clone(&dispatcher),
    ));
    catalog.add_skill(SkillMetadata {
        name: "maya-shot-export".to_string(),
        description: "Export the current Maya shot".to_string(),
        dcc: "maya".to_string(),
        tools: vec![ToolDeclaration {
            name: "maya_shot_export".to_string(),
            ..Default::default()
        }],
        ..Default::default()
    });
    let state = ServerState::builder(registry, dispatcher, catalog).build();

    // Nothing in the catalog matches this at all.
    let unanswerable = dispatch_rmcp_tool_call(
        &state,
        &ready_context(),
        None,
        "search_skills",
        // A query with no lexical overlap at all, so retrieval is empty rather
        // than merely weak. The weak-match case is covered by the
        // low-confidence reason code in the search crate's own tests.
        Some(json!({
            "query": "zzqx jjvvw",
            "limit": 20
        })),
        None,
    )
    .await
    .expect("search_skills dispatch should succeed");
    let payload = result_text_json(&unanswerable);
    assert_eq!(
        payload["skill_total"], 0,
        "precondition: nothing in the catalog serves this request"
    );

    let fallback = &payload["fallback"];
    assert!(
        !fallback.is_null(),
        "the MCP surface must forward the route to the caller: {payload}"
    );
    assert_eq!(fallback["skill"], "dcc-cua");
    assert!(
        fallback["reason"].is_string(),
        "the caller needs a reason code to explain the route: {fallback}"
    );
    assert!(
        fallback["preflight"].is_array(),
        "the caller needs the official component commands: {fallback}"
    );
    assert!(
        !fallback["message"].as_str().unwrap_or_default().is_empty(),
        "the caller needs one sentence to hand to the user: {fallback}"
    );

    // A request the catalog can answer carries no advice at all.
    let answerable = dispatch_rmcp_tool_call(
        &state,
        &ready_context(),
        None,
        "search_skills",
        Some(json!({"query": "maya shot export", "limit": 20})),
        None,
    )
    .await
    .expect("search_skills dispatch should succeed");
    let answerable_payload = result_text_json(&answerable);
    assert_eq!(answerable_payload["skill_total"], 1);
    assert!(
        answerable_payload["fallback"].is_null(),
        "an answered request must not advertise the route: {answerable_payload}"
    );
}

/// An empty retrieval with the route attached returns a JSON envelope, not the
/// bare "No skills found …" string. This is a deliberate change to the empty
/// branch: it makes it consistent with the non-empty branch, which has always
/// returned JSON. Callers matching on the old plain-text string see JSON here.
#[tokio::test]
async fn an_empty_retrieval_with_a_route_returns_json_not_plain_text() {
    let registry = Arc::new(ToolRegistry::new());
    let dispatcher = Arc::new(dcc_mcp_actions::dispatcher::ToolDispatcher::new(
        (*registry).clone(),
    ));
    let catalog = Arc::new(dcc_mcp_skills::SkillCatalog::new_with_dispatcher(
        Arc::clone(&registry),
        Arc::clone(&dispatcher),
    ));
    catalog.add_skill(SkillMetadata {
        name: "maya-shot-export".to_string(),
        description: "Export the current Maya shot".to_string(),
        dcc: "maya".to_string(),
        tools: vec![ToolDeclaration {
            name: "maya_shot_export".to_string(),
            ..Default::default()
        }],
        ..Default::default()
    });
    let state = ServerState::builder(registry, dispatcher, catalog).build();

    let result = dispatch_rmcp_tool_call(
        &state,
        &ready_context(),
        None,
        "search_skills",
        Some(json!({"query": "zzqx jjvvw", "limit": 20})),
        None,
    )
    .await
    .expect("search_skills dispatch should succeed");

    // Parses as JSON — i.e. not the historical plain-text branch.
    let payload = result_text_json(&result);
    assert!(payload["fallback"].is_object());

    // A `scope=` that excludes everything is the caller's own narrowing, not
    // evidence that no skill can do the job, so it falls back to the plain-text
    // branch rather than advertising the route. The catalog skill is added at
    // `Repo` scope, so asking for `admin` matches nothing.
    let scope_excluded = dispatch_rmcp_tool_call(
        &state,
        &ready_context(),
        None,
        "search_skills",
        Some(json!({"query": "zzqx jjvvw", "scope": "admin"})),
        None,
    )
    .await
    .expect("search_skills dispatch should succeed");
    assert_eq!(
        result_text(&scope_excluded),
        "No skills found matching 'zzqx jjvvw'.",
        "a scope filter that excluded everything must not advertise the route"
    );
}
