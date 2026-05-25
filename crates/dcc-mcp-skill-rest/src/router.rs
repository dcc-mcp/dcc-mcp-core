//! Axum router that exposes the [`super::service::SkillRestService`].
//!
//! Each handler is a thin adapter: parse → auth → delegate to the
//! service → wrap in a response. Keeping the adapters tiny means the
//! SOLID service layer is the only thing tests exercise through axum.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{
        Html, IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{delete, get, post},
};
use chrono::Utc;
use futures::StreamExt;
use serde_json::{Value, json};

use super::audit::{AuditEvent, AuditOutcome, AuditSink, NoopAuditSink};
use super::auth::{AllowLocalhostGate, AuthContext, AuthGate, Principal};
use super::errors::{ServiceError, ServiceErrorKind};
use super::openapi::build_openapi_document;
use super::readiness::{ReadinessProbe, StaticReadiness};
use super::service::{
    CallOutcome, CallRequest, ContextSnapshot, DescribeRequest, DescribeResponse, LoadSkillRequest,
    PromptGetResponse, ResourceReadResponse, SearchRequest, SearchResponse, SkillLifecycleResponse,
    SkillListEntry, SkillRestService, ToolSlug, UnloadSkillRequest,
};

/// Runtime configuration for the REST surface.
#[derive(Clone)]
pub struct SkillRestConfig {
    pub service: SkillRestService,
    pub auth: Arc<dyn AuthGate>,
    pub audit: Arc<dyn AuditSink>,
    pub readiness: Arc<dyn ReadinessProbe>,
    pub server_title: String,
    pub server_version: String,
}

impl SkillRestConfig {
    /// Minimal config — defaults everything except the service.
    pub fn new(service: SkillRestService) -> Self {
        Self {
            service,
            auth: Arc::new(AllowLocalhostGate::new()),
            audit: Arc::new(NoopAuditSink),
            readiness: Arc::new(StaticReadiness::fully_ready()),
            server_title: "dcc-mcp-http".into(),
            server_version: env!("CARGO_PKG_VERSION").into(),
        }
    }

    pub fn with_auth(mut self, auth: Arc<dyn AuthGate>) -> Self {
        self.auth = auth;
        self
    }
    pub fn with_audit(mut self, audit: Arc<dyn AuditSink>) -> Self {
        self.audit = audit;
        self
    }
    pub fn with_readiness(mut self, probe: Arc<dyn ReadinessProbe>) -> Self {
        self.readiness = probe;
        self
    }
}

/// Build a [`Router`] that mounts the entire `/v1/*` surface.
///
/// The caller is free to `.merge()` this into their existing MCP
/// router so both live on the same listener.
pub fn build_skill_rest_router(config: SkillRestConfig) -> Router {
    Router::new()
        .route("/v1/healthz", get(handle_healthz))
        .route("/v1/readyz", get(handle_readyz))
        .route("/v1/openapi.json", get(handle_openapi))
        .route("/docs", get(handle_docs))
        .route("/v1/skills", get(handle_list_skills))
        .route("/v1/search", post(handle_search))
        .route("/v1/load_skill", post(handle_load_skill))
        .route("/v1/unload_skill", post(handle_unload_skill))
        .route("/v1/describe", post(handle_describe))
        .route("/v1/tools/{slug}", get(handle_describe_path))
        .route("/v1/call", post(handle_call))
        .route("/v1/dcc/{dcc_type}/call", post(handle_dcc_backend_call))
        .route("/v1/context", get(handle_context))
        // ── #818 phase 1 — resources & prompts as REST ────────────
        .route("/v1/resources", get(handle_list_resources))
        .route("/v1/resources/{uri}", get(handle_read_resource))
        .route("/v1/prompts", get(handle_list_prompts))
        .route("/v1/prompts/{name}", get(handle_get_prompt))
        // ── #818 phase 1b — SSE streams & job cancel ──────────────
        .route("/v1/resources/{uri}/events", get(handle_resource_events))
        .route("/v1/jobs/{id}/events", get(handle_job_events))
        .route("/v1/jobs/{id}", delete(handle_job_cancel))
        .with_state(config)
}

// ── Auth wrapper ─────────────────────────────────────────────────────

fn principal_or_error(
    cfg: &SkillRestConfig,
    peer: Option<SocketAddr>,
    headers: &HeaderMap,
    request_id: &str,
) -> Result<Principal, Box<Response>> {
    let auth_header = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());
    let ctx = AuthContext {
        peer,
        authorization: auth_header,
        request_id: Some(request_id),
    };
    cfg.auth
        .authorize(&ctx)
        .map_err(|err| Box::new(service_error_to_response(err.with_request_id(request_id))))
}

fn request_id(headers: &HeaderMap) -> String {
    headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
}

fn service_error_to_response(err: ServiceError) -> Response {
    let status =
        StatusCode::from_u16(err.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (
        status,
        Json(serde_json::to_value(&err).unwrap_or_else(|_| json!({}))),
    )
        .into_response()
}

fn emit_audit(
    cfg: &SkillRestConfig,
    request_id: &str,
    slug: &str,
    route: &str,
    subject: &str,
    outcome: AuditOutcome,
    started: std::time::Instant,
) {
    cfg.audit.record(AuditEvent {
        request_id: request_id.to_owned(),
        at: Utc::now(),
        slug: slug.to_owned(),
        route: route.to_owned(),
        subject: subject.to_owned(),
        outcome,
        duration_ms: started.elapsed().as_millis() as u64,
    });
}

// ── Handlers ─────────────────────────────────────────────────────────

async fn handle_healthz(State(_cfg): State<SkillRestConfig>) -> impl IntoResponse {
    (StatusCode::OK, Json(json!({"ok": true})))
}

async fn handle_readyz(State(cfg): State<SkillRestConfig>) -> impl IntoResponse {
    let report = cfg.readiness.report();
    let status = if report.is_ready() {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (status, Json(serde_json::to_value(&report).unwrap()))
}

async fn handle_openapi(State(cfg): State<SkillRestConfig>) -> impl IntoResponse {
    let doc = build_openapi_document(&cfg.server_title, &cfg.server_version);
    (StatusCode::OK, Json(doc))
}

async fn handle_docs(State(cfg): State<SkillRestConfig>) -> Response {
    if std::env::var("DCC_MCP_DOCS_UI").is_ok_and(|value| value == "0") {
        return StatusCode::NOT_FOUND.into_response();
    }
    let html = super::openapi::build_docs_html(&cfg.server_title, &cfg.server_version);
    (StatusCode::OK, Html(html)).into_response()
}

/// Pull a best-effort peer `SocketAddr` out of the request headers.
///
/// `axum::extract::ConnectInfo` requires the server to be started with
/// `into_make_service_with_connect_info`, which is not something a
/// library can force on its embedder. Instead we consult — in order —
/// `X-Forwarded-For`, `X-Real-IP`, and `Forwarded`. Absent all three
/// we return `None` and let [`AllowLocalhostGate`] decide (it allows
/// `None` peers, preserving the test-harness contract).
fn peer(headers: &HeaderMap) -> Option<SocketAddr> {
    let candidate = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .or_else(|| headers.get("x-real-ip").and_then(|v| v.to_str().ok()))
        .map(str::trim)?;
    // Peer header is usually just an IP; synthesise a port of 0 so the
    // auth gate can still reason about loopback vs remote.
    let ip: std::net::IpAddr = candidate.parse().ok()?;
    Some(SocketAddr::new(ip, 0))
}

async fn handle_list_skills(State(cfg): State<SkillRestConfig>, headers: HeaderMap) -> Response {
    let rid = request_id(&headers);
    let principal = match principal_or_error(&cfg, peer(&headers), &headers, &rid) {
        Ok(p) => p,
        Err(r) => return *r,
    };
    let started = std::time::Instant::now();
    let entries = cfg.service.list_skills(true);
    emit_audit(
        &cfg,
        &rid,
        "",
        "GET /v1/skills",
        &principal.subject,
        AuditOutcome::Success,
        started,
    );
    (
        StatusCode::OK,
        Json(json!({"total": entries.len(), "skills": entries, "request_id": rid})),
    )
        .into_response()
}

async fn handle_search(
    State(cfg): State<SkillRestConfig>,
    headers: HeaderMap,
    body: Option<Json<SearchRequest>>,
) -> Response {
    let rid = request_id(&headers);
    let principal = match principal_or_error(&cfg, peer(&headers), &headers, &rid) {
        Ok(p) => p,
        Err(r) => return *r,
    };
    let req = body.map(|Json(r)| r).unwrap_or_default();
    let started = std::time::Instant::now();
    let resp = cfg.service.search(&req);
    let total = resp.total;
    emit_audit(
        &cfg,
        &rid,
        "",
        "POST /v1/search",
        &principal.subject,
        AuditOutcome::Success,
        started,
    );
    (
        StatusCode::OK,
        Json(json!({
            "total": total,
            "hits": resp.hits,
            "request_id": rid,
        })),
    )
        .into_response()
}

async fn handle_load_skill(
    State(cfg): State<SkillRestConfig>,
    headers: HeaderMap,
    Json(req): Json<LoadSkillRequest>,
) -> Response {
    let rid = request_id(&headers);
    let principal = match principal_or_error(&cfg, peer(&headers), &headers, &rid) {
        Ok(p) => p,
        Err(r) => return *r,
    };
    let started = std::time::Instant::now();
    match cfg.service.load_skill(&req) {
        Ok(resp) => {
            emit_audit(
                &cfg,
                &rid,
                &req.skill_name,
                "POST /v1/load_skill",
                &principal.subject,
                AuditOutcome::Success,
                started,
            );
            (
                StatusCode::OK,
                Json(json!({
                    "success": true,
                    "skill_name": resp.skill_name,
                    "actions": resp.actions,
                    "request_id": rid,
                })),
            )
                .into_response()
        }
        Err(err) => {
            emit_audit(
                &cfg,
                &rid,
                &req.skill_name,
                "POST /v1/load_skill",
                &principal.subject,
                AuditOutcome::Failure(
                    serde_json::to_value(err.kind)
                        .ok()
                        .and_then(|v| v.as_str().map(ToOwned::to_owned))
                        .unwrap_or_else(|| "internal".into()),
                ),
                started,
            );
            service_error_to_response(err.with_request_id(rid))
        }
    }
}

async fn handle_unload_skill(
    State(cfg): State<SkillRestConfig>,
    headers: HeaderMap,
    Json(req): Json<UnloadSkillRequest>,
) -> Response {
    let rid = request_id(&headers);
    let principal = match principal_or_error(&cfg, peer(&headers), &headers, &rid) {
        Ok(p) => p,
        Err(r) => return *r,
    };
    let started = std::time::Instant::now();
    match cfg.service.unload_skill(&req) {
        Ok(resp) => {
            emit_audit(
                &cfg,
                &rid,
                &req.skill_name,
                "POST /v1/unload_skill",
                &principal.subject,
                AuditOutcome::Success,
                started,
            );
            (
                StatusCode::OK,
                Json(json!({
                    "success": true,
                    "skill_name": resp.skill_name,
                    "removed": resp.removed.unwrap_or(0),
                    "request_id": rid,
                })),
            )
                .into_response()
        }
        Err(err) => {
            emit_audit(
                &cfg,
                &rid,
                &req.skill_name,
                "POST /v1/unload_skill",
                &principal.subject,
                AuditOutcome::Failure(
                    serde_json::to_value(err.kind)
                        .ok()
                        .and_then(|v| v.as_str().map(ToOwned::to_owned))
                        .unwrap_or_else(|| "internal".into()),
                ),
                started,
            );
            service_error_to_response(err.with_request_id(rid))
        }
    }
}

async fn handle_describe(
    State(cfg): State<SkillRestConfig>,
    headers: HeaderMap,
    Json(req): Json<DescribeRequest>,
) -> Response {
    let rid = request_id(&headers);
    let principal = match principal_or_error(&cfg, peer(&headers), &headers, &rid) {
        Ok(p) => p,
        Err(r) => return *r,
    };
    let started = std::time::Instant::now();
    match cfg.service.describe(&req) {
        Ok(r) => {
            emit_audit(
                &cfg,
                &rid,
                req.tool_slug.as_str(),
                "POST /v1/describe",
                &principal.subject,
                AuditOutcome::Success,
                started,
            );
            (StatusCode::OK, Json(serde_json::to_value(&r).unwrap())).into_response()
        }
        Err(err) => {
            let kind_str = serde_json::to_value(err.kind)
                .ok()
                .and_then(|v| v.as_str().map(ToOwned::to_owned))
                .unwrap_or_else(|| "internal".into());
            emit_audit(
                &cfg,
                &rid,
                req.tool_slug.as_str(),
                "POST /v1/describe",
                &principal.subject,
                AuditOutcome::Failure(kind_str),
                started,
            );
            service_error_to_response(err.with_request_id(rid))
        }
    }
}

async fn handle_describe_path(
    State(cfg): State<SkillRestConfig>,
    Path(slug): Path<String>,
    headers: HeaderMap,
) -> Response {
    let rid = request_id(&headers);
    let principal = match principal_or_error(&cfg, peer(&headers), &headers, &rid) {
        Ok(p) => p,
        Err(r) => return *r,
    };
    let req = DescribeRequest {
        tool_slug: ToolSlug(slug.clone()),
        include_schema: true,
    };
    let started = std::time::Instant::now();
    match cfg.service.describe(&req) {
        Ok(r) => {
            emit_audit(
                &cfg,
                &rid,
                &slug,
                "GET /v1/tools/{slug}",
                &principal.subject,
                AuditOutcome::Success,
                started,
            );
            (StatusCode::OK, Json(serde_json::to_value(&r).unwrap())).into_response()
        }
        Err(err) => {
            emit_audit(
                &cfg,
                &rid,
                &slug,
                "GET /v1/tools/{slug}",
                &principal.subject,
                AuditOutcome::Failure(
                    serde_json::to_value(err.kind)
                        .ok()
                        .and_then(|v| v.as_str().map(ToOwned::to_owned))
                        .unwrap_or_else(|| "internal".into()),
                ),
                started,
            );
            service_error_to_response(err.with_request_id(rid))
        }
    }
}

async fn handle_call(
    State(cfg): State<SkillRestConfig>,
    headers: HeaderMap,
    Json(req): Json<CallRequest>,
) -> Response {
    let rid = request_id(&headers);
    let principal = match principal_or_error(&cfg, peer(&headers), &headers, &rid) {
        Ok(p) => p,
        Err(r) => return *r,
    };
    // Ready check — don't dispatch if the DCC host isn't up yet.
    let report = cfg.readiness.report();
    if !report.is_ready() {
        let err = ServiceError::new(ServiceErrorKind::NotReady, "DCC backend is not ready yet")
            .with_request_id(&rid)
            .with_hint(report.status_hint());
        emit_audit(
            &cfg,
            &rid,
            req.tool_slug.as_str(),
            "POST /v1/call",
            &principal.subject,
            AuditOutcome::Failure("not-ready".into()),
            std::time::Instant::now(),
        );
        return service_error_to_response(err);
    }
    let started = std::time::Instant::now();
    match cfg.service.call(&req) {
        Ok(out) => {
            emit_audit(
                &cfg,
                &rid,
                req.tool_slug.as_str(),
                "POST /v1/call",
                &principal.subject,
                AuditOutcome::Success,
                started,
            );
            let body = json!({
                "slug": out.slug.0,
                "output": out.output,
                "validation_skipped": out.validation_skipped,
                "request_id": rid,
            });
            (StatusCode::OK, Json(body)).into_response()
        }
        Err(err) => {
            emit_audit(
                &cfg,
                &rid,
                req.tool_slug.as_str(),
                "POST /v1/call",
                &principal.subject,
                AuditOutcome::Failure(
                    serde_json::to_value(err.kind)
                        .ok()
                        .and_then(|v| v.as_str().map(ToOwned::to_owned))
                        .unwrap_or_else(|| "internal".into()),
                ),
                started,
            );
            service_error_to_response(err.with_request_id(rid))
        }
    }
}

async fn handle_dcc_backend_call(
    State(cfg): State<SkillRestConfig>,
    Path(dcc_type): Path<String>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let rid = request_id(&headers);
    let principal = match principal_or_error(&cfg, peer(&headers), &headers, &rid) {
        Ok(p) => p,
        Err(r) => return *r,
    };
    let backend_tool = body
        .get("backend_tool")
        .or_else(|| body.get("tool"))
        .or_else(|| body.get("action"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let Some(backend_tool) = backend_tool else {
        let err = ServiceError::new(
            ServiceErrorKind::BadRequest,
            "missing required field: backend_tool (aliases: tool, action)",
        )
        .with_request_id(&rid);
        return service_error_to_response(err);
    };
    let params = body
        .get("arguments")
        .or_else(|| body.get("params"))
        .cloned()
        .unwrap_or_else(|| json!({}));

    let report = cfg.readiness.report();
    if !report.is_ready() {
        let err = ServiceError::new(ServiceErrorKind::NotReady, "DCC backend is not ready yet")
            .with_request_id(&rid)
            .with_hint(report.status_hint());
        emit_audit(
            &cfg,
            &rid,
            backend_tool,
            "POST /v1/dcc/{dcc_type}/call",
            &principal.subject,
            AuditOutcome::Failure("not-ready".into()),
            std::time::Instant::now(),
        );
        return service_error_to_response(err);
    }

    let started = std::time::Instant::now();
    match cfg
        .service
        .call_backend_tool_for_dcc(dcc_type.as_str(), backend_tool, params)
    {
        Ok(out) => {
            emit_audit(
                &cfg,
                &rid,
                backend_tool,
                "POST /v1/dcc/{dcc_type}/call",
                &principal.subject,
                AuditOutcome::Success,
                started,
            );
            let body = json!({
                "slug": out.slug.0,
                "output": out.output,
                "validation_skipped": out.validation_skipped,
                "request_id": rid,
            });
            (StatusCode::OK, Json(body)).into_response()
        }
        Err(err) => {
            emit_audit(
                &cfg,
                &rid,
                backend_tool,
                "POST /v1/dcc/{dcc_type}/call",
                &principal.subject,
                AuditOutcome::Failure(
                    serde_json::to_value(err.kind)
                        .ok()
                        .and_then(|v| v.as_str().map(ToOwned::to_owned))
                        .unwrap_or_else(|| "internal".into()),
                ),
                started,
            );
            service_error_to_response(err.with_request_id(rid))
        }
    }
}

async fn handle_context(State(cfg): State<SkillRestConfig>, headers: HeaderMap) -> Response {
    let rid = request_id(&headers);
    let principal = match principal_or_error(&cfg, peer(&headers), &headers, &rid) {
        Ok(p) => p,
        Err(r) => return *r,
    };
    let snap = cfg.service.context_snapshot();
    let started = std::time::Instant::now();
    emit_audit(
        &cfg,
        &rid,
        "",
        "GET /v1/context",
        &principal.subject,
        AuditOutcome::Success,
        started,
    );
    let mut v: Value = serde_json::to_value(snap).unwrap_or_else(|_| json!({}));
    if let Value::Object(ref mut m) = v {
        m.insert("request_id".into(), Value::String(rid));
    }
    (StatusCode::OK, Json(v)).into_response()
}

// ── #818 phase 1 — resources & prompts handlers ──────────────────────

async fn handle_list_resources(State(cfg): State<SkillRestConfig>, headers: HeaderMap) -> Response {
    let rid = request_id(&headers);
    let principal = match principal_or_error(&cfg, peer(&headers), &headers, &rid) {
        Ok(p) => p,
        Err(r) => return *r,
    };
    let started = std::time::Instant::now();
    let entries = cfg.service.resources().list();
    emit_audit(
        &cfg,
        &rid,
        "",
        "GET /v1/resources",
        &principal.subject,
        AuditOutcome::Success,
        started,
    );
    (
        StatusCode::OK,
        Json(json!({"total": entries.len(), "resources": entries, "request_id": rid})),
    )
        .into_response()
}

async fn handle_read_resource(
    State(cfg): State<SkillRestConfig>,
    Path(uri): Path<String>,
    headers: HeaderMap,
) -> Response {
    let rid = request_id(&headers);
    let principal = match principal_or_error(&cfg, peer(&headers), &headers, &rid) {
        Ok(p) => p,
        Err(r) => return *r,
    };
    let started = std::time::Instant::now();
    match cfg.service.resources().read(&uri) {
        Ok(payload) => {
            emit_audit(
                &cfg,
                &rid,
                &uri,
                "GET /v1/resources/{uri}",
                &principal.subject,
                AuditOutcome::Success,
                started,
            );
            (
                StatusCode::OK,
                Json(serde_json::to_value(&payload).unwrap()),
            )
                .into_response()
        }
        Err(err) => {
            let kind_str = serde_json::to_value(err.kind)
                .ok()
                .and_then(|v| v.as_str().map(ToOwned::to_owned))
                .unwrap_or_else(|| "internal".into());
            emit_audit(
                &cfg,
                &rid,
                &uri,
                "GET /v1/resources/{uri}",
                &principal.subject,
                AuditOutcome::Failure(kind_str),
                started,
            );
            service_error_to_response(err.with_request_id(rid))
        }
    }
}

async fn handle_list_prompts(State(cfg): State<SkillRestConfig>, headers: HeaderMap) -> Response {
    let rid = request_id(&headers);
    let principal = match principal_or_error(&cfg, peer(&headers), &headers, &rid) {
        Ok(p) => p,
        Err(r) => return *r,
    };
    let started = std::time::Instant::now();
    let entries = cfg.service.prompts().list();
    emit_audit(
        &cfg,
        &rid,
        "",
        "GET /v1/prompts",
        &principal.subject,
        AuditOutcome::Success,
        started,
    );
    let diagnostics = cfg.service.prompts().diagnostics();
    let mut body = json!({"total": entries.len(), "prompts": entries, "request_id": rid});
    if let Some(diagnostics) = diagnostics {
        body["diagnostics"] = diagnostics;
    }
    (StatusCode::OK, Json(body)).into_response()
}

async fn handle_get_prompt(
    State(cfg): State<SkillRestConfig>,
    Path(name): Path<String>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    let rid = request_id(&headers);
    let principal = match principal_or_error(&cfg, peer(&headers), &headers, &rid) {
        Ok(p) => p,
        Err(r) => return *r,
    };
    let arguments = match query.get("args") {
        Some(raw) => match serde_json::from_str::<Value>(raw) {
            Ok(value @ Value::Object(_)) => value,
            Ok(_) => {
                return service_error_to_response(
                    ServiceError::new(
                        ServiceErrorKind::InvalidParams,
                        "args must be a JSON object",
                    )
                    .with_request_id(rid),
                );
            }
            Err(e) => {
                return service_error_to_response(
                    ServiceError::new(
                        ServiceErrorKind::InvalidParams,
                        format!("invalid args JSON: {e}"),
                    )
                    .with_request_id(rid),
                );
            }
        },
        None => json!({}),
    };
    let started = std::time::Instant::now();
    match cfg.service.prompts().get(&name, &arguments) {
        Ok(payload) => {
            emit_audit(
                &cfg,
                &rid,
                &name,
                "GET /v1/prompts/{name}",
                &principal.subject,
                AuditOutcome::Success,
                started,
            );
            (
                StatusCode::OK,
                Json(serde_json::to_value(&payload).unwrap()),
            )
                .into_response()
        }
        Err(err) => {
            let kind_str = serde_json::to_value(err.kind)
                .ok()
                .and_then(|v| v.as_str().map(ToOwned::to_owned))
                .unwrap_or_else(|| "internal".into());
            emit_audit(
                &cfg,
                &rid,
                &name,
                "GET /v1/prompts/{name}",
                &principal.subject,
                AuditOutcome::Failure(kind_str),
                started,
            );
            service_error_to_response(err.with_request_id(rid))
        }
    }
}

// ── #818 phase 1b — SSE handlers ─────────────────────────────────────

/// `GET /v1/resources/{uri}/events` — SSE stream for resource mutations.
async fn handle_resource_events(
    State(cfg): State<SkillRestConfig>,
    Path(uri): Path<String>,
    headers: HeaderMap,
) -> Response {
    use super::service::ResourceEventStream;

    let rid = request_id(&headers);
    match principal_or_error(&cfg, peer(&headers), &headers, &rid) {
        Ok(_) => {}
        Err(r) => return *r,
    };

    let stream: ResourceEventStream = match cfg.service.resources().subscribe(&uri) {
        Ok(s) => s,
        Err(err) => return service_error_to_response(err.with_request_id(rid)),
    };

    let sse_stream = stream.map(|item| {
        item.map(|ev| {
            let data = serde_json::to_string(&ev).unwrap_or_default();
            Event::default().event("resource").data(data)
        })
    });

    Sse::new(sse_stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

/// `GET /v1/jobs/{id}/events` — SSE stream for a running async job.
async fn handle_job_events(
    State(cfg): State<SkillRestConfig>,
    Path(job_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let rid = request_id(&headers);
    match principal_or_error(&cfg, peer(&headers), &headers, &rid) {
        Ok(_) => {}
        Err(r) => return *r,
    };

    let stream = match cfg.service.jobs().subscribe(&job_id) {
        Ok(s) => s,
        Err(err) => return service_error_to_response(err.with_request_id(rid)),
    };

    let sse_stream = stream.map(|item| {
        item.map(|ev| {
            let data = serde_json::to_string(&ev).unwrap_or_default();
            Event::default().event("job").data(data)
        })
    });

    Sse::new(sse_stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

/// `DELETE /v1/jobs/{id}` — cancel a running async job.
async fn handle_job_cancel(
    State(cfg): State<SkillRestConfig>,
    Path(job_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let rid = request_id(&headers);
    match principal_or_error(&cfg, peer(&headers), &headers, &rid) {
        Ok(_) => {}
        Err(r) => return *r,
    };

    match cfg.service.jobs().cancel(&job_id) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => service_error_to_response(err.with_request_id(rid)),
    }
}

// ── utoipa path-doc stubs ────────────────────────────────────────────
//
// These `op_*` functions exist purely to carry `#[utoipa::path(...)]`
// attributes. `utoipa` only needs a real function with the attribute
// at proc-macro expansion time — it never actually calls them. By
// keeping the doc metadata beside the runtime handlers (rather than
// on the handlers themselves) we avoid the fragile interaction
// between axum's extractor macros and `#[utoipa::path]`'s parameter
// parsing, and we keep the runtime code free of documentation
// boilerplate.
//
// The functions are also conveniently referenced from
// [`super::openapi::SkillRestApiDoc`]'s `paths(...)` list.

#[utoipa::path(
    get,
    path = "/v1/healthz",
    tag = "health",
    responses(
        (status = 200, description = "process is alive", body = serde_json::Value),
    )
)]
#[allow(dead_code)]
pub fn op_healthz() {}

#[utoipa::path(
    get,
    path = "/v1/readyz",
    tag = "health",
    responses(
        (status = 200, description = "base readiness bits are green",
         body = super::readiness::ReadinessReport),
        (status = 503, description = "one or more base readiness bits are red",
         body = super::readiness::ReadinessReport),
    )
)]
#[allow(dead_code)]
pub fn op_readyz() {}

#[utoipa::path(
    get,
    path = "/v1/openapi.json",
    tag = "meta",
    responses(
        (status = 200, description = "machine-readable API contract",
         body = serde_json::Value),
    )
)]
#[allow(dead_code)]
pub fn op_openapi() {}

#[utoipa::path(
    get,
    path = "/v1/skills",
    tag = "skills",
    responses(
        (status = 200, description = "list of loaded skills",
         body = [SkillListEntry]),
        (status = 401, description = "auth gate rejected the request",
         body = ServiceError),
    )
)]
#[allow(dead_code)]
pub fn op_list_skills() {}

#[utoipa::path(
    post,
    path = "/v1/search",
    tag = "skills",
    request_body = super::service::SearchRequest,
    responses(
        (status = 200, description = "compact search hits", body = SearchResponse),
        (status = 401, description = "unauthorized", body = ServiceError),
    )
)]
#[allow(dead_code)]
pub fn op_search() {}

#[utoipa::path(
    post,
    path = "/v1/load_skill",
    tag = "skills",
    request_body = super::service::LoadSkillRequest,
    responses(
        (status = 200, description = "skill loaded", body = SkillLifecycleResponse),
        (status = 400, description = "bad request", body = ServiceError),
        (status = 401, description = "unauthorized", body = ServiceError),
        (status = 404, description = "unknown skill", body = ServiceError),
    )
)]
#[allow(dead_code)]
pub fn op_load_skill() {}

#[utoipa::path(
    post,
    path = "/v1/unload_skill",
    tag = "skills",
    request_body = super::service::UnloadSkillRequest,
    responses(
        (status = 200, description = "skill unloaded", body = SkillLifecycleResponse),
        (status = 400, description = "bad request", body = ServiceError),
        (status = 401, description = "unauthorized", body = ServiceError),
        (status = 404, description = "unknown skill", body = ServiceError),
    )
)]
#[allow(dead_code)]
pub fn op_unload_skill() {}

#[utoipa::path(
    post,
    path = "/v1/describe",
    tag = "skills",
    request_body = super::service::DescribeRequest,
    responses(
        (status = 200, description = "one capability", body = DescribeResponse),
        (status = 404, description = "unknown slug", body = ServiceError),
        (status = 409, description = "slug ambiguous", body = ServiceError),
    )
)]
#[allow(dead_code)]
pub fn op_describe() {}

#[utoipa::path(
    get,
    path = "/v1/tools/{slug}",
    tag = "skills",
    params(
        ("slug" = String, Path, description = "tool slug <dcc>.<skill>.<action>"),
    ),
    responses(
        (status = 200, description = "one capability", body = DescribeResponse),
        (status = 404, description = "unknown slug", body = ServiceError),
    )
)]
#[allow(dead_code)]
pub fn op_describe_path() {}

#[utoipa::path(
    post,
    path = "/v1/call",
    tag = "skills",
    request_body = CallRequest,
    responses(
        (status = 200, description = "successful invocation", body = CallOutcome),
        (status = 400, description = "bad request or invalid params", body = ServiceError),
        (status = 401, description = "unauthorized", body = ServiceError),
        (status = 404, description = "unknown slug", body = ServiceError),
        (status = 409, description = "skill not loaded / ambiguous / affinity", body = ServiceError),
        (status = 502, description = "backend handler error", body = ServiceError),
        (status = 503, description = "DCC not ready", body = ServiceError),
    )
)]
#[allow(dead_code)]
pub fn op_call() {}

#[utoipa::path(
    post,
    path = "/v1/dcc/{dcc_type}/call",
    tag = "skills",
    params(
        ("dcc_type" = String, Path, description = "DCC bucket (must match the action's catalog dcc)"),
    ),
    request_body = serde_json::Value,
    responses(
        (status = 200, description = "successful invocation", body = CallOutcome),
        (status = 400, description = "bad request", body = ServiceError),
        (status = 401, description = "unauthorized", body = ServiceError),
        (status = 404, description = "unknown action", body = ServiceError),
        (status = 409, description = "skill not loaded / ambiguous", body = ServiceError),
        (status = 503, description = "DCC not ready", body = ServiceError),
    )
)]
#[allow(dead_code)]
pub fn op_dcc_backend_call() {}

#[utoipa::path(
    get,
    path = "/v1/context",
    tag = "skills",
    responses(
        (status = 200, description = "current DCC scene/document snapshot",
         body = ContextSnapshot),
        (status = 401, description = "unauthorized", body = ServiceError),
    )
)]
#[allow(dead_code)]
pub fn op_context() {}

#[utoipa::path(
    get,
    path = "/v1/resources",
    tag = "resources",
    responses(
        (status = 200, description = "list of MCP resources exposed by this DCC instance",
         body = serde_json::Value),
        (status = 401, description = "unauthorized", body = ServiceError),
    )
)]
#[allow(dead_code)]
pub fn op_list_resources() {}

#[utoipa::path(
    get,
    path = "/v1/resources/{uri}",
    tag = "resources",
    params(("uri" = String, Path, description = "URL-encoded MCP resource URI")),
    responses(
        (status = 200, description = "resource contents", body = ResourceReadResponse),
        (status = 401, description = "unauthorized", body = ServiceError),
        (status = 404, description = "URI not registered", body = ServiceError),
        (status = 500, description = "read failure", body = ServiceError),
    )
)]
#[allow(dead_code)]
pub fn op_read_resource() {}

#[utoipa::path(
    get,
    path = "/v1/prompts",
    tag = "prompts",
    responses(
        (status = 200, description = "list of MCP prompts exposed by this DCC instance",
         body = serde_json::Value),
        (status = 401, description = "unauthorized", body = ServiceError),
    )
)]
#[allow(dead_code)]
pub fn op_list_prompts() {}

#[utoipa::path(
    get,
    path = "/v1/prompts/{name}",
    tag = "prompts",
    params(("name" = String, Path, description = "Prompt name")),
    responses(
        (status = 200, description = "rendered prompt messages", body = PromptGetResponse),
        (status = 401, description = "unauthorized", body = ServiceError),
        (status = 404, description = "prompt not found", body = ServiceError),
    )
)]
#[allow(dead_code)]
pub fn op_get_prompt() {}

#[utoipa::path(
    get,
    path = "/v1/resources/{uri}/events",
    tag = "resources",
    params(("uri" = String, Path, description = "URL-encoded MCP resource URI")),
    responses(
        (status = 200, description = "SSE stream of resource events (text/event-stream)"),
        (status = 401, description = "unauthorized", body = ServiceError),
        (status = 404, description = "URI not subscribed", body = ServiceError),
    )
)]
#[allow(dead_code)]
pub fn op_resource_events() {}

#[utoipa::path(
    get,
    path = "/v1/jobs/{id}/events",
    tag = "jobs",
    params(("id" = String, Path, description = "Job ID returned by an async POST /v1/call")),
    responses(
        (status = 200, description = "SSE stream of job events (text/event-stream)"),
        (status = 401, description = "unauthorized", body = ServiceError),
        (status = 404, description = "job not found", body = ServiceError),
    )
)]
#[allow(dead_code)]
pub fn op_job_events() {}

#[utoipa::path(
    delete,
    path = "/v1/jobs/{id}",
    tag = "jobs",
    params(("id" = String, Path, description = "Job ID to cancel")),
    responses(
        (status = 204, description = "job cancel signal sent"),
        (status = 401, description = "unauthorized", body = ServiceError),
        (status = 404, description = "job not found", body = ServiceError),
    )
)]
#[allow(dead_code)]
pub fn op_job_cancel() {}
