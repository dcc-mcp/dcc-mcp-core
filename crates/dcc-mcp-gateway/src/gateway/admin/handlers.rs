//! Admin UI HTTP handlers.

use std::collections::HashMap;
use std::path::{Path as FsPath, PathBuf};
use std::time::Duration;
use std::time::UNIX_EPOCH;

use axum::Json;
use axum::extract::{OriginalUri, Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use dcc_mcp_gateway_core::naming::instance_short;
use dcc_mcp_updater::{UpdateInfo, Updater};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::debug_response::{DebugListQuery, debug_response};
use super::html::ADMIN_HTML;
use super::issue_report::{IssueReportMode, issue_report_filename, issue_report_json};
use super::links::AdminLinkBuilder;
use super::skill_reload::reload_skill_paths_and_refresh_backends;
use super::state::{AdminAuditRecord, AdminState};
use super::trace::{AgentContext, DispatchTrace};
use crate::gateway::capability::RefreshReason;
use crate::gateway::capability_service::refresh_all_live_backends;
use crate::gateway::event_log::{ContendEvent, EventKind};
use crate::gateway::resilience::{self as gw_resilience, gateway_limits};
use crate::gateway::response_codec::{
    JSON_MIME, TOKEN_ESTIMATOR, TOON_MIME, default_rest_response_format,
};
use dcc_mcp_db::env::ENV_DCC_MCP_LOG_DIR;
use dcc_mcp_db::read_gateway_log_dir_rows_recent;
use dcc_mcp_transport::discovery::types::{GATEWAY_SENTINEL_DCC_TYPE, ServiceEntry};

const ADMIN_FILE_LOG_READ_TIMEOUT: Duration = Duration::from_millis(750);
const ENV_SENTRY_DSN: &str = "DCC_MCP_SENTRY_DSN";
const ENV_SENTRY_ENVIRONMENT: &str = "DCC_MCP_SENTRY_ENVIRONMENT";
const ENV_SENTRY_RELEASE: &str = "DCC_MCP_SENTRY_RELEASE";
const ENV_SENTRY_SAMPLE_RATE: &str = "DCC_MCP_SENTRY_SAMPLE_RATE";
const ENV_WEBHOOKS_CONFIG: &str = "DCC_MCP_WEBHOOKS_CONFIG";
const ENV_DCC_MCP_ETC_DIR: &str = "DCC_MCP_ETC_DIR";
const ENV_WECOM_WEBHOOK_URL: &str = "DCC_MCP_WECOM_WEBHOOK_URL";
const ENV_WECOM_EVENTS: &str = "DCC_MCP_WECOM_EVENTS";
const ENV_WECOM_TEMPLATE: &str = "DCC_MCP_WECOM_TEMPLATE";
const ENV_OTLP_ENDPOINT: &str = "OTEL_EXPORTER_OTLP_ENDPOINT";
const ENV_OTLP_SERVICE_NAME: &str = "OTEL_SERVICE_NAME";
const ENV_OTLP_HEADERS: &str = "OTEL_EXPORTER_OTLP_HEADERS";
const DEFAULT_WECOM_TEMPLATE: &str = "DCC-MCP $event\nDCC: $dcc-type\nTool: $tool-slug\nURL: $url";
const DEFAULT_SENTRY_CONFIG_FILE: &str = "sentry.json";
const DEFAULT_WEBHOOKS_CONFIG_FILE: &str = "webhooks.yaml";
const DEFAULT_OTLP_CONFIG_FILE: &str = "otlp.json";

#[cfg(test)]
pub(crate) static INTEGRATIONS_TEST_ENV_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

fn traffic_export_filename() -> &'static str {
    "dcc-mcp-traffic-capture.jsonl"
}

/// `GET /admin` — serve the inline HTML dashboard.
pub async fn handle_admin_ui() -> impl IntoResponse {
    let mut resp = axum::response::Html(ADMIN_HTML).into_response();
    resp.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache, no-store"),
    );
    resp
}

/// `GET /admin/api/activity` — unified operator / agent activity timeline.
pub async fn handle_admin_activity(
    State(s): State<AdminState>,
    axum::extract::Query(params): axum::extract::Query<DebugListQuery>,
) -> impl IntoResponse {
    let limit = params.limit(200, 1_000);
    Json(crate::gateway::admin::activity::build_activity_payload(&s, limit).await)
}

/// `GET /admin/api/governance` — effective traffic governance policy and decisions.
pub async fn handle_admin_governance(
    State(s): State<AdminState>,
    axum::extract::Query(params): axum::extract::Query<DebugListQuery>,
) -> impl IntoResponse {
    let limit = params.limit(200, 1_000);
    Json(crate::gateway::admin::governance::build_governance_payload(&s, limit).await)
}

/// `GET /admin/api/traffic?limit=200` — retained live traffic frames.
pub async fn handle_admin_traffic(
    State(s): State<AdminState>,
    headers: HeaderMap,
    OriginalUri(uri): OriginalUri,
    Query(params): Query<DebugListQuery>,
) -> impl IntoResponse {
    let links = AdminLinkBuilder::from_request(&headers, &uri);
    let limit = params.limit(200, 1_000);
    Json(crate::gateway::admin::traffic::build_traffic_payload(
        &s.gateway.traffic_capture,
        limit,
        json!({
            "admin_traffic_url": links.panel_url("traffic"),
            "traffic_api_url": links.api_url("/traffic"),
            "traffic_export_jsonl_url": links.api_url("/traffic/export"),
        }),
    ))
}

/// `GET /admin/api/traffic/export?limit=1000` — retained live frames as JSONL.
pub async fn handle_admin_traffic_export(
    State(s): State<AdminState>,
    Query(params): Query<DebugListQuery>,
) -> impl IntoResponse {
    let limit = params.limit(1_000, 10_000);
    let body = crate::gateway::admin::traffic::build_traffic_export_body(
        &s.gateway.traffic_capture,
        limit,
    );
    let mut response = (StatusCode::OK, body).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/x-ndjson; charset=utf-8"),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!(
            "attachment; filename=\"{}\"",
            traffic_export_filename()
        ))
        .unwrap_or_else(|_| HeaderValue::from_static("attachment")),
    );
    response
}

#[derive(Debug, Deserialize)]
pub struct UpdateIntegrationRequest {
    kind: String,
    #[serde(default)]
    config: Map<String, Value>,
}

/// `GET /admin/api/integrations` — runtime integration state derived from real env/config.
pub async fn handle_admin_integrations(State(s): State<AdminState>) -> impl IntoResponse {
    Json(json!({
        "integrations": build_integration_entries(&s),
    }))
}

/// `PUT /admin/api/integrations` — persist an integration config for restart.
///
/// Integration backends are initialised at process startup, so this endpoint never pretends a
/// submitted value is hot-applied. It writes supported file-backed configs to the user config
/// directory, stores process-local pending state, and reports `pending_restart` until restart.
pub async fn handle_admin_integration_update(
    State(s): State<AdminState>,
    Json(req): Json<UpdateIntegrationRequest>,
) -> Response {
    let kind = req.kind.trim().to_ascii_lowercase();
    let sanitized = match sanitize_integration_config(&kind, req.config) {
        Ok(config) => config,
        Err((status, message)) => {
            return (
                status,
                Json(json!({
                    "error": "invalid_integration_config",
                    "message": message,
                })),
            )
                .into_response();
        }
    };
    let pending_config = match persist_integration_config(&kind, &sanitized) {
        Ok(config) => config,
        Err((status, message)) => {
            return (
                status,
                Json(json!({
                    "error": "integration_config_persist_failed",
                    "message": message,
                })),
            )
                .into_response();
        }
    };

    {
        let mut pending = s.pending_integrations.lock();
        if pending_config.is_empty() {
            pending.remove(&kind);
        } else {
            pending.insert(kind.clone(), Value::Object(pending_config));
        }
    }

    match integration_entry(&s, &kind) {
        Some(entry) => Json(entry).into_response(),
        None => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "unknown_integration",
                "message": format!("unsupported integration kind '{kind}'"),
            })),
        )
            .into_response(),
    }
}

fn build_integration_entries(s: &AdminState) -> Vec<Value> {
    ["sentry", "webhooks", "wecom", "otlp"]
        .into_iter()
        .filter_map(|kind| integration_entry(s, kind))
        .collect()
}

fn integration_entry(s: &AdminState, kind: &str) -> Option<Value> {
    match kind {
        "sentry" => Some(sentry_integration_entry(s)),
        "webhooks" => Some(webhooks_integration_entry(s)),
        "wecom" => Some(wecom_integration_entry(s)),
        "otlp" => Some(otlp_integration_entry(s)),
        _ => None,
    }
}

fn sentry_integration_entry(s: &AdminState) -> Value {
    let pending = pending_integration_config(s, "sentry");
    let has_pending = pending.as_ref().is_some_and(|config| !config.is_empty());
    let dsn = env_string(ENV_SENTRY_DSN);
    let local_config = read_sentry_config_from_default_file();
    let mut config = Map::new();
    let mut error = None;
    let active = match dsn.as_deref() {
        Some(value) if sentry_dsn_looks_valid(value) => {
            config.insert("dsn".into(), Value::String(mask_secret_url(value)));
            true
        }
        Some(_) => {
            error = Some(format!(
                "{ENV_SENTRY_DSN} is set but is not a valid Sentry DSN URL"
            ));
            config.insert("dsn".into(), Value::String("********".into()));
            false
        }
        None => match local_config {
            Some(Ok(saved)) => {
                for (key, value) in saved {
                    config.insert(key, value);
                }
                true
            }
            Some(Err(message)) => {
                error = Some(message);
                false
            }
            None => false,
        },
    };

    if active {
        if let Some(environment) = env_string(ENV_SENTRY_ENVIRONMENT) {
            config.insert("environment".into(), Value::String(environment));
        } else {
            config
                .entry("environment")
                .or_insert_with(|| Value::String("production".into()));
        }
        if let Some(release) = env_string(ENV_SENTRY_RELEASE) {
            config.insert("release".into(), Value::String(release));
        } else {
            config
                .entry("release")
                .or_insert_with(|| Value::String(s.gateway.server_version.clone()));
        }
        if let Some(sample_rate) = env_f64(ENV_SENTRY_SAMPLE_RATE) {
            config.insert("sample_rate".into(), Value::from(sample_rate));
        } else {
            config
                .entry("sample_rate")
                .or_insert_with(|| Value::from(1.0));
        }
    }

    if let Some(pending) = pending {
        for (key, value) in pending {
            config.insert(key, value);
        }
    }
    let status = integration_status(active, has_pending || (!config.is_empty() && !active));
    insert_write_config_path(&mut config, DEFAULT_SENTRY_CONFIG_FILE);

    integration_json(
        "sentry",
        "Sentry Error Monitoring",
        "Send panics, error events, and span breadcrumbs to Sentry.",
        status,
        config,
        vec![
            env_lock("dsn", ENV_SENTRY_DSN),
            env_lock("environment", ENV_SENTRY_ENVIRONMENT),
            env_lock("release", ENV_SENTRY_RELEASE),
            env_lock("sample_rate", ENV_SENTRY_SAMPLE_RATE),
        ],
        error,
    )
}

fn webhooks_integration_entry(s: &AdminState) -> Value {
    let pending = pending_integration_config(s, "webhooks");
    let has_pending = pending.as_ref().is_some_and(|config| !config.is_empty());
    let config_path = env_string(ENV_WEBHOOKS_CONFIG);
    let default_path = default_webhooks_config_path().ok();
    let mut config = Map::new();
    let mut error = None;
    let mut active = false;

    if let Some(path) = config_path {
        config.insert("config_path".into(), Value::String(path.clone()));
        match inspect_webhooks_config(FsPath::new(&path)) {
            Ok(webhook_count) => {
                config.insert("webhook_count".into(), Value::from(webhook_count));
                if let Ok(raw) = std::fs::read_to_string(&path) {
                    config.insert("config_text".into(), Value::String(raw));
                }
                active = true;
            }
            Err(message) => {
                error = Some(message);
            }
        }
    } else if let Some(path) = default_path.as_ref() {
        config.insert(
            "config_path".into(),
            Value::String(path.to_string_lossy().to_string()),
        );
        if path.exists() {
            match inspect_webhooks_config(path) {
                Ok(webhook_count) => {
                    config.insert("webhook_count".into(), Value::from(webhook_count));
                    if let Ok(raw) = std::fs::read_to_string(path) {
                        config.insert("config_text".into(), Value::String(raw));
                    }
                    active = true;
                }
                Err(message) => {
                    error = Some(message);
                }
            }
        } else {
            config.insert(
                "config_text".into(),
                Value::String(default_webhooks_config_template()),
            );
        }
    }

    if let Some(pending_config) = pending.as_ref() {
        if let Some(path) = pending_config.get("config_path").and_then(Value::as_str)
            && let Err(message) = inspect_webhooks_config(FsPath::new(path))
        {
            error = Some(format!("pending config warning: {message}"));
        }
        for (key, value) in pending_config {
            config.insert(key.clone(), value.clone());
        }
    }
    let status = integration_status(active, has_pending);
    insert_write_config_path(&mut config, DEFAULT_WEBHOOKS_CONFIG_FILE);

    integration_json(
        "webhooks",
        "Event Webhooks",
        "Deliver EventBus envelopes to configured HTTP webhook endpoints.",
        status,
        config,
        vec![env_lock("config_path", ENV_WEBHOOKS_CONFIG)],
        error,
    )
}

fn wecom_integration_entry(s: &AdminState) -> Value {
    let pending = pending_integration_config(s, "wecom");
    let has_pending = pending.as_ref().is_some_and(|config| !config.is_empty());
    let webhook_url = env_string(ENV_WECOM_WEBHOOK_URL);
    let mut config = Map::new();
    let mut error = None;
    let active = match webhook_url.as_deref() {
        Some(value) if http_url_looks_valid(value) => {
            config.insert("webhook_url".into(), Value::String(mask_webhook_url(value)));
            config.insert(
                "event_types".into(),
                Value::Array(
                    env_event_patterns(ENV_WECOM_EVENTS)
                        .into_iter()
                        .map(Value::String)
                        .collect(),
                ),
            );
            config.insert(
                "template".into(),
                Value::String(
                    env_string(ENV_WECOM_TEMPLATE)
                        .unwrap_or_else(|| DEFAULT_WECOM_TEMPLATE.to_string()),
                ),
            );
            true
        }
        Some(_) => {
            error = Some(format!(
                "{ENV_WECOM_WEBHOOK_URL} is set but is not a valid HTTP(S) webhook URL"
            ));
            config.insert("webhook_url".into(), Value::String("********".into()));
            false
        }
        None => false,
    };

    let active = if active {
        true
    } else if let Some(saved) = saved_wecom_config_from_default_webhooks() {
        match saved {
            Ok(saved) if !saved.is_empty() => {
                for (key, value) in saved {
                    config.insert(key, value);
                }
                true
            }
            Ok(_) => false,
            Err(message) => {
                error = Some(message);
                false
            }
        }
    } else {
        false
    };

    if let Some(pending) = pending {
        for (key, value) in pending {
            config.insert(key, value);
        }
    }
    let status = integration_status(active, has_pending || (!config.is_empty() && !active));
    insert_write_config_path(&mut config, DEFAULT_WEBHOOKS_CONFIG_FILE);

    integration_json(
        "wecom",
        "WeCom Message Push",
        "Push selected EventBus events to an Enterprise WeChat group robot with a templated message.",
        status,
        config,
        vec![
            env_lock("webhook_url", ENV_WECOM_WEBHOOK_URL),
            env_lock("event_types", ENV_WECOM_EVENTS),
            env_lock("template", ENV_WECOM_TEMPLATE),
        ],
        error,
    )
}

fn otlp_integration_entry(s: &AdminState) -> Value {
    let pending = pending_integration_config(s, "otlp");
    let has_pending = pending.as_ref().is_some_and(|config| !config.is_empty());
    let endpoint = env_string(ENV_OTLP_ENDPOINT);
    let local_config = read_otlp_config_from_default_file();
    let mut config = Map::new();
    let mut error = None;
    let active = if let Some(endpoint) = endpoint {
        if let Some(Ok(saved)) = local_config {
            for (key, value) in saved {
                config.insert(key, value);
            }
        }
        config.insert("endpoint".into(), Value::String(endpoint));
        true
    } else {
        match local_config {
            Some(Ok(saved)) => {
                for (key, value) in saved {
                    config.insert(key, value);
                }
                config
                    .get("endpoint")
                    .and_then(Value::as_str)
                    .is_some_and(|value| !value.trim().is_empty())
            }
            Some(Err(message)) => {
                error = Some(message);
                false
            }
            None => false,
        }
    };

    if active {
        if let Some(service_name) = env_string(ENV_OTLP_SERVICE_NAME) {
            config.insert("service_name".into(), Value::String(service_name));
        }
        if let Some(headers) = env_string(ENV_OTLP_HEADERS) {
            config.insert("headers".into(), Value::String(headers));
        }
    } else if let Some(pending) = pending.as_ref() {
        config = pending.clone();
    }

    if active && let Some(pending) = pending {
        for (key, value) in pending {
            config.insert(key, value);
        }
    }
    let status = integration_status(active, has_pending || (!config.is_empty() && !active));
    insert_write_config_path(&mut config, DEFAULT_OTLP_CONFIG_FILE);

    integration_json(
        "otlp",
        "OTLP Telemetry",
        "Export distributed traces and metrics to an OTLP-compatible collector.",
        status,
        config,
        vec![
            env_lock("endpoint", ENV_OTLP_ENDPOINT),
            env_lock("service_name", ENV_OTLP_SERVICE_NAME),
            env_lock("headers", ENV_OTLP_HEADERS),
        ],
        error,
    )
}

fn integration_json(
    kind: &str,
    label: &str,
    description: &str,
    status: &'static str,
    config: Map<String, Value>,
    env_locked_fields: Vec<Value>,
    error: Option<String>,
) -> Value {
    let mut entry = json!({
        "kind": kind,
        "label": label,
        "description": description,
        "status": status,
        "config": Value::Object(config),
        "env_locked_fields": env_locked_fields,
    });
    if let Some(error) = error {
        entry["error"] = Value::String(error);
    }
    entry
}

fn integration_status(active: bool, pending: bool) -> &'static str {
    if pending {
        "pending_restart"
    } else if active {
        "active"
    } else {
        "inactive"
    }
}

fn pending_integration_config(s: &AdminState, kind: &str) -> Option<Map<String, Value>> {
    s.pending_integrations
        .lock()
        .get(kind)
        .and_then(Value::as_object)
        .cloned()
        .filter(|config| !config.is_empty())
}

fn sanitize_integration_config(
    kind: &str,
    config: Map<String, Value>,
) -> Result<Map<String, Value>, (StatusCode, String)> {
    match kind {
        "sentry" => sanitize_sentry_config(config),
        "webhooks" => sanitize_webhooks_config(config),
        "wecom" => sanitize_wecom_config(config),
        "otlp" => sanitize_string_config(config, &["endpoint", "service_name", "headers"]),
        _ => Err((
            StatusCode::BAD_REQUEST,
            format!("unsupported integration kind '{kind}'"),
        )),
    }
}

fn sanitize_webhooks_config(
    config: Map<String, Value>,
) -> Result<Map<String, Value>, (StatusCode, String)> {
    let mut out = Map::new();
    if let Some(config_text) = optional_string_field(&config, "config_text")? {
        inspect_webhooks_config_text(&config_text).map_err(|message| {
            (
                StatusCode::BAD_REQUEST,
                format!("config_text must be valid webhooks YAML: {message}"),
            )
        })?;
        out.insert("config_text".into(), Value::String(config_text));
    }
    Ok(out)
}

fn sanitize_wecom_config(
    config: Map<String, Value>,
) -> Result<Map<String, Value>, (StatusCode, String)> {
    let mut out = Map::new();
    let webhook_url = optional_string_field(&config, "webhook_url")?;
    if let Some(url) = webhook_url {
        if !http_url_looks_valid(&url) {
            return Err((
                StatusCode::BAD_REQUEST,
                "webhook_url must be a valid HTTP(S) URL".into(),
            ));
        }
        out.insert("webhook_url".into(), Value::String(url));
    }

    let mut events = optional_string_list_field(&config, "event_types")?;
    if events.is_empty() && out.contains_key("webhook_url") {
        events = default_wecom_events();
    }
    if !events.is_empty() {
        out.insert(
            "event_types".into(),
            Value::Array(events.into_iter().map(Value::String).collect()),
        );
    }

    if let Some(template) = optional_string_field(&config, "template")? {
        out.insert("template".into(), Value::String(template));
    } else if out.contains_key("webhook_url") {
        out.insert(
            "template".into(),
            Value::String(DEFAULT_WECOM_TEMPLATE.to_string()),
        );
    }
    Ok(out)
}

fn persist_integration_config(
    kind: &str,
    config: &Map<String, Value>,
) -> Result<Map<String, Value>, (StatusCode, String)> {
    match kind {
        "sentry" => persist_sentry_config(config),
        "webhooks" => persist_webhooks_config(config),
        "wecom" => persist_wecom_config(config),
        "otlp" => persist_json_config(DEFAULT_OTLP_CONFIG_FILE, config),
        _ => Ok(config.clone()),
    }
}

fn persist_sentry_config(
    config: &Map<String, Value>,
) -> Result<Map<String, Value>, (StatusCode, String)> {
    let mut out = persist_json_config(DEFAULT_SENTRY_CONFIG_FILE, config)?;
    if let Some(dsn) = config.get("dsn").and_then(Value::as_str) {
        out.insert("dsn".into(), Value::String(mask_secret_url(dsn)));
    }
    Ok(out)
}

fn persist_json_config(
    file_name: &str,
    config: &Map<String, Value>,
) -> Result<Map<String, Value>, (StatusCode, String)> {
    if config.is_empty() {
        return Ok(Map::new());
    }
    let target = default_integration_config_path(file_name)
        .map_err(|message| (StatusCode::INTERNAL_SERVER_ERROR, message))?;
    write_json_config(&target, config)
        .map_err(|message| (StatusCode::INTERNAL_SERVER_ERROR, message))?;

    let mut out = config.clone();
    out.insert(
        "config_path".into(),
        Value::String(target.to_string_lossy().to_string()),
    );
    out.insert(
        "write_config_path".into(),
        Value::String(target.to_string_lossy().to_string()),
    );
    Ok(out)
}

fn persist_webhooks_config(
    config: &Map<String, Value>,
) -> Result<Map<String, Value>, (StatusCode, String)> {
    let Some(config_text) = config.get("config_text").and_then(Value::as_str) else {
        return Ok(config.clone());
    };
    let target = writable_webhooks_config_path()
        .map_err(|message| (StatusCode::INTERNAL_SERVER_ERROR, message))?;
    write_text_config(&target, config_text)
        .map_err(|message| (StatusCode::INTERNAL_SERVER_ERROR, message))?;

    let mut out = Map::new();
    out.insert(
        "config_path".into(),
        Value::String(target.to_string_lossy().to_string()),
    );
    out.insert(
        "write_config_path".into(),
        Value::String(target.to_string_lossy().to_string()),
    );
    out.insert("config_text".into(), Value::String(config_text.to_string()));
    out.insert(
        "webhook_count".into(),
        Value::from(inspect_webhooks_config_text(config_text).unwrap_or(0)),
    );
    Ok(out)
}

fn persist_wecom_config(
    config: &Map<String, Value>,
) -> Result<Map<String, Value>, (StatusCode, String)> {
    let Some(webhook_url) = config.get("webhook_url").and_then(Value::as_str) else {
        return Ok(Map::new());
    };
    let events = config
        .get("event_types")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .filter(|items| !items.is_empty())
        .unwrap_or_else(default_wecom_events);
    let template = config
        .get("template")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(DEFAULT_WECOM_TEMPLATE);
    let target = default_webhooks_config_path()
        .map_err(|message| (StatusCode::INTERNAL_SERVER_ERROR, message))?;
    let raw = render_webhooks_yaml_with_wecom(&target, webhook_url, &events, template)
        .map_err(|message| (StatusCode::INTERNAL_SERVER_ERROR, message))?;
    write_text_config(&target, &raw)
        .map_err(|message| (StatusCode::INTERNAL_SERVER_ERROR, message))?;

    let mut out = Map::new();
    out.insert(
        "webhook_url".into(),
        Value::String(mask_webhook_url(webhook_url)),
    );
    out.insert(
        "event_types".into(),
        Value::Array(events.into_iter().map(Value::String).collect()),
    );
    out.insert("template".into(), Value::String(template.to_string()));
    out.insert(
        "config_path".into(),
        Value::String(target.to_string_lossy().to_string()),
    );
    out.insert(
        "write_config_path".into(),
        Value::String(target.to_string_lossy().to_string()),
    );
    Ok(out)
}

fn render_webhooks_yaml_with_wecom(
    path: &FsPath,
    webhook_url: &str,
    events: &[String],
    template: &str,
) -> Result<String, String> {
    let mut document = if path.exists() {
        let raw = std::fs::read_to_string(path)
            .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
        match serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&raw)
            .map_err(|err| format!("failed to parse {}: {err}", path.display()))?
        {
            serde_yaml_ng::Value::Mapping(map) => map,
            _ => return Err(format!("{} must contain a YAML mapping", path.display())),
        }
    } else {
        serde_yaml_ng::Mapping::new()
    };

    document
        .entry(serde_yaml_ng::Value::String("queue_capacity".into()))
        .or_insert_with(|| serde_yaml_ng::to_value(1024).expect("u64 serializes to YAML"));

    let webhooks_key = serde_yaml_ng::Value::String("webhooks".into());
    let mut webhooks = document
        .remove(&webhooks_key)
        .and_then(|value| match value {
            serde_yaml_ng::Value::Sequence(items) => Some(items),
            _ => None,
        })
        .unwrap_or_default();
    webhooks.retain(|item| !is_wecom_webhook_yaml(item));
    webhooks.push(wecom_webhook_yaml(webhook_url, events, template)?);
    document.insert(webhooks_key, serde_yaml_ng::Value::Sequence(webhooks));

    serde_yaml_ng::to_string(&serde_yaml_ng::Value::Mapping(document))
        .map_err(|err| format!("failed to render WeCom webhooks YAML: {err}"))
}

fn wecom_webhook_yaml(
    webhook_url: &str,
    events: &[String],
    template: &str,
) -> Result<serde_yaml_ng::Value, String> {
    serde_yaml_ng::to_value(json!({
        "name": "wecom-message-push",
        "kind": "wecom",
        "url": webhook_url,
        "events": events,
        "message_template": template,
    }))
    .map_err(|err| format!("failed to render WeCom webhook YAML: {err}"))
}

fn is_wecom_webhook_yaml(item: &serde_yaml_ng::Value) -> bool {
    let kind = yaml_string(item.get("kind")).unwrap_or_default();
    let name = yaml_string(item.get("name")).unwrap_or_default();
    kind == "wecom" || name == "wecom-message-push"
}

fn sanitize_sentry_config(
    config: Map<String, Value>,
) -> Result<Map<String, Value>, (StatusCode, String)> {
    let mut out = Map::new();
    if let Some(dsn) = optional_string_field(&config, "dsn")? {
        if !sentry_dsn_looks_valid(&dsn) {
            return Err((StatusCode::BAD_REQUEST, "invalid Sentry DSN URL".into()));
        }
        out.insert("dsn".into(), Value::String(dsn));
    }
    for key in ["environment", "release"] {
        if let Some(value) = optional_string_field(&config, key)? {
            out.insert(key.into(), Value::String(value));
        }
    }
    if let Some(value) = config.get("sample_rate")
        && !value.is_null()
    {
        let sample_rate = value
            .as_f64()
            .or_else(|| value.as_str().and_then(|raw| raw.parse::<f64>().ok()))
            .ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    "sample_rate must be a number".to_string(),
                )
            })?;
        if !(0.0..=1.0).contains(&sample_rate) {
            return Err((
                StatusCode::BAD_REQUEST,
                "sample_rate must be between 0.0 and 1.0".into(),
            ));
        }
        out.insert("sample_rate".into(), Value::from(sample_rate));
    }
    Ok(out)
}

fn sanitize_string_config(
    config: Map<String, Value>,
    keys: &[&str],
) -> Result<Map<String, Value>, (StatusCode, String)> {
    let mut out = Map::new();
    for key in keys {
        if let Some(value) = optional_string_field(&config, key)? {
            out.insert((*key).into(), Value::String(value));
        }
    }
    Ok(out)
}

fn optional_string_field(
    config: &Map<String, Value>,
    key: &str,
) -> Result<Option<String>, (StatusCode, String)> {
    let Some(value) = config.get(key) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let Some(raw) = value.as_str() else {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("{key} must be a string or null"),
        ));
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(trimmed.to_string()))
    }
}

fn optional_string_list_field(
    config: &Map<String, Value>,
    key: &str,
) -> Result<Vec<String>, (StatusCode, String)> {
    let Some(value) = config.get(key) else {
        return Ok(Vec::new());
    };
    if value.is_null() {
        return Ok(Vec::new());
    }
    let values = match value {
        Value::String(raw) => split_event_patterns(raw),
        Value::Array(items) => {
            let mut values = Vec::new();
            for item in items {
                let Some(raw) = item.as_str() else {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        format!("{key} array entries must be strings"),
                    ));
                };
                values.extend(split_event_patterns(raw));
            }
            values
        }
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("{key} must be a string, string array, or null"),
            ));
        }
    };
    Ok(values)
}

fn env_lock(key: &str, env_var: &str) -> Value {
    json!({
        "key": key,
        "locked": env_string(env_var).is_some(),
        "env_var": env_var,
    })
}

fn env_string(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn env_f64(key: &str) -> Option<f64> {
    env_string(key).and_then(|value| value.parse::<f64>().ok())
}

fn env_event_patterns(key: &str) -> Vec<String> {
    env_string(key)
        .map(|raw| split_event_patterns(&raw))
        .filter(|events| !events.is_empty())
        .unwrap_or_else(default_wecom_events)
}

fn default_wecom_events() -> Vec<String> {
    vec!["tool.failed".into(), "webhook.delivery_failed".into()]
}

fn split_event_patterns(raw: &str) -> Vec<String> {
    raw.split([',', '\n'])
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn sentry_dsn_looks_valid(value: &str) -> bool {
    reqwest::Url::parse(value).is_ok_and(|url| {
        url_is_http_or_https(&url)
            && !url.username().is_empty()
            && url.host_str().is_some()
            && !url.path().trim_matches('/').is_empty()
    })
}

fn http_url_looks_valid(value: &str) -> bool {
    reqwest::Url::parse(value)
        .is_ok_and(|url| url_is_http_or_https(&url) && url.host_str().is_some())
}

fn url_is_http_or_https(url: &reqwest::Url) -> bool {
    matches!(url.scheme(), "http" | "https")
}

fn mask_secret_url(value: &str) -> String {
    if let Ok(mut url) = reqwest::Url::parse(value) {
        if !url.username().is_empty() {
            let _ = url.set_username("********");
        }
        if url.password().is_some() {
            let _ = url.set_password(Some("********"));
        }
        return url.to_string();
    }
    if value.len() <= 12 {
        "********".into()
    } else {
        format!("{}********{}", &value[..4], &value[value.len() - 4..])
    }
}

fn mask_webhook_url(value: &str) -> String {
    let Ok(mut url) = reqwest::Url::parse(value) else {
        return mask_secret_url(value);
    };
    if !url.username().is_empty() {
        let _ = url.set_username("********");
    }
    if url.password().is_some() {
        let _ = url.set_password(Some("********"));
    }
    if url.query().is_some() {
        let pairs = url
            .query_pairs()
            .map(|(key, value)| {
                let masked = matches!(
                    key.as_ref().to_ascii_lowercase().as_str(),
                    "key" | "token" | "secret" | "access_token"
                );
                (
                    key.into_owned(),
                    if masked {
                        "********".to_string()
                    } else {
                        value.into_owned()
                    },
                )
            })
            .collect::<Vec<_>>();
        url.query_pairs_mut().clear().extend_pairs(
            pairs
                .iter()
                .map(|(key, value)| (key.as_str(), value.as_str())),
        );
    }
    url.to_string()
}

fn inspect_webhooks_config(path: &FsPath) -> Result<usize, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    inspect_webhooks_config_text(&raw)
        .map_err(|err| format!("failed to parse {}: {err}", path.display()))
}

fn inspect_webhooks_config_text(raw: &str) -> Result<usize, String> {
    let doc: serde_yaml_ng::Value = serde_yaml_ng::from_str(raw).map_err(|err| err.to_string())?;
    let count = doc
        .get("webhooks")
        .and_then(serde_yaml_ng::Value::as_sequence)
        .map_or(0, Vec::len);
    Ok(count)
}

fn read_sentry_config_from_default_file() -> Option<Result<Map<String, Value>, String>> {
    let path = match default_integration_config_path(DEFAULT_SENTRY_CONFIG_FILE) {
        Ok(path) => path,
        Err(message) => return Some(Err(message)),
    };
    if !path.exists() {
        return None;
    }
    Some(read_sentry_config_from_file(&path))
}

fn read_sentry_config_from_file(path: &FsPath) -> Result<Map<String, Value>, String> {
    let mut config = read_json_config(path)?;
    let Some(dsn) = config.get("dsn").and_then(Value::as_str) else {
        return Err(format!("{} does not contain a Sentry DSN", path.display()));
    };
    if !sentry_dsn_looks_valid(dsn) {
        return Err(format!("Sentry DSN in {} is invalid", path.display()));
    }
    config.insert("dsn".into(), Value::String(mask_secret_url(dsn)));
    config.insert(
        "config_path".into(),
        Value::String(path.to_string_lossy().to_string()),
    );
    Ok(config)
}

fn read_otlp_config_from_default_file() -> Option<Result<Map<String, Value>, String>> {
    let path = match default_integration_config_path(DEFAULT_OTLP_CONFIG_FILE) {
        Ok(path) => path,
        Err(message) => return Some(Err(message)),
    };
    if !path.exists() {
        return None;
    }
    Some(read_json_config(&path).map(|mut config| {
        config.insert(
            "config_path".into(),
            Value::String(path.to_string_lossy().to_string()),
        );
        config
    }))
}

fn read_json_config(path: &FsPath) -> Result<Map<String, Value>, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    let value: Value = serde_json::from_str(&raw)
        .map_err(|err| format!("failed to parse {}: {err}", path.display()))?;
    value
        .as_object()
        .cloned()
        .ok_or_else(|| format!("{} must contain a JSON object", path.display()))
}

fn default_webhooks_config_template() -> String {
    "queue_capacity: 1024\nwebhooks:\n  - name: studio-events\n    url: http://127.0.0.1:9000/dcc-mcp-events\n    events:\n      - tool.failed\n      - gateway.instance.*\n".into()
}

fn write_json_config(path: &FsPath, config: &Map<String, Value>) -> Result<(), String> {
    let body = serde_json::to_string_pretty(config)
        .map_err(|err| format!("failed to render {}: {err}", path.display()))?;
    write_text_config(path, &body)
}

fn write_text_config(path: &FsPath, body: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create {}: {err}", parent.display()))?;
    }
    let body = if body.ends_with('\n') {
        body.to_string()
    } else {
        format!("{body}\n")
    };
    std::fs::write(path, body).map_err(|err| format!("failed to write {}: {err}", path.display()))
}

fn default_integration_config_path(file_name: &str) -> Result<PathBuf, String> {
    integration_etc_dir().map(|dir| dir.join(file_name))
}

fn default_webhooks_config_path() -> Result<PathBuf, String> {
    default_integration_config_path(DEFAULT_WEBHOOKS_CONFIG_FILE)
}

fn writable_webhooks_config_path() -> Result<PathBuf, String> {
    default_webhooks_config_path()
}

fn insert_write_config_path(config: &mut Map<String, Value>, file_name: &str) {
    if let Ok(path) = default_integration_config_path(file_name) {
        config.insert(
            "write_config_path".into(),
            Value::String(path.to_string_lossy().to_string()),
        );
    }
}

fn integration_etc_dir() -> Result<PathBuf, String> {
    if let Some(path) = env_string(ENV_DCC_MCP_ETC_DIR) {
        return Ok(PathBuf::from(path));
    }
    home_dir()
        .map(|home| home.join("dcc-mcp").join("etc"))
        .ok_or_else(|| {
            format!(
                "unable to resolve home directory; set {ENV_DCC_MCP_ETC_DIR} to a writable config directory"
            )
        })
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            let drive = std::env::var_os("HOMEDRIVE")?;
            let path = std::env::var_os("HOMEPATH")?;
            Some(PathBuf::from(format!(
                "{}{}",
                drive.to_string_lossy(),
                path.to_string_lossy()
            )))
        })
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
}

fn saved_wecom_config_from_default_webhooks() -> Option<Result<Map<String, Value>, String>> {
    let path = match default_webhooks_config_path() {
        Ok(path) => path,
        Err(message) => return Some(Err(message)),
    };
    if !path.exists() {
        return None;
    }
    Some(read_wecom_config_from_webhooks_file(&path))
}

fn read_wecom_config_from_webhooks_file(path: &FsPath) -> Result<Map<String, Value>, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    let doc: serde_yaml_ng::Value = serde_yaml_ng::from_str(&raw)
        .map_err(|err| format!("failed to parse {}: {err}", path.display()))?;
    let webhooks = doc
        .get("webhooks")
        .and_then(serde_yaml_ng::Value::as_sequence)
        .ok_or_else(|| format!("{} does not contain a webhooks list", path.display()))?;
    for item in webhooks {
        let kind = yaml_string(item.get("kind")).unwrap_or_default();
        let name = yaml_string(item.get("name")).unwrap_or_default();
        if kind != "wecom" && name != "wecom-message-push" {
            continue;
        }
        let url = yaml_string(item.get("url")).unwrap_or_default();
        if !http_url_looks_valid(&url) {
            return Err(format!(
                "WeCom webhook url in {} is invalid",
                path.display()
            ));
        }
        let mut config = Map::new();
        config.insert("webhook_url".into(), Value::String(mask_webhook_url(&url)));
        config.insert(
            "event_types".into(),
            Value::Array(
                yaml_string_list(item.get("events"))
                    .into_iter()
                    .map(Value::String)
                    .collect(),
            ),
        );
        config.insert(
            "template".into(),
            Value::String(
                yaml_string(item.get("message_template"))
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| DEFAULT_WECOM_TEMPLATE.to_string()),
            ),
        );
        config.insert(
            "config_path".into(),
            Value::String(path.to_string_lossy().to_string()),
        );
        return Ok(config);
    }
    Ok(Map::new())
}

fn yaml_string(value: Option<&serde_yaml_ng::Value>) -> Option<String> {
    value
        .and_then(serde_yaml_ng::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn yaml_string_list(value: Option<&serde_yaml_ng::Value>) -> Vec<String> {
    match value {
        Some(serde_yaml_ng::Value::Sequence(items)) => items
            .iter()
            .filter_map(|item| yaml_string(Some(item)))
            .collect(),
        Some(serde_yaml_ng::Value::String(raw)) => split_event_patterns(raw),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod integration_config_tests {
    use super::*;

    struct EnvGuard {
        previous: Vec<(&'static str, Option<String>)>,
    }

    impl EnvGuard {
        fn new(values: &[(&'static str, Option<String>)]) -> Self {
            const KEYS: &[&str] = &[
                ENV_WEBHOOKS_CONFIG,
                ENV_DCC_MCP_ETC_DIR,
                "USERPROFILE",
                "HOMEDRIVE",
                "HOMEPATH",
                "HOME",
            ];
            let previous = KEYS
                .iter()
                .map(|key| (*key, std::env::var(key).ok()))
                .collect::<Vec<_>>();
            unsafe {
                for key in KEYS {
                    std::env::remove_var(key);
                }
                for (key, value) in values {
                    match value {
                        Some(value) => std::env::set_var(key, value),
                        None => std::env::remove_var(key),
                    }
                }
            }
            Self { previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unsafe {
                for (key, value) in &self.previous {
                    match value {
                        Some(value) => std::env::set_var(key, value),
                        None => std::env::remove_var(key),
                    }
                }
            }
        }
    }

    fn webhooks_config_text(name: &str) -> String {
        format!(
            "queue_capacity: 16\nwebhooks:\n  - name: {name}\n    url: http://127.0.0.1:9000/hook\n    events:\n      - tool.failed\n"
        )
    }

    #[test]
    fn webhooks_persist_writes_local_etc_and_ignores_requested_path() {
        let _lock = INTEGRATIONS_TEST_ENV_LOCK.lock();
        let dir = tempfile::tempdir().expect("tempdir");
        let etc_dir = dir.path().join("etc");
        let requested = dir.path().join("outside.yaml");
        let _env = EnvGuard::new(&[(ENV_DCC_MCP_ETC_DIR, Some(etc_dir.display().to_string()))]);
        let config_text = webhooks_config_text("notify");
        let mut config = Map::new();
        config.insert("config_text".into(), Value::String(config_text.clone()));
        config.insert(
            "config_path".into(),
            Value::String(requested.display().to_string()),
        );

        let saved = persist_webhooks_config(&config).expect("webhooks config should persist");

        let expected = etc_dir.join(DEFAULT_WEBHOOKS_CONFIG_FILE);
        assert!(expected.exists());
        assert!(!requested.exists());
        assert_eq!(
            saved.get("config_path").and_then(Value::as_str),
            Some(expected.to_string_lossy().as_ref())
        );
        assert_eq!(std::fs::read_to_string(expected).unwrap(), config_text);
        assert_eq!(saved.get("webhook_count"), Some(&Value::from(1)));
    }

    #[test]
    fn webhooks_persist_writes_local_etc_even_when_runtime_config_path_is_set() {
        let _lock = INTEGRATIONS_TEST_ENV_LOCK.lock();
        let dir = tempfile::tempdir().expect("tempdir");
        let etc_dir = dir.path().join("etc");
        let runtime_config = dir.path().join("runtime").join("webhooks.yaml");
        let _env = EnvGuard::new(&[
            (ENV_DCC_MCP_ETC_DIR, Some(etc_dir.display().to_string())),
            (
                ENV_WEBHOOKS_CONFIG,
                Some(runtime_config.display().to_string()),
            ),
        ]);
        let config_text = webhooks_config_text("runtime-notify");
        let mut config = Map::new();
        config.insert("config_text".into(), Value::String(config_text.clone()));

        let saved = persist_webhooks_config(&config).expect("webhooks config should persist");

        let expected = etc_dir.join(DEFAULT_WEBHOOKS_CONFIG_FILE);
        assert!(expected.exists());
        assert!(!runtime_config.exists());
        assert_eq!(
            saved.get("config_path").and_then(Value::as_str),
            Some(expected.to_string_lossy().as_ref())
        );
        assert_eq!(
            saved.get("write_config_path").and_then(Value::as_str),
            Some(expected.to_string_lossy().as_ref())
        );
        assert_eq!(std::fs::read_to_string(expected).unwrap(), config_text);
        assert_eq!(saved.get("webhook_count"), Some(&Value::from(1)));
    }

    #[test]
    fn wecom_persist_preserves_existing_non_wecom_webhooks() {
        let _lock = INTEGRATIONS_TEST_ENV_LOCK.lock();
        let dir = tempfile::tempdir().expect("tempdir");
        let etc_dir = dir.path().join("etc");
        let webhooks_path = etc_dir.join(DEFAULT_WEBHOOKS_CONFIG_FILE);
        std::fs::create_dir_all(&etc_dir).expect("create etc dir");
        std::fs::write(
            &webhooks_path,
            r#"
queue_capacity: 64
webhooks:
  - name: notify
    url: http://127.0.0.1:9000/hook
    events:
      - tool.failed
  - name: wecom-message-push
    kind: wecom
    url: https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=old
    events:
      - old.event
    message_template: Old $event
"#,
        )
        .expect("write existing webhooks config");
        let _env = EnvGuard::new(&[(ENV_DCC_MCP_ETC_DIR, Some(etc_dir.display().to_string()))]);

        let mut config = Map::new();
        config.insert(
            "webhook_url".into(),
            Value::String("https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=new".into()),
        );
        config.insert(
            "event_types".into(),
            Value::Array(vec![
                Value::String("tool.completed".into()),
                Value::String("gateway.instance.*".into()),
            ]),
        );
        config.insert(
            "template".into(),
            Value::String("New $event $dcc-type $url".into()),
        );

        let saved = persist_wecom_config(&config).expect("wecom config should persist");

        assert_eq!(
            saved.get("config_path").and_then(Value::as_str),
            Some(webhooks_path.to_string_lossy().as_ref())
        );
        let raw = std::fs::read_to_string(&webhooks_path).expect("read saved webhooks config");
        assert!(raw.contains("name: notify"));
        assert!(raw.contains("http://127.0.0.1:9000/hook"));
        assert!(raw.contains("queue_capacity: 64"));
        assert!(raw.contains("key=new"));
        assert!(raw.contains("New $event $dcc-type $url"));
        assert!(!raw.contains("key=old"));
        assert!(!raw.contains("old.event"));
        assert_eq!(inspect_webhooks_config_text(&raw), Ok(2));
    }
}

#[derive(Debug, Default, Deserialize)]
pub struct AdminInstancesQuery {
    /// Default: current routable instances. `all` exposes the registry view.
    view: Option<String>,
    /// Compatibility flag for callers that want stale diagnostic rows.
    include_stale: Option<bool>,
    /// Include rows whose owner process is gone. Diagnostic use only.
    include_dead: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
pub struct AdminSkillDetailQuery {
    pub name: Option<String>,
    pub skill_name: Option<String>,
    pub dcc: Option<String>,
    pub dcc_type: Option<String>,
    pub instance_id: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct IssueReportQuery {
    /// Default is public-safe. Use `mode=raw` for local evidence review.
    mode: Option<String>,
    /// Compatibility flag for explicit raw export requests.
    include_raw: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct AdminInstanceUpdateRequest {
    /// Defaults to true: check, then stage the update when one is available.
    apply: Option<bool>,
    /// Defaults to the server binary because instance cards represent backends.
    binary: Option<String>,
}

impl IssueReportQuery {
    fn mode(&self) -> IssueReportMode {
        if self
            .mode
            .as_deref()
            .is_some_and(|value| value.eq_ignore_ascii_case("raw"))
            || self.include_raw.as_deref().is_some_and(|value| {
                matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes")
            })
        {
            IssueReportMode::RawDebugBundle
        } else {
            IssueReportMode::PublicSafe
        }
    }
}

/// `GET /admin/api/instances` — list current routable instances by default.
pub async fn handle_admin_instances(
    State(s): State<AdminState>,
    axum::extract::Query(params): axum::extract::Query<AdminInstancesQuery>,
) -> impl IntoResponse {
    let include_dead = params.include_dead.unwrap_or(false);
    let include_stale = params.include_stale.unwrap_or(false);
    let registry_view = params
        .view
        .as_deref()
        .is_some_and(|v| v.eq_ignore_ascii_case("all") || v.eq_ignore_ascii_case("registry"))
        || include_stale
        || include_dead;

    let registry = s.gateway.registry.read().await;
    let (entries, evicted_dead) = if registry_view {
        if include_dead {
            (s.gateway.all_instances(&registry), 0usize)
        } else {
            match s.gateway.read_alive_instances(&registry) {
                Ok((entries, evicted)) => (entries, evicted),
                Err(err) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({
                            "error": "registry-read-failed",
                            "message": err.to_string(),
                        })),
                    )
                        .into_response();
                }
            }
        }
    } else {
        (s.gateway.live_instances(&registry), 0usize)
    };

    let known_total = entries.len();
    let mut live_count = 0usize;
    let mut stale_count = 0usize;
    let mut unhealthy_count = 0usize;
    let instances: Vec<Value> = entries
        .into_iter()
        .filter(|e| {
            let stale = e.is_stale(s.gateway.stale_timeout);
            if stale {
                stale_count += 1;
            }
            registry_view || !stale
        })
        .map(|e| {
            let mut v = s.gateway.instance_json(&e);
            match v["status"].as_str() {
                Some("available" | "busy") => live_count += 1,
                Some("stale") => {}
                _ => unhealthy_count += 1,
            }
            // Alias `instance_id` → `id` for the UI convenience.
            let id = v["instance_id"].clone();
            v.as_object_mut().map(|m| m.insert("id".into(), id));
            v
        })
        .collect();

    Json(json!({
        "total": instances.len(),
        "known_total": known_total,
        "evicted_dead": evicted_dead,
        "view": if registry_view { "all" } else { "live" },
        "summary": {
            "live": live_count,
            "stale": stale_count,
            "unhealthy": unhealthy_count,
        },
        "instances": instances,
    }))
    .into_response()
}

/// `POST /admin/api/instances/{instance_id}/update` — check and optionally stage a server update.
pub async fn handle_admin_instance_update(
    State(s): State<AdminState>,
    Path(instance_filter): Path<String>,
    Json(req): Json<AdminInstanceUpdateRequest>,
) -> Response {
    let binary_name = req
        .binary
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("dcc-mcp-server")
        .to_string();
    let apply = req.apply.unwrap_or(true);

    let instance = match admin_find_instance_entry(&s, &instance_filter).await {
        Ok(entry) => entry,
        Err(response) => return response,
    };
    let instance_id = instance.instance_id.to_string();
    let instance_short_id = instance_short(&instance.instance_id);
    let (current_version, displayed_current_version, current_version_source) =
        admin_instance_update_version(&instance);

    let manifest_url = match &s.gateway.update_manifest_url {
        Some(url) => url.clone(),
        None => {
            return (
                StatusCode::NOT_IMPLEMENTED,
                Json(json!({
                    "status": "not_configured",
                    "error": "update_manifest_url_not_configured",
                    "message": "Update manifest URL is not configured for this gateway.",
                    "hint": "Set DCC_MCP_UPDATE_MANIFEST_URL or configure update_manifest_url on the gateway.",
                    "instance_id": instance_id,
                    "instance_short": instance_short_id,
                    "binary_name": binary_name,
                    "current_version": displayed_current_version,
                    "current_version_source": current_version_source,
                    "update_available": false,
                    "requires_restart": false,
                })),
            )
                .into_response();
        }
    };

    let manifest = match crate::gateway::update_manifest::fetch_update_manifest(
        &s.gateway.http_client,
        &manifest_url,
    )
    .await
    {
        Ok(manifest) => manifest,
        Err(err) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({
                    "status": "manifest_error",
                    "error": "failed_to_fetch_update_manifest",
                    "message": err.to_string(),
                    "instance_id": instance_id,
                    "instance_short": instance_short_id,
                    "binary_name": binary_name,
                    "current_version": displayed_current_version,
                    "current_version_source": current_version_source,
                    "update_available": false,
                    "requires_restart": false,
                })),
            )
                .into_response();
        }
    };

    let Some(manifest_entry) = manifest.get(&binary_name) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({
                "status": "binary_not_found",
                "error": "binary_not_found",
                "message": format!("Binary '{binary_name}' was not found in the update manifest."),
                "instance_id": instance_id,
                "instance_short": instance_short_id,
                "binary_name": binary_name,
                "current_version": displayed_current_version,
                "current_version_source": current_version_source,
                "update_available": false,
                "requires_restart": false,
            })),
        )
            .into_response();
    };

    let update_available =
        crate::gateway::is_newer_version(&manifest_entry.version, &current_version);
    if !update_available || !apply {
        return Json(json!({
            "status": if update_available { "available" } else { "up_to_date" },
            "instance_id": instance_id,
            "instance_short": instance_short_id,
            "binary_name": binary_name,
            "current_version": displayed_current_version,
            "current_version_source": current_version_source,
            "latest_version": manifest_entry.version,
            "download_url": manifest_entry.url,
            "sha256": manifest_entry.sha256,
            "release_notes": manifest_entry.release_notes,
            "update_available": update_available,
            "requires_restart": false,
            "message": if update_available {
                "An update is available."
            } else {
                "Already running the latest available version."
            },
        }))
        .into_response();
    }

    if manifest_entry.url.is_none() {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({
                "status": "download_failed",
                "error": "download_url_not_configured",
                "message": format!("No download URL is configured for binary '{binary_name}'."),
                "instance_id": instance_id,
                "instance_short": instance_short_id,
                "binary_name": binary_name,
                "current_version": displayed_current_version,
                "current_version_source": current_version_source,
                "latest_version": manifest_entry.version,
                "update_available": true,
                "requires_restart": false,
            })),
        )
            .into_response();
    }

    let info = UpdateInfo {
        update_available,
        current_version: current_version.clone(),
        latest_version: manifest_entry.version.clone(),
        download_url: manifest_entry.url.clone(),
        sha256: manifest_entry.sha256.clone(),
        release_notes: manifest_entry.release_notes.clone(),
    };
    let updater = Updater::new("http://127.0.0.1", &binary_name, &current_version);
    let downloaded = match updater.download_update(&info).await {
        Ok(path) => path,
        Err(err) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({
                    "status": "download_failed",
                    "error": "update_download_failed",
                    "message": err.to_string(),
                    "instance_id": instance_id,
                    "instance_short": instance_short_id,
                    "binary_name": binary_name,
                    "current_version": displayed_current_version,
                    "current_version_source": current_version_source,
                    "latest_version": manifest_entry.version,
                    "update_available": true,
                    "requires_restart": false,
                })),
            )
                .into_response();
        }
    };

    if let Err(err) = Updater::stage_update(&downloaded, &binary_name) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "status": "stage_failed",
                "error": "update_stage_failed",
                "message": err.to_string(),
                "instance_id": instance_id,
                "instance_short": instance_short_id,
                "binary_name": binary_name,
                "current_version": displayed_current_version,
                "current_version_source": current_version_source,
                "latest_version": manifest_entry.version,
                "update_available": true,
                "requires_restart": false,
            })),
        )
            .into_response();
    }

    Json(json!({
        "status": "staged",
        "instance_id": instance_id,
        "instance_short": instance_short_id,
        "binary_name": binary_name,
        "current_version": displayed_current_version,
        "current_version_source": current_version_source,
        "latest_version": manifest_entry.version,
        "download_url": manifest_entry.url,
        "sha256": manifest_entry.sha256,
        "release_notes": manifest_entry.release_notes,
        "staged_at": downloaded.to_string_lossy(),
        "update_available": true,
        "requires_restart": true,
        "message": "Update downloaded and staged. Restart the binary to apply.",
    }))
    .into_response()
}

async fn admin_find_instance_entry(
    s: &AdminState,
    instance_filter: &str,
) -> Result<ServiceEntry, Response> {
    let filter = instance_filter.trim();
    if filter.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "missing_instance_id",
                "message": "Instance id is required.",
            })),
        )
            .into_response());
    }

    let registry = s.gateway.registry.read().await;
    let entries = match s.gateway.read_alive_instances(&registry) {
        Ok((entries, _)) => entries,
        Err(_) => s.gateway.all_instances(&registry),
    };
    let filter_lower = filter.to_ascii_lowercase();
    let mut matches = entries
        .into_iter()
        .filter(|entry| {
            let id = entry.instance_id.to_string();
            id.eq_ignore_ascii_case(filter)
                || instance_short(&entry.instance_id).eq_ignore_ascii_case(filter)
                || id.to_ascii_lowercase().starts_with(&filter_lower)
        })
        .collect::<Vec<_>>();

    match matches.len() {
        1 => Ok(matches.remove(0)),
        0 => Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "instance_not_found",
                "message": format!("No live instance matches '{filter}'."),
            })),
        )
            .into_response()),
        _ => Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "ambiguous_instance_id",
                "message": format!("Instance id prefix '{filter}' matches multiple live instances."),
                "matches": matches
                    .iter()
                    .map(|entry| entry.instance_id.to_string())
                    .collect::<Vec<_>>(),
            })),
        )
            .into_response()),
    }
}

fn admin_instance_update_version(entry: &ServiceEntry) -> (String, Option<String>, &'static str) {
    if let Some(version) = entry
        .adapter_version
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return (
            version.to_string(),
            Some(version.to_string()),
            "adapter_version",
        );
    }
    if let Some(version) = entry
        .version
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return (version.to_string(), Some(version.to_string()), "version");
    }
    ("0.0.0".to_string(), None, "unknown")
}

#[derive(Debug, Default, Deserialize)]
pub struct DeregisteredQuery {
    limit: Option<String>,
}

/// `GET /admin/api/deregistered` — recently auto-deregistered registry rows.
pub async fn handle_admin_deregistered(
    State(s): State<AdminState>,
    axum::extract::Query(params): axum::extract::Query<DeregisteredQuery>,
) -> impl IntoResponse {
    let limit = params
        .limit
        .as_deref()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(100)
        .clamp(1, 100);
    let rows = s
        .admin_sqlite_lane
        .as_ref()
        .map(|lane| lane.reader().list_deregistered_instances(limit))
        .unwrap_or_default();

    Json(json!({
        "total": rows.len(),
        "deregistered": rows,
    }))
}

/// `GET /admin/api/tools` — list all registered capability records.
pub async fn handle_admin_tools(State(s): State<AdminState>) -> impl IntoResponse {
    refresh_all_live_backends(&s.gateway, RefreshReason::Periodic).await;
    let records = s.gateway.capability_index.snapshot().records;
    let tools: Vec<Value> = records
        .iter()
        .map(|r| {
            let instance_prefix = instance_short(&r.instance_id);
            json!({
                "slug": r.tool_slug,
                "name": r.backend_tool,
                "dcc_type": r.dcc_type,
                "summary": r.summary,
                "skill_name": r.skill_name,
                "instance_id": r.instance_id.to_string(),
                "instance_prefix": instance_prefix,
            })
        })
        .collect();
    Json(json!({ "total": tools.len(), "tools": tools }))
}

/// `GET /admin/api/skills` — skills currently indexed by the gateway.
pub async fn handle_admin_skills(State(s): State<AdminState>) -> impl IntoResponse {
    reload_skill_paths_and_refresh_backends(&s, RefreshReason::Periodic).await;
    let records = s.gateway.capability_index.snapshot().records;
    Json(crate::gateway::admin::skill_health::build_skill_inventory_payload(&s, records).await)
}

fn admin_skill_query_name(params: &AdminSkillDetailQuery) -> Option<&str> {
    params
        .name
        .as_deref()
        .or(params.skill_name.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn admin_skill_query_dcc(params: &AdminSkillDetailQuery) -> Option<&str> {
    params
        .dcc_type
        .as_deref()
        .or(params.dcc.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn admin_instance_matches_filter(instance: &Value, filter: Option<&str>) -> bool {
    let Some(filter) = filter.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };
    let instance_id = instance
        .get("instance_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let instance_short = instance
        .get("instance_short")
        .and_then(Value::as_str)
        .unwrap_or_default();
    instance_id.eq_ignore_ascii_case(filter)
        || instance_short.eq_ignore_ascii_case(filter)
        || instance_id
            .to_ascii_lowercase()
            .starts_with(&filter.to_ascii_lowercase())
}

fn admin_parse_backend_skill_detail(instance: &Value) -> Value {
    let instance_id = instance
        .get("instance_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let instance_short = instance
        .get("instance_short")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let dcc_type = instance
        .get("dcc_type")
        .and_then(Value::as_str)
        .unwrap_or_default();

    let mut detail = if let Some(result) = instance.get("result").and_then(Value::as_str) {
        serde_json::from_str::<Value>(result).unwrap_or_else(|_| json!({ "message": result }))
    } else if let Some(error) = instance.get("error").and_then(Value::as_str) {
        json!({ "error": error })
    } else {
        json!({})
    };

    if !detail.is_object() {
        detail = json!({ "value": detail });
    }

    if let Some(obj) = detail.as_object_mut() {
        obj.insert("instance_id".to_string(), json!(instance_id));
        obj.insert("instance_short".to_string(), json!(instance_short));
        obj.insert("dcc_type".to_string(), json!(dcc_type));
    }
    detail
}

fn admin_skill_detail_instances(text: &str, instance_filter: Option<&str>) -> Vec<Value> {
    let parsed = serde_json::from_str::<Value>(text).unwrap_or_else(|_| json!({ "message": text }));
    if let Some(instances) = parsed.get("instances").and_then(Value::as_array) {
        return instances
            .iter()
            .filter(|instance| admin_instance_matches_filter(instance, instance_filter))
            .map(admin_parse_backend_skill_detail)
            .collect();
    }
    vec![parsed]
}

/// `GET /admin/api/skill-detail` — raw rendered-review details for one skill.
pub async fn handle_admin_skill_detail(
    State(s): State<AdminState>,
    Query(params): Query<AdminSkillDetailQuery>,
) -> impl IntoResponse {
    let Some(skill_name) = admin_skill_query_name(&params) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "missing required query parameter: name" })),
        );
    };

    reload_skill_paths_and_refresh_backends(&s, RefreshReason::Periodic).await;
    let mut args = json!({ "skill_name": skill_name });
    if let Some(dcc) = admin_skill_query_dcc(&params)
        && let Some(obj) = args.as_object_mut()
    {
        obj.insert("dcc_type".to_string(), json!(dcc));
    }
    if let Some(instance_id) = params
        .instance_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        && let Some(obj) = args.as_object_mut()
    {
        obj.insert("instance_id".to_string(), json!(instance_id));
    }

    let (text, is_error) =
        crate::gateway::aggregator::skill_mgmt_dispatch(&s.gateway, "get_skill_info", &args).await;
    let instances = admin_skill_detail_instances(&text, params.instance_id.as_deref());
    let skill = instances.first().cloned().unwrap_or(Value::Null);
    let status = if is_error && instances.is_empty() {
        StatusCode::BAD_GATEWAY
    } else {
        StatusCode::OK
    };
    (
        status,
        Json(json!({
            "skill": skill,
            "instances": instances,
            "error": if is_error { Some(text) } else { None },
        })),
    )
}

fn admin_audit_row_json(r: &AdminAuditRecord, links: Option<AdminLinkBuilder>) -> Value {
    let ts = r
        .timestamp
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|_d| chrono::DateTime::<chrono::Utc>::from(r.timestamp).to_rfc3339())
        .unwrap_or_default();
    let mut row = json!({
        "timestamp": ts,
        "request_id": r.request_id,
        "trace_id": r.trace_id,
        "span_id": r.span_id,
        "parent_span_id": r.parent_span_id,
        "method": r.method,
        "instance_id": r.instance_id,
        "session_id": r.session_id,
        "transport": r.transport,
        "agent_id": r.agent_id,
        "agent_name": r.agent_name,
        "agent_model": r.agent_model,
        "actor_id": r.actor_id,
        "actor_name": r.actor_name,
        "actor_email_hash": r.actor_email_hash,
        "actor": display_actor_parts(
            r.actor_name.as_deref(),
            r.actor_id.as_deref(),
            r.auth_subject.as_deref(),
            r.actor_email_hash.as_deref(),
        ),
        "client_platform": r.client_platform,
        "client_os": r.client_os,
        "client_host": r.client_host,
        "auth_subject": r.auth_subject,
        "source_ip": r.source_ip,
        "attribution_trust": r.attribution_trust,
        "parent_request_id": r.parent_request_id,
        "tool": r.action,
        "dcc_type": r.dcc_type,
        "status": if r.success { "ok" } else { "err" },
        "success": r.success,
        "error": r.error,
        "duration_ms": r.duration_ms,
    });
    apply_token_fields(&mut row, r.token_accounting.as_ref());
    if let Some(llm) = r.llm_usage.as_ref() {
        row["llm_usage"] = serde_json::to_value(llm).unwrap_or_default();
    }
    if let Some(links) = links {
        row["links"] = links.request_links(&r.request_id);
    }
    row
}

fn display_actor_parts(
    actor_name: Option<&str>,
    actor_id: Option<&str>,
    auth_subject: Option<&str>,
    actor_email_hash: Option<&str>,
) -> Option<String> {
    actor_name
        .or(actor_id)
        .or(auth_subject)
        .or(actor_email_hash)
        .map(ToString::to_string)
}

fn display_actor(ctx: Option<&AgentContext>) -> Option<String> {
    let ctx = ctx?;
    display_actor_parts(
        ctx.actor_name.as_deref(),
        ctx.actor_id.as_deref(),
        ctx.auth_subject.as_deref(),
        ctx.actor_email_hash.as_deref(),
    )
}

fn apply_token_fields(
    row: &mut Value,
    token_accounting: Option<&crate::gateway::admin::trace::TokenTelemetry>,
) {
    let Some(tokens) = token_accounting else {
        return;
    };
    row["token_accounting"] = serde_json::to_value(tokens).unwrap_or(Value::Null);
    row["response_format"] = json!(tokens.response_format.clone());
    row["token_estimator"] = json!(tokens.token_estimator.clone());
    row["original_bytes"] = json!(tokens.original_bytes);
    row["returned_bytes"] = json!(tokens.returned_bytes);
    row["original_tokens"] = json!(tokens.original_tokens);
    row["returned_tokens"] = json!(tokens.returned_tokens);
    row["saved_tokens"] = json!(tokens.saved_tokens);
    row["savings_pct"] = json!(tokens.savings_pct);
}

fn payload_token_accounting(input: Option<usize>, output: Option<usize>) -> Value {
    let total = match (input, output) {
        (Some(input), Some(output)) => Some(input.saturating_add(output)),
        (Some(input), None) => Some(input),
        (None, Some(output)) => Some(output),
        (None, None) => None,
    };
    json!({
        "kind": "payload",
        "token_estimator": TOKEN_ESTIMATOR,
        "input_tokens": input,
        "output_tokens": output,
        "total_tokens": total,
        "has_input_tokens": input.is_some(),
        "has_output_tokens": output.is_some(),
        "missing_payload_tokens": input.is_none() && output.is_none(),
    })
}

/// `GET /admin/api/calls` — recent calls from the AuditLog ring buffer.
///
/// If no `AuditLog` is attached to the state, returns an empty array.
pub async fn handle_admin_calls(
    State(s): State<AdminState>,
    axum::extract::Query(params): axum::extract::Query<DebugListQuery>,
    headers: HeaderMap,
    OriginalUri(uri): OriginalUri,
) -> impl IntoResponse {
    let links = Some(AdminLinkBuilder::from_request(&headers, &uri));
    let limit = params.limit(200, 1_000);
    let mut by_rid: HashMap<String, Value> = HashMap::new();
    if let Some(ref lane) = s.admin_sqlite_lane {
        let r = lane.reader();
        for rec in r.list_audits_recent(limit.saturating_mul(4).max(500)) {
            by_rid.insert(
                rec.request_id.clone(),
                admin_audit_row_json(&rec, links.clone()),
            );
        }
    }
    if let Some(log) = &s.audit_log {
        for r in log.lock().iter().rev().take(limit) {
            by_rid.insert(r.request_id.clone(), admin_audit_row_json(r, links.clone()));
        }
    }
    let mut calls: Vec<Value> = by_rid.into_values().collect();
    calls.sort_by(|a, b| {
        let ta = a.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");
        let tb = b.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");
        tb.cmp(ta)
    });
    calls.truncate(limit);
    Json(json!({ "total": calls.len(), "calls": calls }))
}

/// `GET /admin/api/logs` — gateway contention events (same ring as
/// `resources://gateway/events`).
///
/// Rows are normalised to `{timestamp, level, message}` for the embedded admin
/// UI. Data comes from [`GatewayState::event_log`] (same ring as
/// `resources://gateway/events`).
pub async fn handle_admin_logs(
    State(s): State<AdminState>,
    axum::extract::Query(params): axum::extract::Query<DebugListQuery>,
) -> impl IntoResponse {
    let limit = params.limit(500, 1_000);
    let mut logs: Vec<Value> = s
        .gateway
        .event_log
        .recent_events(limit)
        .into_iter()
        .map(contend_event_to_admin_row)
        .collect();

    // Merge on-disk log files (issue #963).
    let log_dir = std::env::var(ENV_DCC_MCP_LOG_DIR).unwrap_or_else(|_| {
        #[cfg(test)]
        {
            String::new()
        }
        #[cfg(not(test))]
        {
            dcc_mcp_db::default_gateway_log_dir()
        }
    });
    let file_log_task = tokio::task::spawn_blocking(move || {
        if !std::fs::metadata(&log_dir)
            .map(|m| m.is_dir())
            .unwrap_or(false)
        {
            return Vec::new();
        }
        read_gateway_log_dir_rows_recent(&log_dir, limit)
    });
    match tokio::time::timeout(ADMIN_FILE_LOG_READ_TIMEOUT, file_log_task).await {
        Ok(Ok(mut file_logs)) => logs.append(&mut file_logs),
        Ok(Err(err)) => {
            tracing::warn!(error = %err, "admin file log read task failed");
        }
        Err(_) => {
            tracing::warn!(
                timeout_ms = ADMIN_FILE_LOG_READ_TIMEOUT.as_millis() as u64,
                "admin file log read timed out"
            );
        }
    }

    if let Some(audit) = &s.audit_log {
        let records = audit.lock().clone();
        for r in records.iter().rev().take(limit) {
            let ts = r
                .timestamp
                .duration_since(UNIX_EPOCH)
                .ok()
                .map(|_| {
                    chrono::DateTime::<chrono::Utc>::from(r.timestamp)
                        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
                })
                .unwrap_or_default();
            let inst = r.instance_id.as_deref().unwrap_or("-");
            let tool = r.action.as_str();
            let msg = format!(
                "{} {} {}ms — {}",
                r.method.as_deref().unwrap_or("call"),
                if r.success { "ok" } else { "err" },
                r.duration_ms
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "?".into()),
                tool
            );
            logs.push(json!({
                "timestamp": ts,
                "level": if r.success { "info" } else { "warn" },
                "message": msg,
                "source": "audit",
                "dcc_type": r.dcc_type,
                "instance_id": r.instance_id,
                "request_id": r.request_id,
                "tool": tool,
                "success": r.success,
                "detail": format!("instance={inst}"),
                "token_accounting": r.token_accounting.as_ref(),
            }));
        }
    }

    logs.sort_by(|a, b| {
        let ta = a.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");
        let tb = b.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");
        tb.cmp(ta)
    });
    logs.truncate(limit);

    Json(json!({ "total": logs.len(), "logs": logs }))
}

/// `GET /admin/api/health` — service health summary.
pub async fn handle_admin_health(State(s): State<AdminState>) -> impl IntoResponse {
    let registry = s.gateway.registry.read().await;
    let all = s.gateway.all_instances(&registry);
    let ready = s.gateway.live_instances(&registry).len();
    let gateway_sentinels = registry.list_instances(GATEWAY_SENTINEL_DCC_TYPE);
    let total = all.len();
    drop(registry);

    let uptime_secs = s.started_at.elapsed().unwrap_or_default().as_secs();

    let status = if ready > 0 || total == 0 {
        "ok"
    } else {
        "degraded"
    };

    let limits = gateway_limits();
    let circuits = gw_resilience::circuits().snapshot_json();
    let rss_bytes = gateway_self_rss_bytes();

    (
        StatusCode::OK,
        Json(json!({
            "status": status,
            "instances_ready": ready,
            "instances_total": total,
            "uptime_secs": uptime_secs,
            "version": s.gateway.server_version,
            "rss_bytes": rss_bytes,
            "response_format": {
                "default": default_rest_response_format().as_str(),
                "legacy_mime": JSON_MIME,
                "compact_mime": TOON_MIME,
                "token_estimator": TOKEN_ESTIMATOR,
            },
            "gateway": gateway_health_snapshot(&gateway_sentinels),
            "limits": {
                "body_max_bytes": limits.body_max_bytes,
                "rate_limit_per_minute_per_ip": limits.rate_limit_per_minute_per_ip,
                "xff_trusted_depth": limits.xff_trusted_depth,
                "read_retry_max": limits.read_retry_max,
                "circuit_failure_threshold": limits.circuit_failure_threshold,
                "circuit_open_secs": limits.circuit_open_secs,
            },
            "circuits": circuits,
        })),
    )
}

fn gateway_health_snapshot(sentinels: &[ServiceEntry]) -> Value {
    let mut rows: Vec<Value> = sentinels.iter().map(gateway_sentinel_json).collect();
    rows.sort_by(|a, b| {
        let role_a = a.get("role").and_then(Value::as_str).unwrap_or("");
        let role_b = b.get("role").and_then(Value::as_str).unwrap_or("");
        let rank_a = if role_a == "active" { 0 } else { 1 };
        let rank_b = if role_b == "active" { 0 } else { 1 };
        rank_a.cmp(&rank_b).then_with(|| {
            let ta = a
                .get("last_heartbeat_unix")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let tb = b
                .get("last_heartbeat_unix")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            tb.cmp(&ta)
        })
    });
    let current = rows
        .iter()
        .find(|row| row.get("role").and_then(Value::as_str) == Some("active"))
        .cloned()
        .or_else(|| rows.first().cloned());
    let candidates: Vec<Value> = rows
        .into_iter()
        .filter(|row| row.get("role").and_then(Value::as_str) != Some("active"))
        .collect();
    json!({
        "current": current,
        "candidates": candidates,
    })
}

fn gateway_sentinel_json(entry: &ServiceEntry) -> Value {
    let last_heartbeat_secs = entry
        .last_heartbeat
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs());
    let role = entry
        .metadata
        .get("gateway_role")
        .cloned()
        .unwrap_or_else(|| "active".to_string());
    let name = entry
        .metadata
        .get("gateway_name")
        .cloned()
        .or_else(|| entry.display_name.clone())
        .unwrap_or_else(|| format!("gateway-pid{}", entry.pid.unwrap_or_default()));
    json!({
        "name": name,
        "role": role,
        "pid": entry.pid,
        "host": entry.host,
        "port": entry.port,
        "instance_id": entry.instance_id.to_string(),
        "version": entry.version,
        "adapter_version": entry.adapter_version,
        "adapter_dcc": entry.adapter_dcc,
        "last_heartbeat_unix": last_heartbeat_secs,
        "metadata": entry.metadata,
    })
}

fn gateway_self_rss_bytes() -> Option<u64> {
    use sysinfo::{Pid, ProcessesToUpdate, System};
    let mut sys = System::new();
    let pid = Pid::from_u32(std::process::id());
    sys.refresh_processes(ProcessesToUpdate::Some(std::slice::from_ref(&pid)), true);
    sys.process(pid).map(|p| p.memory())
}
/// `GET /admin/api/traces?limit=200` — recent per-call dispatch traces (Phase 2).
///
/// Each trace includes a waterfall of [`TraceSpan`]s plus optionally the
/// request / response payloads captured in `handle_tools_call`.
/// Returns `{"total": N, "traces": [...]}`.  When no `TraceLog` is attached
/// to the state, returns an empty array.
pub async fn handle_admin_traces(
    State(s): State<AdminState>,
    headers: HeaderMap,
    OriginalUri(uri): OriginalUri,
    axum::extract::Query(params): axum::extract::Query<DebugListQuery>,
) -> impl IntoResponse {
    let links = AdminLinkBuilder::from_request(&headers, &uri);
    let limit = params.limit(200, 500);
    let mut by_id: HashMap<String, DispatchTrace> = HashMap::new();
    if let Some(ref lane) = s.admin_sqlite_lane {
        let r = lane.reader();
        for t in r.list_traces_since(None, limit.saturating_mul(4).max(500)) {
            by_id.insert(t.request_id.clone(), t);
        }
    }
    if let Some(log) = &s.trace_log {
        for t in log.recent(limit) {
            by_id.insert(t.request_id.clone(), t);
        }
    }
    let mut traces: Vec<DispatchTrace> = by_id.into_values().collect();
    traces.sort_by(|a, b| {
        let ta = a.started_at.duration_since(UNIX_EPOCH).ok();
        let tb = b.started_at.duration_since(UNIX_EPOCH).ok();
        tb.cmp(&ta)
    });
    traces.truncate(limit);
    let mapped: Vec<Value> = traces
        .iter()
        .map(|trace| dispatch_trace_to_admin_row(trace, Some(links.clone())))
        .collect();
    let payload = json!({
        "total": mapped.len(),
        "traces": mapped,
        "links": {
            "admin_traces_url": links.panel_url("traces"),
            "stats_url": links.panel_url("stats"),
        }
    });
    let compact = crate::gateway::admin::compact::compact_trace_list_payload(&payload);
    debug_response(&headers, &params, StatusCode::OK, payload, Some(compact))
}

/// `GET /admin/api/traces/{request_id}` — full waterfall for one call.
///
/// Returns 404 when the trace is not in the ring buffer or SQLite store.
pub async fn handle_admin_trace_detail(
    State(s): State<AdminState>,
    headers: HeaderMap,
    OriginalUri(uri): OriginalUri,
    axum::extract::Query(params): axum::extract::Query<DebugListQuery>,
    axum::extract::Path(request_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let links = AdminLinkBuilder::from_request(&headers, &uri);
    if let Some(trace) = s.trace_log.as_ref().and_then(|log| log.get(&request_id)) {
        let payload = trace_detail_json(&trace, Some(links.request_links(&request_id)));
        let compact = crate::gateway::admin::compact::compact_trace_detail_payload(&payload);
        return debug_response(&headers, &params, StatusCode::OK, payload, Some(compact));
    }
    if let Some(ref lane) = s.admin_sqlite_lane {
        let r = lane.reader();
        if let Some(trace) = r.get_trace(&request_id) {
            let payload = trace_detail_json(&trace, Some(links.request_links(&request_id)));
            let compact = crate::gateway::admin::compact::compact_trace_detail_payload(&payload);
            return debug_response(&headers, &params, StatusCode::OK, payload, Some(compact));
        }
    }
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": "trace not found", "request_id": request_id })),
    )
        .into_response()
}

/// `GET /admin/api/tasks` — task-like projection over retained gateway work.
pub async fn handle_admin_tasks(
    State(s): State<AdminState>,
    axum::extract::Query(params): axum::extract::Query<DebugListQuery>,
    headers: HeaderMap,
    OriginalUri(uri): OriginalUri,
) -> impl IntoResponse {
    let limit = params.limit(200, 1_000);
    let links = AdminLinkBuilder::from_request(&headers, &uri);
    Json(crate::gateway::admin::activity::build_tasks_payload(&s, limit, links).await)
}

/// `GET /admin/api/workflows` — agent/session workflow projection over
/// retained search telemetry, traces, and audit rows.
pub async fn handle_admin_workflows(
    State(s): State<AdminState>,
    axum::extract::Query(params): axum::extract::Query<DebugListQuery>,
    headers: HeaderMap,
    OriginalUri(uri): OriginalUri,
) -> impl IntoResponse {
    let limit = params.limit(100, 500);
    let links = AdminLinkBuilder::from_request(&headers, &uri);
    Json(crate::gateway::admin::workflows::build_workflows_payload(&s, limit, links).await)
}

/// `GET /admin/api/debug-bundle/{request_id}` — correlated material for one request.
pub async fn handle_admin_debug_bundle(
    State(s): State<AdminState>,
    headers: HeaderMap,
    OriginalUri(uri): OriginalUri,
    axum::extract::Query(params): axum::extract::Query<DebugListQuery>,
    axum::extract::Path(request_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let links = AdminLinkBuilder::from_request(&headers, &uri);
    match crate::gateway::admin::activity::build_debug_bundle(&s, &request_id).await {
        Some(mut bundle) => {
            let resolved_request_id = bundle
                .get("request_id")
                .and_then(Value::as_str)
                .unwrap_or(&request_id)
                .to_string();
            bundle["links"] = links.request_links(&resolved_request_id);
            let compact = crate::gateway::admin::compact::compact_debug_bundle_payload(&bundle);
            debug_response(&headers, &params, StatusCode::OK, bundle, Some(compact))
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "debug bundle not found", "request_id": request_id })),
        )
            .into_response(),
    }
}

/// `GET /v1/debug/traces/{lookup_id}` — trace lookup by trace id or request id.
pub async fn handle_v1_debug_trace_lookup(
    State(s): State<AdminState>,
    headers: HeaderMap,
    OriginalUri(uri): OriginalUri,
    Query(params): Query<DebugListQuery>,
    Path(lookup_id): Path<String>,
) -> impl IntoResponse {
    let links = AdminLinkBuilder::from_request(&headers, &uri);
    match crate::gateway::admin::activity::build_debug_bundle(&s, &lookup_id).await {
        Some(bundle) => {
            let request_id = bundle
                .get("request_id")
                .and_then(Value::as_str)
                .unwrap_or(&lookup_id);
            let payload = json!({
                "lookup_id": lookup_id,
                "trace_id": bundle.get("trace_id").cloned().unwrap_or(Value::Null),
                "request_id": request_id,
                "request_ids": bundle.get("request_ids").cloned().unwrap_or_else(|| json!([])),
                "trace": bundle.get("trace").cloned().unwrap_or(Value::Null),
                "traces": bundle.get("traces").cloned().unwrap_or_else(|| json!([])),
                "links": links.request_links(request_id),
            });
            let compact = crate::gateway::admin::compact::compact_trace_context_payload(&payload);
            debug_response(&headers, &params, StatusCode::OK, payload, Some(compact))
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "trace not found", "lookup_id": lookup_id })),
        )
            .into_response(),
    }
}

/// `GET /admin/api/issue-report/{request_id}` — export a GitHub-attachable JSON report.
pub async fn handle_admin_issue_report(
    State(s): State<AdminState>,
    headers: HeaderMap,
    OriginalUri(uri): OriginalUri,
    Query(params): Query<IssueReportQuery>,
    axum::extract::Path(request_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let links = AdminLinkBuilder::from_request(&headers, &uri);
    match crate::gateway::admin::activity::build_debug_bundle(&s, &request_id).await {
        Some(mut bundle) => {
            let request_links = links.request_links(&request_id);
            bundle["links"] = request_links.clone();
            let report = issue_report_json(&request_id, bundle, request_links, params.mode());
            let mut response = (StatusCode::OK, Json(report)).into_response();
            if let Ok(value) = HeaderValue::from_str(&format!(
                "attachment; filename=\"{}\"",
                issue_report_filename(&request_id)
            )) {
                response
                    .headers_mut()
                    .insert(header::CONTENT_DISPOSITION, value);
            }
            response
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "issue report not found", "request_id": request_id })),
        )
            .into_response(),
    }
}

/// `GET /admin/api/stats?range=1h|24h|7d` — aggregated call statistics (Phase 3).
///
/// Computes on-demand from the [`TraceLog`] ring buffer: call count, success
/// rate, latency percentiles, top-N tools, top-N instances, and hour-of-day
/// distribution.  Returns `{"range":"...", "total_calls":N, ...}`.
pub async fn handle_admin_stats(
    State(s): State<AdminState>,
    headers: HeaderMap,
    axum::extract::Query(params): axum::extract::Query<DebugListQuery>,
) -> impl IntoResponse {
    use crate::gateway::admin::stats::StatsRange;

    let range_str = params.range();
    let range = StatsRange::from_str(range_str);

    match &s.stats {
        Some(agg) => {
            let stats = agg.compute(range);
            let mut root = serde_json::to_value(&stats).unwrap_or(json!({}));
            if let Some(obj) = root.as_object_mut() {
                obj.insert("p50_ms".to_string(), json!(stats.latency_ms.p50_ms));
                obj.insert("p95_ms".to_string(), json!(stats.latency_ms.p95_ms));
                obj.insert(
                    "governance".to_string(),
                    crate::gateway::admin::governance::build_governance_stats(&s),
                );
                obj.insert(
                    "avg_tokens_per_call".to_string(),
                    json!(stats.avg_total_tokens_per_call),
                );
                obj.insert(
                    "payload_token_estimator".to_string(),
                    json!(TOKEN_ESTIMATOR),
                );
                // Embedded admin UI expects a 0–100 percentage in `success_rate`.
                obj.insert(
                    "success_rate".to_string(),
                    json!(stats.success_rate * 100.0),
                );
            }
            debug_response(&headers, &params, StatusCode::OK, root.clone(), Some(root))
        }
        None => {
            let root = json!({
            "error": "stats aggregator not available — admin feature may be disabled",
            "range": range_str,
            "total_calls": 0,
            "successful_calls": 0,
            "failed_calls": 0,
            "success_rate": 0.0,
            "total_input_tokens": 0,
            "total_output_tokens": 0,
            "total_tokens": 0,
            "avg_input_tokens_per_call": 0.0,
            "avg_output_tokens_per_call": 0.0,
            "avg_total_tokens_per_call": 0.0,
            "avg_tokens_per_call": 0.0,
            "payload_token_estimator": TOKEN_ESTIMATOR,
            "payload_token_usage": crate::gateway::admin::stats::PayloadTokenUsageStats::empty(0),
            "token_usage": crate::gateway::admin::stats::TokenUsageStats::default(),
            "governance": crate::gateway::admin::governance::build_governance_stats(&s),
            });
            debug_response(&headers, &params, StatusCode::OK, root.clone(), Some(root))
        }
    }
}

/// `GET /admin/api/search-telemetry?limit=200` — recent search-quality
/// records plus aggregate hit-rate metrics.
pub async fn handle_admin_search_telemetry(
    State(s): State<AdminState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let limit = params
        .get("limit")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(200)
        .clamp(1, 1_000);
    Json(
        serde_json::to_value(s.gateway.search_telemetry.snapshot(limit)).unwrap_or(json!({
            "stats": {},
            "total": 0,
            "recent": [],
        })),
    )
}

#[derive(Debug, Deserialize)]
pub struct SkillPathAddBody {
    pub path: String,
}

async fn wait_for_custom_skill_path_visible(
    lane: &crate::gateway::admin::sqlite_lane::AdminSqliteLane,
    needle: &str,
) {
    for _ in 0..80 {
        if lane
            .reader()
            .list_custom_skill_paths()
            .iter()
            .any(|(_, p)| p == needle)
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    tracing::warn!(path = %needle, "skill path not visible after 2 s poll — writer may be lagging");
}

async fn wait_until_custom_skill_path_id_removed(
    lane: &crate::gateway::admin::sqlite_lane::AdminSqliteLane,
    id: i64,
) {
    for _ in 0..80 {
        if !lane
            .reader()
            .list_custom_skill_paths()
            .iter()
            .any(|(i, _)| *i == id)
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    tracing::warn!(
        skill_path_id = id,
        "skill path id not removed after 2 s poll — writer may be lagging"
    );
}

fn push_admin_operator_note(state: &AdminState, msg: String) {
    state.gateway.event_log.push(ContendEvent::new(
        EventKind::OperatorNote,
        "admin",
        "gateway",
        Some(msg),
    ));
}

/// `GET /admin/api/skill-paths` — skill search paths (snapshot + SQLite custom).
pub async fn handle_admin_skill_paths(State(s): State<AdminState>) -> impl IntoResponse {
    Json(crate::gateway::admin::skill_health::build_skill_paths_payload(&s))
}

/// `POST /admin/api/skill-paths` — enqueue a custom path; embedder hook may reload disk catalog.
pub async fn handle_admin_skill_path_add(
    State(s): State<AdminState>,
    Json(body): Json<SkillPathAddBody>,
) -> impl IntoResponse {
    let path = body.path.trim().to_string();
    if path.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "path is empty" })),
        )
            .into_response();
    }
    let Some(ref lane) = s.admin_sqlite_lane else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "admin sqlite lane disabled" })),
        )
            .into_response();
    };
    if lane.try_add_skill_path(path.clone()) {
        wait_for_custom_skill_path_visible(lane, &path).await;
        reload_skill_paths_and_refresh_backends(&s, RefreshReason::ToolsListChanged).await;
        push_admin_operator_note(
            &s,
            format!("Custom skill path persisted; catalog reload hook ran: {path}"),
        );
        (StatusCode::OK, Json(json!({ "ok": true, "path": path }))).into_response()
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "persist queue full or sqlite disabled" })),
        )
            .into_response()
    }
}

/// `DELETE /admin/api/skill-paths/{id}` — remove a custom path row.
pub async fn handle_admin_skill_path_delete(
    State(s): State<AdminState>,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> impl IntoResponse {
    let Some(ref lane) = s.admin_sqlite_lane else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "admin sqlite lane disabled" })),
        )
            .into_response();
    };
    if lane.try_delete_skill_path(id) {
        wait_until_custom_skill_path_id_removed(lane, id).await;
        reload_skill_paths_and_refresh_backends(&s, RefreshReason::ToolsListChanged).await;
        push_admin_operator_note(
            &s,
            format!("Custom skill path removed (id={id}); catalog reload hook ran."),
        );
        (StatusCode::OK, Json(json!({ "ok": true, "id": id }))).into_response()
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "persist queue full or sqlite disabled" })),
        )
            .into_response()
    }
}

/// `GET /admin/api/workers` — per-instance worker cards (Phase 4).
///
/// Returns the live registry view of each known instance plus best-effort
/// uptime / heartbeat fields.  CPU and memory are reported as `null` until
/// the per-backend diagnostic resource is wired (separate follow-up — see
/// the `admin::workers` module docs).
pub async fn handle_admin_workers(State(s): State<AdminState>) -> impl IntoResponse {
    let payload = crate::gateway::admin::workers::build_workers_payload(&s.gateway).await;
    Json(payload)
}

fn trace_detail_json(trace: &DispatchTrace, links: Option<Value>) -> Value {
    let mut value = serde_json::to_value(trace).unwrap_or(json!({}));
    let input_tokens = trace.input_tokens();
    let output_tokens = trace.output_tokens();
    let total_tokens = trace.total_tokens();
    if let Some(links) = links {
        value["links"] = links;
    }
    if let Some(obj) = value.as_object_mut() {
        obj.insert("input_tokens".to_string(), json!(input_tokens));
        obj.insert("output_tokens".to_string(), json!(output_tokens));
        obj.insert("total_tokens".to_string(), json!(total_tokens));
        obj.insert("estimated_tokens".to_string(), json!(total_tokens));
        obj.insert("estimated_total_tokens".to_string(), json!(total_tokens));
        obj.insert(
            "payload_token_accounting".to_string(),
            payload_token_accounting(input_tokens, output_tokens),
        );
        obj.insert(
            "payload_token_estimator".to_string(),
            json!(TOKEN_ESTIMATOR),
        );
    }
    value
}

fn dispatch_trace_to_admin_row(t: &DispatchTrace, links: Option<AdminLinkBuilder>) -> Value {
    let ts = chrono::DateTime::<chrono::Utc>::from(t.started_at)
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let tool = t.tool_slug.clone().unwrap_or_else(|| t.method.clone());
    let status = if t.ok { "ok" } else { "err" };
    let (slowest_span_name, slowest_span_ms) = t
        .slowest_span()
        .map(|(span, ms)| (Some(span.name.clone()), Some(ms)))
        .unwrap_or((None, None));
    let input_tokens = t.input_tokens();
    let output_tokens = t.output_tokens();
    let total_tokens = t.total_tokens();
    let agent_id = t
        .agent_context
        .as_ref()
        .and_then(|ctx| ctx.agent_id.clone());
    let agent_name = t
        .agent_context
        .as_ref()
        .and_then(|ctx| ctx.agent_name.clone());
    let agent_model = t
        .agent_context
        .as_ref()
        .and_then(|ctx| ctx.model.clone().or_else(|| ctx.model_version.clone()));
    let actor = display_actor(t.agent_context.as_ref());
    let actor_id = t
        .agent_context
        .as_ref()
        .and_then(|ctx| ctx.actor_id.clone());
    let actor_name = t
        .agent_context
        .as_ref()
        .and_then(|ctx| ctx.actor_name.clone());
    let actor_email_hash = t
        .agent_context
        .as_ref()
        .and_then(|ctx| ctx.actor_email_hash.clone());
    let client_platform = t
        .agent_context
        .as_ref()
        .and_then(|ctx| ctx.client_platform.clone());
    let client_os = t
        .agent_context
        .as_ref()
        .and_then(|ctx| ctx.client_os.clone());
    let client_host = t
        .agent_context
        .as_ref()
        .and_then(|ctx| ctx.client_host.clone());
    let auth_subject = t
        .agent_context
        .as_ref()
        .and_then(|ctx| ctx.auth_subject.clone());
    let source_ip = t
        .agent_context
        .as_ref()
        .and_then(|ctx| ctx.source_ip.clone());
    let attribution_trust = t
        .agent_context
        .as_ref()
        .map(|ctx| ctx.trust.clone())
        .filter(|trust| !trust.is_empty());
    let mut row = json!({
        "timestamp": ts,
        "request_id": t.request_id,
        "trace_id": t.trace_id,
        "span_id": t.span_id,
        "parent_span_id": t.parent_span_id,
        "parent_request_id": t.parent_request_id,
        "tool": tool,
        "status": status,
        "success": t.ok,
        "total_ms": t.total_ms,
        "instance_id": t.instance_id,
        "dcc_type": t.dcc_type,
        "transport": t.transport,
        "agent_id": agent_id,
        "agent_name": agent_name,
        "agent_model": agent_model,
        "actor_id": actor_id,
        "actor_name": actor_name,
        "actor_email_hash": actor_email_hash,
        "actor": actor,
        "client_platform": client_platform,
        "client_os": client_os,
        "client_host": client_host,
        "auth_subject": auth_subject,
        "source_ip": source_ip,
        "attribution_trust": attribution_trust,
        "span_count": t.span_count(),
        "input_bytes": t.input_bytes(),
        "output_bytes": t.output_bytes(),
        "input_tokens": input_tokens,
        "output_tokens": output_tokens,
        "total_tokens": total_tokens,
        "payload_token_accounting": payload_token_accounting(input_tokens, output_tokens),
        "payload_token_estimator": TOKEN_ESTIMATOR,
        "slowest_span_name": slowest_span_name,
        "slowest_span_ms": slowest_span_ms,
    });
    apply_token_fields(&mut row, t.token_accounting.as_ref());
    if let Some(llm) = t.llm_usage.as_ref() {
        row["llm_usage"] = serde_json::to_value(llm).unwrap_or_default();
    }
    if let Some(links) = links {
        row["links"] = links.request_links(&t.request_id);
    }
    row
}

fn contend_event_to_admin_row(e: ContendEvent) -> Value {
    if matches!(e.event, EventKind::OperatorNote) {
        let message = e
            .reason
            .clone()
            .unwrap_or_else(|| "operator note".to_string());
        return json!({
            "timestamp": e.timestamp,
            "level": "info",
            "message": message,
            "source": "admin",
            "event": e.event,
            "dcc_type": e.dcc_type,
            "instance_id": e.instance_id,
            "reason": e.reason,
        });
    }
    let label = e.event.as_label();
    let mut message = format!("{label} dcc_type={} instance={}", e.dcc_type, e.instance_id);
    if let Some(r) = &e.reason {
        message.push_str(" — ");
        message.push_str(r);
    }
    json!({
        "timestamp": e.timestamp,
        "level": "info",
        "message": message,
        "source": "contention",
        "event": e.event,
        "dcc_type": e.dcc_type,
        "instance_id": e.instance_id,
        "reason": e.reason,
    })
}
