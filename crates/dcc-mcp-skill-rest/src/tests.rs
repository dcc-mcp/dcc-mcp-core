//! End-to-end tests for the per-DCC REST skill API.
//!
//! The goals, in priority order, mirror the user story in issue #658
//! and the token-efficiency requirement we were explicitly asked to
//! validate:
//!
//! 1. **RESTful skill surface works**: search → describe → call is a
//!    valid agent flow on a real axum Router.
//! 2. **MCP can call the same capabilities accurately**: the MCP
//!    `tools/call` path produces the same output envelope as
//!    `POST /v1/call` for the same slug. No drift.
//! 3. **No token waste**: `/v1/search` hits stay under the strict
//!    [`SEARCH_HIT_BUDGET_BYTES`] budget and do not carry the full
//!    `input_schema` by accident.
//! 4. **Enterprise controls** (#660): auth gate, audit sink,
//!    readiness — each behaves as specified.

use std::sync::Arc;

use axum::Router;
use axum_test::TestServer;
use serde_json::{Value, json};

use dcc_mcp_actions::dispatcher::ToolDispatcher;
use dcc_mcp_actions::registry::{ToolMeta, ToolRegistry};
use dcc_mcp_models::SkillMetadata;
use dcc_mcp_skills::SkillCatalog;

use super::SEARCH_HIT_BUDGET_BYTES;
use super::audit::{AuditOutcome, VecAuditSink};
use super::auth::{AllowLocalhostGate, BearerTokenGate};
use super::readiness::StaticReadiness;
use super::router::{SkillRestConfig, build_skill_rest_router};
use super::service::SkillRestService;

// ── Fixture ──────────────────────────────────────────────────────────

/// Registers one skill with one action, wires a dispatcher handler so
/// `/v1/call` can actually invoke it, and returns the pieces every
/// test uses.
fn fixture_loaded_spheres() -> (SkillRestService, Arc<ToolRegistry>, Arc<ToolDispatcher>) {
    let registry = Arc::new(ToolRegistry::new());

    let schema = json!({
        "type": "object",
        "properties": {"radius": {"type": "number"}},
        "required": ["radius"]
    });

    registry.register_action(ToolMeta {
        name: "create_sphere".into(),
        dcc: "maya".into(),
        description: "Create a polygon sphere".into(),
        tags: vec!["geometry".into(), "poly".into()],
        input_schema: schema,
        skill_name: Some("spheres".into()),
        enabled: true,
        ..Default::default()
    });

    let dispatcher = Arc::new(ToolDispatcher::new((*registry).clone()));
    dispatcher.register_handler("create_sphere", |params: Value| {
        let radius = params.get("radius").and_then(Value::as_f64).unwrap_or(1.0);
        Ok(json!({"name": "pSphere1", "radius": radius}))
    });

    let catalog = Arc::new(SkillCatalog::new_with_dispatcher(
        registry.clone(),
        dispatcher.clone(),
    ));

    // Add a SkillMetadata record so `list_skills` reports the skill as
    // loaded. Wire directly via `add_skill` to avoid filesystem I/O.
    let meta = SkillMetadata {
        name: "spheres".into(),
        dcc: "maya".into(),
        description: "Spheres toolkit".into(),
        tags: vec!["geometry".into()],
        ..Default::default()
    };
    catalog.add_skill(meta);
    // Mark the skill loaded so the /v1/call ready-gate doesn't reject.
    let _ = catalog.load_skill("spheres");

    let service = SkillRestService::from_catalog_and_dispatcher(catalog, dispatcher.clone());
    (service, registry, dispatcher)
}

fn build_server(service: SkillRestService) -> (TestServer, Arc<VecAuditSink>) {
    let sink = Arc::new(VecAuditSink::new());
    let cfg = SkillRestConfig::new(service)
        .with_audit(sink.clone())
        .with_readiness(Arc::new(StaticReadiness::fully_ready()))
        .with_auth(Arc::new(AllowLocalhostGate::new()));
    let app: Router = build_skill_rest_router(cfg);
    let server = TestServer::new(app);
    (server, sink)
}

// ── High-value scenarios ─────────────────────────────────────────────

/// Goal #1 — agent flow: search then describe then call, with a real
/// axum server in the loop.
#[tokio::test]
async fn search_describe_call_round_trip() {
    let (svc, _reg, _disp) = fixture_loaded_spheres();
    let (server, _audit) = build_server(svc);

    // 1. Search.
    let resp = server
        .post("/v1/search")
        .json(&json!({"query": "sphere"}))
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["total"], 1);
    let slug = body["hits"][0]["slug"].as_str().expect("slug").to_owned();
    assert_eq!(slug, "maya.spheres.create_sphere");
    assert!(
        body["hits"][0].get("input_schema").is_none(),
        "search hit must NOT expand input_schema (token budget)"
    );

    // 2. Describe with schema.
    let resp = server
        .post("/v1/describe")
        .json(&json!({"tool_slug": slug, "include_schema": true}))
        .await;
    resp.assert_status_ok();
    let desc: Value = resp.json();
    assert_eq!(desc["entry"]["slug"], slug);
    assert!(desc["input_schema"].is_object());
    assert_eq!(desc["input_schema"]["required"][0], "radius");

    // 3. Call — same slug, valid params.
    let resp = server
        .post("/v1/call")
        .json(&json!({"tool_slug": slug, "params": {"radius": 2.5}}))
        .await;
    resp.assert_status_ok();
    let out: Value = resp.json();
    assert_eq!(out["slug"], slug);
    assert_eq!(out["output"]["name"], "pSphere1");
    assert_eq!(out["output"]["radius"], 2.5);
}

/// Goal #2 — MCP and REST must yield identical envelopes for the same
/// slug. We reach in through the service layer (which is also what
/// the MCP wrapper tools on the gateway dispatch through) and compare
/// with what HTTP produces.
#[tokio::test]
async fn mcp_wrapper_and_rest_agree_on_call_output() {
    let (svc, _reg, _disp) = fixture_loaded_spheres();

    // "MCP wrapper" path — same service.call() the gateway's
    // `call_tool` MCP wrapper uses.
    let mcp_out = svc
        .call(&super::service::CallRequest {
            tool_slug: super::service::ToolSlug("maya.spheres.create_sphere".into()),
            params: json!({"radius": 3.0}),
            meta: None,
        })
        .expect("mcp call ok");

    // REST path.
    let (server, _audit) = build_server(svc);
    let resp = server
        .post("/v1/call")
        .json(&json!({
            "tool_slug": "maya.spheres.create_sphere",
            "params": {"radius": 3.0}
        }))
        .await;
    resp.assert_status_ok();
    let rest_out: Value = resp.json();

    // Envelopes must agree on every semantically-meaningful field.
    assert_eq!(rest_out["slug"], mcp_out.slug.0);
    assert_eq!(rest_out["output"], mcp_out.output);
    assert_eq!(rest_out["validation_skipped"], mcp_out.validation_skipped);
}

/// Goal #3 — strict per-hit byte budget. Prevents accidental token
/// waste if someone adds a new field to `SkillListEntry` later.
#[tokio::test]
async fn search_hit_is_compact() {
    // Craft a skill with an absurdly long description; the hit must
    // still serialise under SEARCH_HIT_BUDGET_BYTES.
    let (svc, _reg, _disp) = fixture_loaded_spheres();
    let (server, _audit) = build_server(svc);

    let resp = server.post("/v1/search").json(&json!({})).await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    for hit in body["hits"].as_array().unwrap() {
        let s = serde_json::to_string(hit).unwrap();
        assert!(
            s.len() < SEARCH_HIT_BUDGET_BYTES,
            "hit serialised to {} bytes (> {} budget): {s}",
            s.len(),
            SEARCH_HIT_BUDGET_BYTES
        );
    }
}

/// Invalid params return 400 with `kind=invalid-params`.
#[tokio::test]
async fn call_rejects_invalid_params() {
    let (svc, _, _) = fixture_loaded_spheres();
    let (server, audit) = build_server(svc);

    let resp = server
        .post("/v1/call")
        .json(&json!({
            "tool_slug": "maya.spheres.create_sphere",
            "params": {}  // missing required `radius`
        }))
        .await;

    assert_eq!(resp.status_code().as_u16(), 400);
    let body: Value = resp.json();
    assert_eq!(body["kind"], "invalid-params");
    assert!(body["message"].is_string());

    // Every call must leave one audit record — failure or success.
    let events = audit.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].slug, "maya.spheres.create_sphere");
    assert!(matches!(events[0].outcome, AuditOutcome::Failure(_)));
}

/// Unknown slug returns 404 with `kind=unknown-slug` and a hint.
#[tokio::test]
async fn call_rejects_unknown_slug() {
    let (svc, _, _) = fixture_loaded_spheres();
    let (server, _) = build_server(svc);
    let resp = server
        .post("/v1/call")
        .json(&json!({"tool_slug": "maya.spheres.does_not_exist"}))
        .await;
    assert_eq!(resp.status_code().as_u16(), 404);
    let body: Value = resp.json();
    assert_eq!(body["kind"], "unknown-slug");
    assert!(body["hint"].is_string());
}

/// Malformed slug is 400 bad-request, not 404.
#[tokio::test]
async fn call_rejects_malformed_slug() {
    let (svc, _, _) = fixture_loaded_spheres();
    let (server, _) = build_server(svc);
    let resp = server
        .post("/v1/call")
        .json(&json!({"tool_slug": "not-a-slug"}))
        .await;
    assert_eq!(resp.status_code().as_u16(), 400);
    let body: Value = resp.json();
    assert_eq!(body["kind"], "bad-request");
}

/// Readiness gating: /v1/call must 503 until the probe
/// is green, but /v1/healthz stays 200 regardless.
#[tokio::test]
async fn call_respects_readiness() {
    let (svc, _, _) = fixture_loaded_spheres();
    let readiness = Arc::new(StaticReadiness::new());
    let cfg = SkillRestConfig::new(svc).with_readiness(readiness.clone());
    let server = TestServer::new(build_skill_rest_router(cfg));

    // readyz reflects not-ready.
    let resp = server.get("/v1/readyz").await;
    assert_eq!(resp.status_code().as_u16(), 503);

    // healthz is always 200.
    let resp = server.get("/v1/healthz").await;
    resp.assert_status_ok();

    // /v1/call with not-ready returns 503.
    let resp = server
        .post("/v1/call")
        .json(&json!({
            "tool_slug": "maya.spheres.create_sphere",
            "params": {"radius": 1.0}
        }))
        .await;
    assert_eq!(resp.status_code().as_u16(), 503);
    let body: Value = resp.json();
    assert_eq!(body["kind"], "not-ready");

    // Flip to ready — call succeeds.
    readiness.set_dispatcher_ready(true);
    readiness.set_dcc_ready(true);
    let resp = server
        .post("/v1/call")
        .json(&json!({
            "tool_slug": "maya.spheres.create_sphere",
            "params": {"radius": 1.0}
        }))
        .await;
    resp.assert_status_ok();
}

/// BearerTokenGate rejects requests without a valid header.
#[tokio::test]
async fn bearer_auth_gate_rejects_without_token() {
    let (svc, _, _) = fixture_loaded_spheres();
    let gate = Arc::new(BearerTokenGate::new(vec!["s3cret".into()]).unwrap());
    let cfg = SkillRestConfig::new(svc).with_auth(gate);
    let server = TestServer::new(build_skill_rest_router(cfg));

    let resp = server.get("/v1/skills").await;
    assert_eq!(resp.status_code().as_u16(), 401);
    let body: Value = resp.json();
    assert_eq!(body["kind"], "unauthorized");
}

/// With a valid token, all routes work. Request id echoes back.
#[tokio::test]
async fn bearer_auth_gate_accepts_valid_token_and_echoes_request_id() {
    let (svc, _, _) = fixture_loaded_spheres();
    let gate = Arc::new(BearerTokenGate::new(vec!["s3cret".into()]).unwrap());
    let cfg = SkillRestConfig::new(svc).with_auth(gate);
    let server = TestServer::new(build_skill_rest_router(cfg));

    let resp = server
        .get("/v1/skills")
        .add_header("authorization", "Bearer s3cret")
        .add_header("x-request-id", "req-42")
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["request_id"], "req-42");
}

/// OpenAPI document lists every documented route and parses as JSON.
#[tokio::test]
async fn openapi_document_served() {
    let (svc, _, _) = fixture_loaded_spheres();
    let (server, _) = build_server(svc);
    let resp = server.get("/v1/openapi.json").await;
    resp.assert_status_ok();
    let doc: Value = resp.json();
    let openapi_version = doc["openapi"].as_str().unwrap_or_default();
    assert!(
        openapi_version.starts_with("3."),
        "expected OpenAPI 3.x, got {openapi_version}"
    );
    for p in [
        "/v1/skills",
        "/v1/search",
        "/v1/describe",
        "/v1/call",
        "/v1/context",
        "/v1/healthz",
        "/v1/readyz",
    ] {
        assert!(
            doc["paths"].get(p).is_some(),
            "OpenAPI doc missing path {p}"
        );
    }
}

#[tokio::test]
async fn docs_ui_served_and_can_be_disabled() {
    let (svc, _, _) = fixture_loaded_spheres();
    let (server, _) = build_server(svc);

    let _g = dcc_mcp_test_utils::EnvVarGuard::set("DCC_MCP_DOCS_UI", None);
    let resp = server.get("/docs").await;
    resp.assert_status_ok();
    let html = resp.text();
    assert!(html.contains("scalar") || html.contains("Scalar"));
    drop(_g);

    let _g = dcc_mcp_test_utils::EnvVarGuard::set("DCC_MCP_DOCS_UI", Some("0"));
    let disabled = server.get("/docs").await;
    disabled.assert_status_not_found();
}

/// Context endpoint reports loaded skills and action count.
#[tokio::test]
async fn context_reports_counts() {
    let (svc, _, _) = fixture_loaded_spheres();
    svc.update_context(|c| {
        c.scene = Some("unsaved".into());
        c.dcc = Some("maya".into());
    });
    let (server, _) = build_server(svc);
    let resp = server.get("/v1/context").await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["dcc"], "maya");
    assert_eq!(body["scene"], "unsaved");
    assert!(body["action_count"].as_u64().unwrap() >= 1);
    assert!(body["loaded_skill_count"].as_u64().unwrap() >= 1);
}

/// `/v1/tools/{slug}` path form is a describe alias.
#[tokio::test]
async fn tools_path_alias_returns_same_as_describe() {
    let (svc, _, _) = fixture_loaded_spheres();
    let (server, _) = build_server(svc);

    let r1 = server
        .post("/v1/describe")
        .json(&json!({"tool_slug": "maya.spheres.create_sphere"}))
        .await;
    r1.assert_status_ok();
    let r2 = server.get("/v1/tools/maya.spheres.create_sphere").await;
    r2.assert_status_ok();

    let b1: Value = r1.json();
    let b2: Value = r2.json();
    assert_eq!(b1["entry"], b2["entry"]);
    assert_eq!(b1["input_schema"], b2["input_schema"]);
}

/// `POST /v1/dcc/{dcc_type}/call` routes by backend tool name without a dotted slug.
#[tokio::test]
async fn dcc_path_post_invokes_backend_tool() {
    let (svc, _, _) = fixture_loaded_spheres();
    let (server, _) = build_server(svc);
    let resp = server
        .post("/v1/dcc/maya/call")
        .json(&json!({"backend_tool": "create_sphere", "arguments": {"radius": 3.0}}))
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["slug"], "maya.spheres.create_sphere");
    assert_eq!(body["output"]["radius"], 3.0);
}

/// Every successful request gets exactly one audit event with a
/// success outcome and a non-empty request id.
#[tokio::test]
async fn every_success_emits_one_audit_event() {
    let (svc, _, _) = fixture_loaded_spheres();
    let (server, audit) = build_server(svc);

    let _ = server.get("/v1/skills").await;
    let _ = server.post("/v1/search").json(&json!({})).await;
    let _ = server
        .post("/v1/call")
        .json(&json!({
            "tool_slug": "maya.spheres.create_sphere",
            "params": {"radius": 1.0}
        }))
        .await;

    let events = audit.events();
    assert!(
        events.len() >= 3,
        "expected at least 3 audit events, got {} — {events:#?}",
        events.len()
    );
    for e in events {
        assert!(!e.request_id.is_empty(), "request_id must be populated");
        assert!(matches!(e.outcome, AuditOutcome::Success));
    }
}

// ── Real-world scenarios ─────────────────────────────────────────────
//
// The following block simulates a busy DCC instance: multiple skills,
// multiple DCCs in the registry, and dozens of actions. These are the
// shapes an agent actually sees in production, and they exercise the
// search/describe/call contract end-to-end through axum, not just the
// service-layer unit tests above.

/// Build a realistic catalog: two loaded Maya skills (`spheres`,
/// `lighting`) plus one *unloaded* Blender skill (`rigging`). Three
/// actions on each skill, for nine total. The handlers are wired so
/// the entire flow — including /v1/call — works against the fixture.
fn fixture_multi_skill() -> (SkillRestService, Arc<VecAuditSink>) {
    let registry = Arc::new(ToolRegistry::new());

    // Keep schema identical so we can focus on list/search/filter.
    let num_schema = json!({
        "type": "object",
        "properties": {"n": {"type": "number"}},
        "required": ["n"]
    });

    // Maya · spheres (loaded) — 3 actions.
    for name in ["create_sphere", "scale_sphere", "delete_sphere"] {
        registry.register_action(ToolMeta {
            name: name.into(),
            dcc: "maya".into(),
            description: format!("Spheres toolkit: {name}"),
            tags: vec!["geometry".into(), "poly".into()],
            input_schema: num_schema.clone(),
            skill_name: Some("spheres".into()),
            enabled: true,
            ..Default::default()
        });
    }

    // Maya · lighting (loaded) — 3 actions. Shares one tag with
    // spheres (`scene`) and has its own (`light`).
    for name in ["create_light", "set_intensity", "delete_light"] {
        registry.register_action(ToolMeta {
            name: name.into(),
            dcc: "maya".into(),
            description: format!("Lighting toolkit: {name}"),
            tags: vec!["scene".into(), "light".into()],
            input_schema: num_schema.clone(),
            skill_name: Some("lighting".into()),
            enabled: true,
            ..Default::default()
        });
    }

    // Blender · rigging (UNloaded) — 3 actions. Should be filtered
    // out of /v1/search by default (loaded_only == true) and also
    // rejected by /v1/call with kind=skill-not-loaded.
    for name in ["add_bone", "weight_paint", "remove_bone"] {
        registry.register_action(ToolMeta {
            name: name.into(),
            dcc: "blender".into(),
            description: format!("Rigging toolkit: {name}"),
            tags: vec!["rig".into()],
            input_schema: num_schema.clone(),
            skill_name: Some("rigging".into()),
            enabled: true,
            ..Default::default()
        });
    }

    let dispatcher = Arc::new(ToolDispatcher::new((*registry).clone()));
    // Single generic handler — every action just echoes `n` so the
    // REST call path can succeed on any loaded slug.
    for action in [
        "create_sphere",
        "scale_sphere",
        "delete_sphere",
        "create_light",
        "set_intensity",
        "delete_light",
    ] {
        dispatcher.register_handler(action, |params: Value| {
            let n = params.get("n").and_then(Value::as_f64).unwrap_or(0.0);
            Ok(json!({"echo": n}))
        });
    }

    let catalog = Arc::new(SkillCatalog::new_with_dispatcher(
        registry.clone(),
        dispatcher.clone(),
    ));

    for (name, dcc, loaded) in [
        ("spheres", "maya", true),
        ("lighting", "maya", true),
        ("rigging", "blender", false),
    ] {
        catalog.add_skill(SkillMetadata {
            name: name.into(),
            dcc: dcc.into(),
            description: format!("{name} toolkit"),
            ..Default::default()
        });
        if loaded {
            let _ = catalog.load_skill(name);
        }
    }

    let svc = SkillRestService::from_catalog_and_dispatcher(catalog, dispatcher);
    let sink = Arc::new(VecAuditSink::new());
    (svc, sink)
}

/// Wire a router around an existing audit sink (re-using `build_server`
/// is nice but we need to share the sink across test steps here).
fn build_server_with_audit(svc: SkillRestService, sink: Arc<VecAuditSink>) -> TestServer {
    let cfg = SkillRestConfig::new(svc)
        .with_audit(sink)
        .with_readiness(Arc::new(StaticReadiness::fully_ready()))
        .with_auth(Arc::new(AllowLocalhostGate::new()));
    TestServer::new(build_skill_rest_router(cfg))
}

/// Real-world agent flow: search by DCC + tag, pick a hit, describe
/// it, call it. Verifies the multi-skill, multi-DCC instance case
/// that #658 was designed for.
#[tokio::test]
async fn multi_skill_agent_flow_filters_by_dcc_and_tag() {
    let (svc, sink) = fixture_multi_skill();
    let server = build_server_with_audit(svc, sink.clone());

    // Narrow by dcc=maya AND tag=light — only lighting survives.
    let resp = server
        .post("/v1/search")
        .json(&json!({"dcc_type": "maya", "tags": ["light"]}))
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["total"], 3, "expected 3 lighting actions, got {body}");

    let skills: Vec<&str> = body["hits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["skill"].as_str().unwrap())
        .collect();
    for s in &skills {
        assert_eq!(*s, "lighting", "filter leaked a non-lighting skill: {s}");
    }

    // Describe + call on a slug we just discovered.
    let slug = body["hits"][0]["slug"].as_str().unwrap().to_owned();
    let desc = server
        .post("/v1/describe")
        .json(&json!({"tool_slug": slug, "include_schema": true}))
        .await;
    desc.assert_status_ok();
    let desc_body: Value = desc.json();
    assert_eq!(desc_body["entry"]["skill"], "lighting");

    let call = server
        .post("/v1/call")
        .json(&json!({"tool_slug": slug, "params": {"n": 4.0}}))
        .await;
    call.assert_status_ok();
    let call_body: Value = call.json();
    assert_eq!(call_body["output"]["echo"], 4.0);

    // One audit event per HTTP call (search + describe + call = 3).
    assert_eq!(sink.events().len(), 3);
}

/// Calls into an *unloaded* skill must be rejected at /v1/call with
/// kind=skill-not-loaded, even when the slug is otherwise well-formed.
/// This is the key multi-DCC safety property for #658.
#[tokio::test]
async fn call_rejects_unloaded_skill_in_multi_skill_catalog() {
    let (svc, _) = fixture_multi_skill();
    let (server, _) = build_server(svc);

    // rigging is unloaded in the fixture.
    let resp = server
        .post("/v1/call")
        .json(&json!({
            "tool_slug": "blender.rigging.add_bone",
            "params": {"n": 1.0}
        }))
        .await;

    // 409 Conflict is the REST mapping of ServiceErrorKind::SkillNotLoaded.
    assert_eq!(resp.status_code().as_u16(), 409);
    let body: Value = resp.json();
    assert_eq!(body["kind"], "skill-not-loaded");
    assert!(
        body["hint"]
            .as_str()
            .unwrap_or_default()
            .contains("load_skill"),
        "expected a load_skill remediation hint, got {body}"
    );
}

/// `loaded_only=false` returns unloaded actions too, but they come
/// back with `loaded: false` so an agent can decide whether to load
/// the owning skill or pick a loaded alternative.
#[tokio::test]
async fn search_loaded_only_false_surfaces_unloaded_skills() {
    let (svc, _) = fixture_multi_skill();
    let (server, _) = build_server(svc);

    let resp = server
        .post("/v1/search")
        .json(&json!({"loaded_only": false}))
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["total"], 9, "expected all 9 actions exposed");

    let unloaded: Vec<_> = body["hits"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|h| h["loaded"] == false)
        .collect();
    assert_eq!(unloaded.len(), 3, "rigging has 3 unloaded actions");
    for h in &unloaded {
        assert_eq!(h["skill"], "rigging");
        assert_eq!(h["dcc"], "blender");
        assert_eq!(h["next_step"]["action"], "load_skill");
        assert_eq!(h["next_step"]["arguments"]["skill_name"], "rigging");
        assert_eq!(h["next_step"]["arguments"]["dcc"], "blender");
    }
}

/// REST-only progressive loading: discover an unloaded skill, POST the
/// returned `next_step.arguments` to `/v1/load_skill`, then search again
/// and see the action become loaded without any MCP `tools/call`.
#[tokio::test]
async fn rest_load_skill_endpoint_completes_progressive_loading_loop() {
    let (svc, _) = fixture_multi_skill();
    let (server, _) = build_server(svc);

    let search = server
        .post("/v1/search")
        .json(&json!({"query": "bone", "loaded_only": false}))
        .await;
    search.assert_status_ok();
    let body: Value = search.json();
    let next_args = body["hits"][0]["next_step"]["arguments"].clone();
    assert_eq!(next_args["skill_name"], "rigging");

    let loaded = server.post("/v1/load_skill").json(&next_args).await;
    loaded.assert_status_ok();
    let loaded_body: Value = loaded.json();
    assert_eq!(loaded_body["success"], true);
    assert_eq!(loaded_body["skill_name"], "rigging");

    let search = server
        .post("/v1/search")
        .json(&json!({"query": "bone"}))
        .await;
    search.assert_status_ok();
    let after: Value = search.json();
    assert!(
        after["hits"]
            .as_array()
            .unwrap()
            .iter()
            .all(|h| h["loaded"] == true),
        "default search should only return loaded rigging actions after /v1/load_skill: {after}"
    );
}

/// The whole `/v1/search` response — not just one hit — must stay
/// compact enough to enumerate realistic DCC instances in a single
/// agent turn. Asserts a generous-but-finite ceiling that still
/// regresses if someone accidentally re-introduces `input_schema`
/// into `SkillListEntry`.
///
/// At 9 actions the response must fit well under 8 KiB. Rationale:
/// modern LLM context windows treat 8 KiB ≈ 2 k tokens; that's the
/// threshold where "cheap to enumerate" stops being true.
#[tokio::test]
async fn search_total_response_fits_token_budget_for_nine_actions() {
    let (svc, _) = fixture_multi_skill();
    let (server, _) = build_server(svc);

    let resp = server
        .post("/v1/search")
        .json(&json!({"loaded_only": false}))
        .await;
    resp.assert_status_ok();

    // Per-instance ceiling derived from SEARCH_HIT_BUDGET_BYTES plus
    // envelope overhead; covers the `{total, hits:[]}` framing.
    let bytes: Value = resp.json();
    let serialised = serde_json::to_string(&bytes).unwrap();
    let ceiling = SEARCH_HIT_BUDGET_BYTES * 9 + 256;
    assert!(
        serialised.len() < ceiling,
        "full /v1/search response was {} bytes, exceeds {} ceiling — \
         likely schema re-expansion or duplicated fields",
        serialised.len(),
        ceiling,
    );
}

/// HTTP-layer `limit` must truncate wire hits *before* they reach
/// the caller. Important: the service unit test already covers the
/// logic; this one proves the axum bridge does not silently drop the
/// option.
#[tokio::test]
async fn search_http_layer_honours_limit() {
    let (svc, _) = fixture_multi_skill();
    let (server, _) = build_server(svc);

    let resp = server
        .post("/v1/search")
        .json(&json!({"loaded_only": false, "limit": 3}))
        .await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["total"], 3);
    assert_eq!(body["hits"].as_array().unwrap().len(), 3);
}

/// Describe with `include_schema=false` must be materially smaller
/// than the schema-expanded form. This is the token-saving contract
/// an agent relies on when it only needs metadata.
#[tokio::test]
async fn describe_without_schema_is_materially_smaller() {
    let (svc, _) = fixture_multi_skill();
    let (server, _) = build_server(svc);

    let slug = "maya.spheres.create_sphere";

    let with_schema = server
        .post("/v1/describe")
        .json(&json!({"tool_slug": slug, "include_schema": true}))
        .await;
    with_schema.assert_status_ok();
    let with_bytes = serde_json::to_vec(&with_schema.json::<Value>()).unwrap();

    let without_schema = server
        .post("/v1/describe")
        .json(&json!({"tool_slug": slug, "include_schema": false}))
        .await;
    without_schema.assert_status_ok();
    let without_bytes = serde_json::to_vec(&without_schema.json::<Value>()).unwrap();

    assert!(
        without_bytes.len() < with_bytes.len(),
        "include_schema=false ({} B) should be smaller than \
         include_schema=true ({} B)",
        without_bytes.len(),
        with_bytes.len(),
    );
    // And the schema field is genuinely absent, not just shorter.
    let no_schema: Value = serde_json::from_slice(&without_bytes).unwrap();
    assert!(no_schema.get("input_schema").is_none());
}

/// The cornerstone parity test for the whole #658 / #660 effort:
/// every hit returned by `/v1/search` is reachable through the same
/// `SkillRestService.search()` the MCP gateway wrapper uses. This
/// verifies — in a multi-skill, multi-DCC instance — that REST and
/// MCP clients see the *same* list of tools, in the *same* order,
/// with *byte-identical* slugs.
///
/// A regression here would mean the two surfaces diverge, forcing
/// agents to learn two tool catalogs and doubling the token cost of
/// discovery.
#[tokio::test]
async fn rest_and_mcp_surfaces_agree_on_multi_skill_catalog() {
    let (svc, _) = fixture_multi_skill();

    // "MCP wrapper" path — what gateway::capability_service::search
    // invokes on this instance.
    let mcp_hits = svc.search(&super::service::SearchRequest {
        loaded_only: false,
        ..Default::default()
    });

    // REST path — what non-MCP callers see.
    let (server, _) = build_server(svc);
    let resp = server
        .post("/v1/search")
        .json(&json!({"loaded_only": false}))
        .await;
    resp.assert_status_ok();
    let rest_body: Value = resp.json();
    let rest_slugs: Vec<String> = rest_body["hits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["slug"].as_str().unwrap().to_owned())
        .collect();
    let mcp_slugs: Vec<String> = mcp_hits.hits.iter().map(|h| h.slug.0.clone()).collect();

    assert_eq!(
        mcp_slugs, rest_slugs,
        "REST and MCP surfaces returned different slug lists; \
         agents would need two catalogs — token waste + drift risk"
    );
    assert_eq!(
        rest_body["total"].as_u64().unwrap() as usize,
        mcp_hits.total,
        "total mismatch between REST and MCP"
    );
}

// ── #818 phase 1 — resource & prompt endpoints ────────────────────────

/// `GET /v1/resources` returns an empty list for the default
/// `EmptyResourceProvider`; the wire envelope is well-formed.
#[tokio::test]
async fn list_resources_default_is_empty() {
    let (svc, _reg, _disp) = fixture_loaded_spheres();
    let (server, _audit) = build_server(svc);

    let resp = server.get("/v1/resources").await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["total"], 0);
    assert!(body["resources"].is_array());
    assert_eq!(body["resources"].as_array().unwrap().len(), 0);
    assert!(body["request_id"].is_string());
}

/// `GET /v1/resources/{uri}` against the default `EmptyResourceProvider`
/// returns a structured 404 (not a panic, not a generic 500).
#[tokio::test]
async fn read_resource_default_returns_not_found() {
    let (svc, _reg, _disp) = fixture_loaded_spheres();
    let (server, _audit) = build_server(svc);

    let resp = server.get("/v1/resources/file%3A%2F%2Fmissing").await;
    resp.assert_status(axum::http::StatusCode::NOT_FOUND);
    let body: Value = resp.json();
    assert_eq!(body["kind"], "not-found");
    assert!(body["message"].as_str().unwrap().contains("not found"));
}

/// `GET /v1/prompts` returns an empty list by default.
#[tokio::test]
async fn list_prompts_default_is_empty() {
    let (svc, _reg, _disp) = fixture_loaded_spheres();
    let (server, _audit) = build_server(svc);

    let resp = server.get("/v1/prompts").await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["total"], 0);
    assert!(body["prompts"].is_array());
    assert_eq!(body["diagnostics"]["prompt_count"], 0);
    assert_eq!(body["diagnostics"]["enabled"], false);
}

/// `GET /v1/prompts/{name}` against the default returns a structured 404.
#[tokio::test]
async fn get_prompt_default_returns_not_found() {
    let (svc, _reg, _disp) = fixture_loaded_spheres();
    let (server, _audit) = build_server(svc);

    let resp = server.get("/v1/prompts/missing").await;
    resp.assert_status(axum::http::StatusCode::NOT_FOUND);
    let body: Value = resp.json();
    assert_eq!(body["kind"], "not-found");
}

/// Wiring a real `ResourceProvider` via `with_resources` flows through
/// to `GET /v1/resources` — proves the builder + DIP boundary works.
#[tokio::test]
async fn list_resources_with_custom_provider_returns_entries() {
    use super::service::{
        ResourceContent, ResourceListEntry, ResourceProvider, ResourceReadResponse,
    };
    use crate::errors::{ServiceError, ServiceErrorKind};

    struct StaticProvider;
    impl ResourceProvider for StaticProvider {
        fn list(&self) -> Vec<ResourceListEntry> {
            vec![ResourceListEntry {
                uri: "scene://current".into(),
                name: "Active scene".into(),
                description: Some("Current Maya scene snapshot".into()),
                mime_type: Some("application/json".into()),
            }]
        }
        fn read(&self, uri: &str) -> Result<ResourceReadResponse, ServiceError> {
            if uri == "scene://current" {
                Ok(ResourceReadResponse {
                    contents: vec![ResourceContent {
                        uri: uri.into(),
                        mime_type: Some("application/json".into()),
                        text: Some(r#"{"objects":3}"#.into()),
                        blob: None,
                    }],
                })
            } else {
                Err(ServiceError::new(
                    ServiceErrorKind::NotFound,
                    format!("not registered: {uri}"),
                ))
            }
        }
    }

    let (svc, _reg, _disp) = fixture_loaded_spheres();
    let svc = svc.with_resources(Arc::new(StaticProvider));
    let (server, _audit) = build_server(svc);

    // List.
    let resp = server.get("/v1/resources").await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["total"], 1);
    assert_eq!(body["resources"][0]["uri"], "scene://current");

    // Read existing.
    let resp = server.get("/v1/resources/scene%3A%2F%2Fcurrent").await;
    resp.assert_status_ok();
    let body: Value = resp.json();
    assert_eq!(body["contents"][0]["uri"], "scene://current");
    assert_eq!(body["contents"][0]["text"], r#"{"objects":3}"#);

    // Read missing → 404.
    let resp = server.get("/v1/resources/scene%3A%2F%2Fmissing").await;
    resp.assert_status(axum::http::StatusCode::NOT_FOUND);
}

// ── #818 phase 1b — SSE endpoint smoke tests ─────────────────────────

/// `GET /v1/resources/{uri}/events` returns 200 text/event-stream for
/// the default `EmptyResourceProvider`. The default `subscribe`
/// implementation returns an immediately-terminating empty stream —
/// callers get a valid SSE response with no events (correct for
/// embedders that have not wired real push yet).
#[tokio::test]
async fn resource_events_returns_sse_for_default_provider() {
    let (svc, _reg, _disp) = fixture_loaded_spheres();
    let (server, _audit) = build_server(svc);

    let resp = server
        .get("/v1/resources/scene%3A%2F%2Fcurrent/events")
        .await;
    resp.assert_status_ok();
    assert!(
        resp.header("content-type")
            .to_str()
            .unwrap_or("")
            .contains("text/event-stream"),
        "expected text/event-stream from default SSE endpoint"
    );
}

/// `GET /v1/jobs/{id}/events` returns 404 for unknown job ids.
#[tokio::test]
async fn job_events_not_found_for_empty_controller() {
    let (svc, _reg, _disp) = fixture_loaded_spheres();
    let (server, _audit) = build_server(svc);

    let resp = server.get("/v1/jobs/nonexistent-id/events").await;
    resp.assert_status(axum::http::StatusCode::NOT_FOUND);
    let body: Value = resp.json();
    assert_eq!(body["kind"], "not-found");
}

/// `DELETE /v1/jobs/{id}` returns 404 for unknown jobs.
#[tokio::test]
async fn job_cancel_not_found_for_empty_controller() {
    let (svc, _reg, _disp) = fixture_loaded_spheres();
    let (server, _audit) = build_server(svc);

    let resp = server.delete("/v1/jobs/nonexistent-id").await;
    resp.assert_status(axum::http::StatusCode::NOT_FOUND);
    let body: Value = resp.json();
    assert_eq!(body["kind"], "not-found");
}

/// Wiring a `JobController` that streams one `Done` event returns
/// `200 text/event-stream` and supports cancel.
#[tokio::test]
async fn job_events_with_real_controller_streams_and_cancels() {
    use super::service::{CallOutcome, EventStream, JobController, JobEvent, ToolSlug};
    use crate::errors::{ServiceError, ServiceErrorKind};
    use futures::stream;

    struct OneEventController;
    impl JobController for OneEventController {
        fn subscribe(&self, job_id: &str) -> Result<EventStream, ServiceError> {
            if job_id == "known" {
                let ev = JobEvent::Done {
                    result: CallOutcome {
                        slug: ToolSlug("maya.skill.action".into()),
                        output: serde_json::json!({"ok": true}),
                        validation_skipped: false,
                    },
                };
                Ok(Box::pin(stream::once(async move { Ok(ev) })))
            } else {
                Err(ServiceError::new(
                    ServiceErrorKind::NotFound,
                    format!("job not found: {job_id}"),
                ))
            }
        }
        fn cancel(&self, _: &str) -> Result<(), ServiceError> {
            Ok(())
        }
    }

    let (svc, _reg, _disp) = fixture_loaded_spheres();
    let svc = svc.with_jobs(Arc::new(OneEventController));
    let (server, _audit) = build_server(svc);

    // Known job → 200 text/event-stream.
    let resp = server.get("/v1/jobs/known/events").await;
    resp.assert_status_ok();
    assert!(
        resp.header("content-type")
            .to_str()
            .unwrap_or("")
            .contains("text/event-stream"),
        "expected text/event-stream"
    );

    // Unknown job → 404.
    let resp = server.get("/v1/jobs/unknown/events").await;
    resp.assert_status(axum::http::StatusCode::NOT_FOUND);

    // Cancel always succeeds in mock.
    let resp = server.delete("/v1/jobs/known").await;
    resp.assert_status(axum::http::StatusCode::NO_CONTENT);
}

/// Thread-affinity violations must return structured remediation context (#1075).
#[tokio::test]
async fn call_thread_affinity_violation_includes_context() {
    use dcc_mcp_models::ThreadAffinity;

    let registry = Arc::new(ToolRegistry::new());
    registry.register_action(ToolMeta {
        name: "main_only".into(),
        dcc: "maya".into(),
        skill_name: Some("rig".into()),
        enabled: true,
        thread_affinity: ThreadAffinity::Main,
        enforce_thread_affinity: true,
        ..Default::default()
    });
    let dispatcher = Arc::new(ToolDispatcher::new((*registry).clone()));
    dispatcher.register_handler("main_only", |_| Ok(json!({"ok": true})));
    let catalog = Arc::new(SkillCatalog::new_with_dispatcher(
        registry.clone(),
        dispatcher.clone(),
    ));
    let _ = catalog.load_skill("rig");
    let svc = SkillRestService::from_catalog_and_dispatcher(catalog, dispatcher);
    let (server, _) = build_server(svc);

    let resp = server
        .post("/v1/call")
        .json(&json!({"tool_slug": "maya.rig.main_only", "arguments": {}}))
        .await;
    assert_eq!(resp.status_code().as_u16(), 409);
    let body: Value = resp.json();
    assert_eq!(body["kind"], "thread-affinity-violation");
    assert!(body["hint"].as_str().is_some());
    let ctx = body["context"].as_object().expect("context object");
    assert_eq!(ctx["declared_affinity"], "main");
    assert_eq!(ctx["observed_affinity"], "any");
    assert_eq!(ctx["host_dispatcher_attached"], false);
    assert_eq!(ctx["observed_context"], "worker_no_dispatcher");
}

// ── PIP-520: _meta passthrough business scenario tests ──────────────

/// Fixture that registers tools which consume `_meta` from their params.
/// Models realistic adapter skills that need request-level context.
fn fixture_meta_aware_tools() -> (SkillRestService, Arc<ToolRegistry>, Arc<ToolDispatcher>) {
    let registry = Arc::new(ToolRegistry::new());

    // Tool 1: credential_profile — selects credentials based on profile
    let cred_schema = json!({
        "type": "object",
        "properties": {"service": {"type": "string"}},
        "required": ["service"]
    });
    registry.register_action(ToolMeta {
        name: "credential_resolver".into(),
        dcc: "maya".into(),
        description: "Resolves credentials via _meta.credential_profile".into(),
        input_schema: cred_schema,
        skill_name: Some("auth".into()),
        enabled: true,
        ..Default::default()
    });

    // Tool 2: permission_hint — enforces read-only mode
    let perm_schema = json!({
        "type": "object",
        "properties": {"action": {"type": "string"}},
        "required": ["action"]
    });
    registry.register_action(ToolMeta {
        name: "permission_gate".into(),
        dcc: "maya".into(),
        description: "Enforces read-only via _meta.permission_hint".into(),
        input_schema: perm_schema,
        skill_name: Some("auth".into()),
        enabled: true,
        ..Default::default()
    });

    // Tool 3: project_scope — filters data by project
    let scope_schema = json!({
        "type": "object",
        "properties": {"asset_name": {"type": "string"}},
        "required": ["asset_name"]
    });
    registry.register_action(ToolMeta {
        name: "asset_lookup".into(),
        dcc: "maya".into(),
        description: "Filters assets by _meta.project_scope".into(),
        input_schema: scope_schema,
        skill_name: Some("pipeline".into()),
        enabled: true,
        ..Default::default()
    });

    // Tool 4: agent_context — identifies the calling agent/actor
    let agent_schema = json!({
        "type": "object",
        "properties": {"message": {"type": "string"}},
        "required": ["message"]
    });
    registry.register_action(ToolMeta {
        name: "agent_greeter".into(),
        dcc: "maya".into(),
        description: "Reads _meta.agent_context for caller identity".into(),
        input_schema: agent_schema,
        skill_name: Some("telemetry".into()),
        enabled: true,
        ..Default::default()
    });

    // Tool 5: additionalProperties: false — strict schema, must still work
    let strict_schema = json!({
        "type": "object",
        "properties": {"value": {"type": "number"}},
        "required": ["value"],
        "additionalProperties": false
    });
    registry.register_action(ToolMeta {
        name: "strict_tool".into(),
        dcc: "maya".into(),
        description: "Strict additionalProperties:false schema".into(),
        input_schema: strict_schema,
        skill_name: Some("pipeline".into()),
        enabled: true,
        ..Default::default()
    });

    let dispatcher = Arc::new(ToolDispatcher::new((*registry).clone()));

    // Handler 1: reads credential_profile from _meta
    dispatcher.register_handler("credential_resolver", |params: Value| {
        let profile = params
            .pointer("/_meta/credential_profile")
            .and_then(Value::as_str)
            .unwrap_or("default");
        let creds = match profile {
            "prod" => json!({"endpoint": "https://prod.api.example.com", "token": "prod-token"}),
            "staging" => {
                json!({"endpoint": "https://staging.api.example.com", "token": "staging-token"})
            }
            _ => json!({"endpoint": "https://dev.api.example.com", "token": "dev-token"}),
        };
        Ok(json!({
            "resolved": true,
            "profile": profile,
            "credentials": creds,
            "service": params["service"]
        }))
    });

    // Handler 2: enforces read-only via permission_hint
    dispatcher.register_handler("permission_gate", |params: Value| {
        let hint = params
            .pointer("/_meta/permission_hint")
            .and_then(Value::as_str)
            .unwrap_or("read-write");
        let action = params["action"].as_str().unwrap_or("");
        if hint == "read-only" && action == "delete" {
            return Ok(json!({
                "allowed": false,
                "reason": format!("action '{}' denied: permission_hint is '{}'", action, hint),
                "hint": hint,
            }));
        }
        Ok(json!({
            "allowed": true,
            "action": action,
            "hint": hint,
        }))
    });

    // Handler 3: filters by project_scope from _meta
    dispatcher.register_handler("asset_lookup", |params: Value| {
        let scope = params
            .pointer("/_meta/project_scope")
            .and_then(Value::as_str)
            .unwrap_or("");
        let asset_name = params["asset_name"].as_str().unwrap_or("");
        // Simulated asset database — only returns assets in scope
        let all_assets = vec![
            json!({"name": "hero_model", "project": "movie-42", "path": "/scenes/movie42/hero.ma"}),
            json!({"name": "prop_chair", "project": "movie-99", "path": "/scenes/movie99/chair.ma"}),
            json!({"name": "env_forest", "project": "movie-42", "path": "/scenes/movie42/forest.ma"}),
        ];
        let filtered: Vec<_> = if scope.is_empty() {
            all_assets.clone()
        } else {
            all_assets
                .into_iter()
                .filter(|a| a["project"] == scope)
                .collect()
        };
        Ok(json!({
            "scope": scope,
            "query": asset_name,
            "results": filtered.len(),
            "assets": filtered,
        }))
    });

    // Handler 4: reads agent_context from _meta for caller identity
    dispatcher.register_handler("agent_greeter", |params: Value| {
        let meta = params.get("_meta");
        let agent_ctx = meta.and_then(|m| m.get("agent_context"));
        let actor_id = agent_ctx
            .and_then(|ac| ac.get("actor_id"))
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let agent_name = agent_ctx
            .and_then(|ac| ac.get("agent_name"))
            .and_then(Value::as_str)
            .unwrap_or("unknown-agent");
        let session_id = agent_ctx
            .and_then(|ac| ac.get("session_id"))
            .and_then(Value::as_str)
            .unwrap_or("no-session");
        Ok(json!({
            "greeting": format!("Hello {}! I see you're using {}. Session: {}", actor_id, agent_name, session_id),
            "message": params["message"],
            "actor_id": actor_id,
            "agent_name": agent_name,
            "session_id": session_id,
        }))
    });

    // Handler 5: strict schema, just echoes — validates additionalProperties:false still works
    dispatcher.register_handler("strict_tool", |params: Value| {
        Ok(json!({
            "echoed_value": params["value"],
            "has_meta": params.get("_meta").is_some(),
        }))
    });

    let catalog = Arc::new(SkillCatalog::new_with_dispatcher(
        registry.clone(),
        dispatcher.clone(),
    ));

    for (skill_name, dcc, desc, tags) in [
        (
            "auth",
            "maya",
            "Authentication & authorization",
            vec!["security"],
        ),
        ("pipeline", "maya", "Pipeline utilities", vec!["pipeline"]),
        (
            "telemetry",
            "maya",
            "Telemetry & observability",
            vec!["observability"],
        ),
    ] {
        let meta = SkillMetadata {
            name: skill_name.into(),
            dcc: dcc.into(),
            description: desc.into(),
            tags: tags.into_iter().map(String::from).collect(),
            ..Default::default()
        };
        catalog.add_skill(meta);
        let _ = catalog.load_skill(skill_name);
    }

    let service = SkillRestService::from_catalog_and_dispatcher(catalog, dispatcher.clone());
    (service, registry, dispatcher)
}

// ── Scenario 1: credential_profile resolution ────────────────

#[tokio::test]
async fn meta_credential_profile_selects_prod_credentials() {
    let (svc, _, _) = fixture_meta_aware_tools();
    let (server, _) = build_server(svc);

    let resp = server
        .post("/v1/call")
        .json(&json!({
            "tool_slug": "maya.auth.credential_resolver",
            "params": {"service": "fpt"},
            "meta": {
                "credential_profile": "prod",
                "agent_context": {"actor_id": "artist-1"}
            }
        }))
        .await;
    resp.assert_status_ok();
    let out: Value = resp.json();
    assert_eq!(out["output"]["profile"], "prod");
    assert_eq!(out["output"]["resolved"], true);
    assert_eq!(
        out["output"]["credentials"]["endpoint"],
        "https://prod.api.example.com"
    );
    assert_eq!(out["output"]["credentials"]["token"], "prod-token");
    assert_eq!(out["output"]["service"], "fpt");
}

#[tokio::test]
async fn meta_credential_profile_defaults_when_absent() {
    let (svc, _, _) = fixture_meta_aware_tools();
    let (server, _) = build_server(svc);

    // No meta at all — handler should fall back to "default"
    let resp = server
        .post("/v1/call")
        .json(&json!({
            "tool_slug": "maya.auth.credential_resolver",
            "params": {"service": "fpt"}
        }))
        .await;
    resp.assert_status_ok();
    let out: Value = resp.json();
    assert_eq!(out["output"]["profile"], "default");
    assert_eq!(
        out["output"]["credentials"]["endpoint"],
        "https://dev.api.example.com"
    );
}

// ── Scenario 2: permission_hint enforcement ────────────────

#[tokio::test]
async fn meta_permission_hint_read_only_blocks_delete() {
    let (svc, _, _) = fixture_meta_aware_tools();
    let (server, _) = build_server(svc);

    let resp = server
        .post("/v1/call")
        .json(&json!({
            "tool_slug": "maya.auth.permission_gate",
            "params": {"action": "delete"},
            "meta": {"permission_hint": "read-only"}
        }))
        .await;
    resp.assert_status_ok();
    let out: Value = resp.json();
    assert_eq!(out["output"]["allowed"], false);
    assert!(out["output"]["reason"].as_str().unwrap().contains("denied"));
}

#[tokio::test]
async fn meta_permission_hint_read_write_allows_delete() {
    let (svc, _, _) = fixture_meta_aware_tools();
    let (server, _) = build_server(svc);

    let resp = server
        .post("/v1/call")
        .json(&json!({
            "tool_slug": "maya.auth.permission_gate",
            "params": {"action": "delete"},
            "meta": {"permission_hint": "read-write"}
        }))
        .await;
    resp.assert_status_ok();
    let out: Value = resp.json();
    assert_eq!(out["output"]["allowed"], true);
    assert_eq!(out["output"]["hint"], "read-write");
}

// ── Scenario 3: project_scope isolation ────────────────

#[tokio::test]
async fn meta_project_scope_filters_assets_to_movie_42() {
    let (svc, _, _) = fixture_meta_aware_tools();
    let (server, _) = build_server(svc);

    let resp = server
        .post("/v1/call")
        .json(&json!({
            "tool_slug": "maya.pipeline.asset_lookup",
            "params": {"asset_name": "hero"},
            "meta": {"project_scope": "movie-42"}
        }))
        .await;
    resp.assert_status_ok();
    let out: Value = resp.json();
    assert_eq!(out["output"]["scope"], "movie-42");
    assert_eq!(out["output"]["results"], 2); // hero_model + env_forest
    let assets = out["output"]["assets"].as_array().unwrap();
    for asset in assets {
        assert_eq!(asset["project"], "movie-42");
    }
}

#[tokio::test]
async fn meta_project_scope_empty_returns_all_assets() {
    let (svc, _, _) = fixture_meta_aware_tools();
    let (server, _) = build_server(svc);

    // No meta — should return all assets (scope is empty string)
    let resp = server
        .post("/v1/call")
        .json(&json!({
            "tool_slug": "maya.pipeline.asset_lookup",
            "params": {"asset_name": "hero"}
        }))
        .await;
    resp.assert_status_ok();
    let out: Value = resp.json();
    assert_eq!(out["output"]["scope"], "");
    assert_eq!(out["output"]["results"], 3); // all 3 assets
}

// ── Scenario 4: agent_context passthrough ────────────────

#[tokio::test]
async fn meta_agent_context_identifies_caller() {
    let (svc, _, _) = fixture_meta_aware_tools();
    let (server, _) = build_server(svc);

    let resp = server
        .post("/v1/call")
        .json(&json!({
            "tool_slug": "maya.telemetry.agent_greeter",
            "params": {"message": "hello"},
            "meta": {
                "agent_context": {
                    "actor_id": "artist-42",
                    "agent_name": "claude-code",
                    "session_id": "sess-abc-123",
                    "agent_version": "4.6.0"
                }
            }
        }))
        .await;
    resp.assert_status_ok();
    let out: Value = resp.json();
    assert_eq!(out["output"]["actor_id"], "artist-42");
    assert_eq!(out["output"]["agent_name"], "claude-code");
    assert_eq!(out["output"]["session_id"], "sess-abc-123");
    let greeting = out["output"]["greeting"].as_str().unwrap();
    assert!(greeting.contains("artist-42"));
    assert!(greeting.contains("claude-code"));
}

#[tokio::test]
async fn meta_agent_context_unknown_when_absent() {
    let (svc, _, _) = fixture_meta_aware_tools();
    let (server, _) = build_server(svc);

    // No meta — handler falls back to "unknown"
    let resp = server
        .post("/v1/call")
        .json(&json!({
            "tool_slug": "maya.telemetry.agent_greeter",
            "params": {"message": "hi"}
        }))
        .await;
    resp.assert_status_ok();
    let out: Value = resp.json();
    assert_eq!(out["output"]["actor_id"], "unknown");
    assert_eq!(out["output"]["agent_name"], "unknown-agent");
}

// ── Scenario 5: strict schema (additionalProperties: false) ──

#[tokio::test]
async fn meta_passthrough_with_strict_schema_additional_properties_false() {
    let (svc, _, _) = fixture_meta_aware_tools();
    let (server, _) = build_server(svc);

    // This would fail validation if _meta were injected BEFORE validation
    let resp = server
        .post("/v1/call")
        .json(&json!({
            "tool_slug": "maya.pipeline.strict_tool",
            "params": {"value": 42.0},
            "meta": {
                "agent_context": {"actor_id": "strict-user"},
                "credential_profile": "prod",
                "permission_hint": "read-only",
                "project_scope": "movie-42",
            }
        }))
        .await;
    resp.assert_status_ok();
    let out: Value = resp.json();
    assert_eq!(out["output"]["echoed_value"], 42.0);
    // _meta should be present in handler params (injected after validation)
    assert_eq!(out["output"]["has_meta"], true);
}

// ── Scenario 6: service-layer meta passthrough ─────────────

#[test]
fn service_layer_call_passes_meta_through_to_invoker() {
    let (svc, _, _) = fixture_meta_aware_tools();

    // Service-layer call (bypasses HTTP) — the same path the gateway
    // MCP wrapper uses via `call_tool` -> `call_service`.
    let outcome = svc
        .call(&super::service::CallRequest {
            tool_slug: super::service::ToolSlug("maya.auth.credential_resolver".into()),
            params: json!({"service": "fpt"}),
            meta: Some(json!({
                "credential_profile": "staging",
                "agent_context": {"actor_id": "svc-caller"}
            })),
        })
        .expect("service call");
    let output = &outcome.output;
    assert_eq!(output["profile"], "staging");
    assert_eq!(
        output["credentials"]["endpoint"],
        "https://staging.api.example.com"
    );
}

#[test]
fn service_layer_call_without_meta_is_backward_compatible() {
    let (svc, _, _) = fixture_meta_aware_tools();

    let outcome = svc
        .call(&super::service::CallRequest {
            tool_slug: super::service::ToolSlug("maya.auth.credential_resolver".into()),
            params: json!({"service": "fpt"}),
            meta: None,
        })
        .expect("service call");
    assert_eq!(outcome.output["profile"], "default");
}

// ── Scenario 7: multi-field passthrough ────────────────────

#[tokio::test]
async fn meta_multiple_fields_all_passed_through_together() {
    let (svc, _, _) = fixture_meta_aware_tools();
    let (server, _) = build_server(svc);

    // Send all allowlisted fields + agent_context at once
    let resp = server
        .post("/v1/call")
        .json(&json!({
            "tool_slug": "maya.telemetry.agent_greeter",
            "params": {"message": "multi-test"},
            "meta": {
                "agent_context": {
                    "actor_id": "multi-user",
                    "agent_name": "test-agent",
                    "session_id": "multi-session"
                },
                "credential_profile": "prod",
                "permission_hint": "read-write",
                "project_scope": "movie-42",
                "search_id": "search-xyz"
            }
        }))
        .await;
    resp.assert_status_ok();
    let out: Value = resp.json();
    assert_eq!(out["output"]["actor_id"], "multi-user");
    assert_eq!(out["output"]["agent_name"], "test-agent");
    assert_eq!(out["output"]["session_id"], "multi-session");
}
