//! Axum router that exposes the [`super::service::SkillRestService`].
//!
//! Each handler is a thin adapter: parse → auth → delegate to the
//! service → wrap in a response. Keeping the adapters tiny means the
//! SOLID service layer is the only thing tests exercise through axum.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use chrono::Utc;
use serde_json::{Value, json};

use super::audit::{AuditEvent, AuditOutcome, AuditSink, NoopAuditSink};
use super::auth::{AllowLocalhostGate, AuthContext, AuthGate, Principal};
use super::errors::{ServiceError, ServiceErrorKind};
use super::openapi::build_openapi_document;
use super::readiness::{ReadinessProbe, StaticReadiness};
use super::service::{
    CallOutcome, CallRequest, ContextSnapshot, DescribeRequest, DescribeResponse, SearchRequest,
    SearchResponse, SkillListEntry, SkillRestService, ToolSlug,
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
        .route("/v1/describe", post(handle_describe))
        .route("/v1/tools/{slug}", get(handle_describe_path))
        .route("/v1/call", post(handle_call))
        .route("/v1/context", get(handle_context))
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
    let doc = build_openapi_document(&cfg.server_title, &cfg.server_version);
    let html = utoipa_scalar::Scalar::new(doc).to_html();
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
            .with_hint(format!(
                "readiness: process={}, dispatcher={}, dcc={}",
                report.process, report.dispatcher, report.dcc
            ));
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
        (status = 200, description = "all three readiness bits are green",
         body = super::readiness::ReadinessReport),
        (status = 503, description = "one or more readiness bits are red",
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
