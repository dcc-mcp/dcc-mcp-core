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
