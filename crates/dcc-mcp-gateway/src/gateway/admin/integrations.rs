use std::path::{Path as FsPath, PathBuf};

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::state::AdminState;

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
    mut config: Map<String, Value>,
    env_locked_fields: Vec<Value>,
    error: Option<String>,
) -> Value {
    redact_integration_config_for_response(kind, &mut config);
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

fn redact_integration_config_for_response(kind: &str, config: &mut Map<String, Value>) {
    match kind {
        "sentry" => {
            if let Some(value) = config.get("dsn").and_then(Value::as_str) {
                config.insert("dsn".into(), Value::String(mask_secret_url(value)));
            }
        }
        "webhooks" => {
            if let Some(value) = config.get("config_text").and_then(Value::as_str) {
                config.insert(
                    "config_text".into(),
                    Value::String(redact_webhooks_config_text(value)),
                );
            }
        }
        "wecom" => {
            if let Some(value) = config.get("webhook_url").and_then(Value::as_str) {
                config.insert("webhook_url".into(), Value::String(mask_webhook_url(value)));
            }
        }
        "otlp" => {
            if let Some(value) = config.get("headers").and_then(Value::as_str) {
                config.insert("headers".into(), Value::String(mask_header_list(value)));
            }
        }
        _ => {}
    }
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

fn redact_webhooks_config_text(raw: &str) -> String {
    raw.lines()
        .map(redact_webhooks_config_line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn redact_webhooks_config_line(line: &str) -> String {
    let trimmed = line.trim_start();
    let indent_len = line.len() - trimmed.len();
    let indent = &line[..indent_len];
    if let Some((key, value)) = trimmed.split_once(':') {
        let key_lower = key.trim().to_ascii_lowercase();
        let value = value.trim();
        if key_lower == "authorization"
            || key_lower == "token"
            || key_lower == "secret"
            || key_lower == "api_key"
        {
            return format!("{indent}{}: ********", key.trim());
        }
        if key_lower == "url" && !value.is_empty() {
            return format!("{indent}{}: {}", key.trim(), mask_webhook_url(value));
        }
    }
    redact_bearer_token(line)
}

fn redact_bearer_token(value: &str) -> String {
    let Some(index) = value.to_ascii_lowercase().find("bearer ") else {
        return value.to_string();
    };
    let token_start = index + "bearer ".len();
    let token_end = value[token_start..]
        .find(char::is_whitespace)
        .map(|offset| token_start + offset)
        .unwrap_or(value.len());
    format!("{}********{}", &value[..token_start], &value[token_end..])
}

fn mask_header_list(raw: &str) -> String {
    raw.split(',')
        .map(|item| {
            let item = item.trim();
            if item.is_empty() {
                return String::new();
            }
            match item.split_once('=') {
                Some((key, value)) => {
                    let key = key.trim();
                    let value = value.trim();
                    if value.is_empty() {
                        format!("{key}=")
                    } else {
                        format!("{key}=********")
                    }
                }
                None => "********".to_string(),
            }
        })
        .filter(|item| !item.is_empty())
        .collect::<Vec<_>>()
        .join(",")
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
