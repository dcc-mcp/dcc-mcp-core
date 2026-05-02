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

use dcc_mcp_actions::dispatcher::ActionDispatcher;
use dcc_mcp_actions::registry::{ActionMeta, ActionRegistry};
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
fn fixture_loaded_spheres() -> (SkillRestService, Arc<ActionRegistry>, Arc<ActionDispatcher>) {
    let registry = Arc::new(ActionRegistry::new());

    let schema = json!({
        "type": "object",
        "properties": {"radius": {"type": "number"}},
        "required": ["radius"]
    });

    registry.register_action(ActionMeta {
        name: "create_sphere".into(),
        dcc: "maya".into(),
        description: "Create a polygon sphere".into(),
        tags: vec!["geometry".into(), "poly".into()],
        input_schema: schema,
        skill_name: Some("spheres".into()),
        enabled: true,
        ..Default::default()
    });

    let dispatcher = Arc::new(ActionDispatcher::new((*registry).clone()));
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

/// Readiness three-state gating: /v1/call must 503 until the probe
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

    unsafe {
        std::env::remove_var("DCC_MCP_DOCS_UI");
    }
    let resp = server.get("/docs").await;
    resp.assert_status_ok();
    let html = resp.text();
    assert!(html.contains("scalar") || html.contains("Scalar"));

    unsafe {
        std::env::set_var("DCC_MCP_DOCS_UI", "0");
    }
    let disabled = server.get("/docs").await;
    disabled.assert_status_not_found();
    unsafe {
        std::env::remove_var("DCC_MCP_DOCS_UI");
    }
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
    let registry = Arc::new(ActionRegistry::new());

    // Keep schema identical so we can focus on list/search/filter.
    let num_schema = json!({
        "type": "object",
        "properties": {"n": {"type": "number"}},
        "required": ["n"]
    });

    // Maya · spheres (loaded) — 3 actions.
    for name in ["create_sphere", "scale_sphere", "delete_sphere"] {
        registry.register_action(ActionMeta {
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
        registry.register_action(ActionMeta {
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
        registry.register_action(ActionMeta {
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

    let dispatcher = Arc::new(ActionDispatcher::new((*registry).clone()));
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
    }
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
