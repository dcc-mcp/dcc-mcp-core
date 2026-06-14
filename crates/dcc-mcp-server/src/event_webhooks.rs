use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use dcc_mcp_actions::EventBus;
use dcc_mcp_actions::events::EventEnvelope;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

const ENV_WEBHOOKS_CONFIG: &str = "DCC_MCP_WEBHOOKS_CONFIG";
const ENV_DCC_MCP_ETC_DIR: &str = "DCC_MCP_ETC_DIR";
const ENV_WECOM_WEBHOOK_URL: &str = "DCC_MCP_WECOM_WEBHOOK_URL";
const ENV_WECOM_EVENTS: &str = "DCC_MCP_WECOM_EVENTS";
const ENV_WECOM_TEMPLATE: &str = "DCC_MCP_WECOM_TEMPLATE";
const DEFAULT_QUEUE_CAPACITY: usize = 1024;
const DEFAULT_ATTEMPTS: usize = 3;
const DEFAULT_TIMEOUT_MS: u64 = 2_000;
const DEFAULT_BACKOFF_MS: &[u64] = &[200, 1_000, 5_000];
const DELIVERY_FAILED_EVENT: &str = "webhook.delivery_failed";
const DEFAULT_WECOM_TEMPLATE: &str = "DCC-MCP $event\nDCC: $dcc-type\nTool: $tool-slug\nURL: $url";
const DEFAULT_WEBHOOKS_CONFIG_FILE: &str = "webhooks.yaml";

#[derive(Debug, Default, Deserialize)]
struct WebhookConfigDocument {
    #[serde(default = "default_queue_capacity")]
    queue_capacity: usize,
    #[serde(default)]
    webhooks: Vec<WebhookConfig>,
}

#[derive(Debug, Deserialize)]
struct WebhookConfig {
    name: String,
    url: String,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    events: Vec<String>,
    #[serde(default)]
    headers: HashMap<String, String>,
    #[serde(default)]
    delivery: DeliveryConfig,
    #[serde(default)]
    filters: Vec<HashMap<String, Value>>,
    #[serde(default)]
    payload_template: Option<String>,
    #[serde(default)]
    message_template: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct DeliveryConfig {
    #[serde(default = "default_attempts")]
    attempts: usize,
    #[serde(default = "default_timeout_ms")]
    timeout_ms: u64,
    #[serde(default = "default_backoff_ms_vec")]
    backoff_ms: Vec<u64>,
}

impl Default for DeliveryConfig {
    fn default() -> Self {
        Self {
            attempts: default_attempts(),
            timeout_ms: default_timeout_ms(),
            backoff_ms: default_backoff_ms_vec(),
        }
    }
}

#[derive(Clone, Debug)]
struct DeliveryPolicy {
    attempts: usize,
    timeout: Duration,
    backoff: Vec<Duration>,
}

#[derive(Clone, Debug)]
struct EventWebhook {
    name: String,
    url: String,
    kind: WebhookKind,
    events: Vec<String>,
    headers: Vec<(String, String)>,
    delivery: DeliveryPolicy,
    filters: Vec<FilterRule>,
    payload_template: Option<String>,
    message_template: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WebhookKind {
    Generic,
    WeCom,
}

#[derive(Clone, Debug)]
struct FilterRule {
    expected: Vec<(String, Value)>,
}

#[derive(Clone, Debug)]
struct DeliveryTask {
    webhook: EventWebhook,
    event: EventEnvelope,
}

pub(crate) struct EventWebhookRuntime {
    bus: EventBus,
    subscriptions: Vec<(String, u64)>,
    worker: JoinHandle<()>,
}

impl EventWebhookRuntime {
    pub(crate) fn from_env(bus: EventBus) -> Result<Option<Self>> {
        let mut document = if let Some(path) = configured_webhooks_path() {
            Some(read_config_document(&path).with_context(|| {
                format!(
                    "failed to load event webhook configuration from {}",
                    path.display()
                )
            })?)
        } else {
            None
        };
        if let Some(wecom) = wecom_config_from_env()? {
            document
                .get_or_insert_with(WebhookConfigDocument::default)
                .webhooks
                .push(wecom);
        }
        let Some(document) = document else {
            return Ok(None);
        };
        Ok(Some(Self::from_document(bus, document)?))
    }

    fn from_document(bus: EventBus, document: WebhookConfigDocument) -> Result<Self> {
        let webhooks = resolve_webhooks(document.webhooks)?;
        if webhooks.is_empty() {
            tracing::warn!(
                env = ENV_WEBHOOKS_CONFIG,
                "event webhook configuration contains no webhooks"
            );
        }

        let (tx, rx) = mpsc::channel(document.queue_capacity.max(1));
        let mut subscriptions = Vec::new();
        for webhook in &webhooks {
            for event_name in &webhook.events {
                let event_pattern = event_name.clone();
                let sender = tx.clone();
                let webhook = webhook.clone();
                let subscription_id = bus.subscribe_event(event_pattern.clone(), move |event| {
                    if event.name.starts_with("webhook.") {
                        return;
                    }
                    if !webhook.matches(event) {
                        return;
                    }
                    let task = DeliveryTask {
                        webhook: webhook.clone(),
                        event: event.clone(),
                    };
                    if let Err(err) = sender.try_send(task) {
                        tracing::warn!(
                            webhook = %webhook.name,
                            event_id = %event.id,
                            event_name = %event.name,
                            error = %err,
                            "event webhook delivery queue is full; dropping event"
                        );
                    }
                });
                subscriptions.push((event_pattern, subscription_id));
            }
        }
        drop(tx);

        let worker = tokio::spawn(run_delivery_worker(bus.clone(), rx));
        Ok(Self {
            bus,
            subscriptions,
            worker,
        })
    }
}

impl Drop for EventWebhookRuntime {
    fn drop(&mut self) {
        for (event_name, subscriber_id) in self.subscriptions.drain(..) {
            let _ = self.bus.unsubscribe(&event_name, subscriber_id);
        }
        self.worker.abort();
    }
}

async fn run_delivery_worker(bus: EventBus, mut rx: mpsc::Receiver<DeliveryTask>) {
    let client = reqwest::Client::new();
    while let Some(task) = rx.recv().await {
        let client = client.clone();
        let bus = bus.clone();
        tokio::spawn(async move {
            deliver_with_retries(&client, &bus, task).await;
        });
    }
}

async fn deliver_with_retries(client: &reqwest::Client, bus: &EventBus, task: DeliveryTask) {
    let mut last_error = String::new();
    for attempt in 1..=task.webhook.delivery.attempts {
        let body = task.webhook.render_payload(&task.event);
        let mut request = client
            .post(&task.webhook.url)
            .timeout(task.webhook.delivery.timeout)
            .header("content-type", "application/json")
            .header("x-dcc-mcp-event-id", task.event.id.as_str())
            .header("x-dcc-mcp-event-name", task.event.name.as_str())
            .body(body);
        for (name, value) in &task.webhook.headers {
            request = request.header(name, value);
        }

        match request
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
        {
            Ok(_) => return,
            Err(err) => {
                last_error = err.to_string();
                tracing::warn!(
                    webhook = %task.webhook.name,
                    event_id = %task.event.id,
                    event_name = %task.event.name,
                    attempt,
                    attempts = task.webhook.delivery.attempts,
                    error = %last_error,
                    "event webhook delivery failed"
                );
                if attempt < task.webhook.delivery.attempts {
                    tokio::time::sleep(task.webhook.delivery.backoff_for_attempt(attempt)).await;
                }
            }
        }
    }

    let _ = bus.emit(
        DELIVERY_FAILED_EVENT,
        json!({"service": "dcc-mcp-server", "webhook": task.webhook.name}),
        json!({
            "event_id": task.event.id,
            "event_name": task.event.name,
        }),
        json!({
            "webhook": task.webhook.name,
            "url": task.webhook.url,
            "attempts": task.webhook.delivery.attempts,
            "error": last_error,
        }),
    );
}

impl EventWebhook {
    fn matches(&self, event: &EventEnvelope) -> bool {
        if self.filters.is_empty() {
            return true;
        }
        let event_value = event.to_value();
        self.filters.iter().any(|rule| rule.matches(&event_value))
    }

    fn render_payload(&self, event: &EventEnvelope) -> String {
        let event_value = event.to_value();
        match self.kind {
            WebhookKind::Generic => match &self.payload_template {
                Some(template) => render_template(template, &event_value),
                None => serde_json::to_string(&event_value).unwrap_or_else(|_| "{}".to_string()),
            },
            WebhookKind::WeCom => {
                let template = self
                    .message_template
                    .as_deref()
                    .or(self.payload_template.as_deref())
                    .unwrap_or(DEFAULT_WECOM_TEMPLATE);
                let content =
                    render_dollar_template(&render_template(template, &event_value), &event_value);
                serde_json::to_string(&json!({
                    "msgtype": "markdown",
                    "markdown": {
                        "content": content,
                    },
                }))
                .unwrap_or_else(|_| "{}".to_string())
            }
        }
    }
}

impl DeliveryPolicy {
    fn from_config(config: DeliveryConfig) -> Result<Self> {
        if config.attempts == 0 {
            bail!("delivery.attempts must be at least 1");
        }
        if config.timeout_ms == 0 {
            bail!("delivery.timeout_ms must be at least 1");
        }
        let backoff_ms = if config.backoff_ms.is_empty() {
            default_backoff_ms_vec()
        } else {
            config.backoff_ms
        };
        Ok(Self {
            attempts: config.attempts,
            timeout: Duration::from_millis(config.timeout_ms),
            backoff: backoff_ms.into_iter().map(Duration::from_millis).collect(),
        })
    }

    fn backoff_for_attempt(&self, attempt: usize) -> Duration {
        let index = attempt.saturating_sub(1);
        self.backoff
            .get(index)
            .copied()
            .or_else(|| self.backoff.last().copied())
            .unwrap_or_else(|| Duration::from_millis(0))
    }
}

impl FilterRule {
    fn from_map(map: HashMap<String, Value>) -> Result<Self> {
        if map.is_empty() {
            bail!("webhook filter rules must not be empty");
        }
        Ok(Self {
            expected: map.into_iter().collect(),
        })
    }

    fn matches(&self, event_value: &Value) -> bool {
        self.expected
            .iter()
            .all(|(path, expected)| matches_expected(path_value(event_value, path), expected))
    }
}

fn resolve_webhooks(configs: Vec<WebhookConfig>) -> Result<Vec<EventWebhook>> {
    configs
        .into_iter()
        .map(resolve_webhook)
        .collect::<Result<Vec<_>>>()
}

fn resolve_webhook(config: WebhookConfig) -> Result<EventWebhook> {
    let name = config.name.trim().to_string();
    if name.is_empty() {
        bail!("webhook.name must not be empty");
    }
    let url = interpolate_env(config.url.trim()).trim().to_string();
    if url.is_empty() {
        bail!("webhook '{name}' url must not be empty");
    }
    let parsed_url = reqwest::Url::parse(&url)
        .map_err(|err| anyhow!("webhook '{name}' url is invalid: {err}"))?;
    if !matches!(parsed_url.scheme(), "http" | "https") {
        bail!("webhook '{name}' url must use http or https");
    }
    if config.events.is_empty() {
        bail!("webhook '{name}' must subscribe to at least one event");
    }

    let events = config
        .events
        .into_iter()
        .map(|event| event.trim().to_string())
        .filter(|event| !event.is_empty())
        .collect::<Vec<_>>();
    if events.is_empty() {
        bail!("webhook '{name}' must subscribe to at least one non-empty event");
    }

    let headers = config
        .headers
        .into_iter()
        .map(|(key, value)| (key, interpolate_env(&value)))
        .collect();

    let filters = config
        .filters
        .into_iter()
        .map(FilterRule::from_map)
        .collect::<Result<Vec<_>>>()?;
    let kind = match config
        .kind
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("generic")
        .to_ascii_lowercase()
        .as_str()
    {
        "generic" | "http" | "webhook" => WebhookKind::Generic,
        "wecom" | "enterprise_wechat" | "wechat_work" => WebhookKind::WeCom,
        other => bail!("webhook '{name}' kind {other:?} is not supported"),
    };

    Ok(EventWebhook {
        name,
        url,
        kind,
        events,
        headers,
        delivery: DeliveryPolicy::from_config(config.delivery)?,
        filters,
        payload_template: config.payload_template,
        message_template: config.message_template,
    })
}

fn read_config_document(path: &Path) -> Result<WebhookConfigDocument> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    serde_yaml_ng::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
}

fn configured_webhooks_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(ENV_WEBHOOKS_CONFIG).filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(path));
    }
    let path = default_webhooks_config_path()?;
    path.exists().then_some(path)
}

fn default_webhooks_config_path() -> Option<PathBuf> {
    integration_etc_dir().map(|dir| dir.join(DEFAULT_WEBHOOKS_CONFIG_FILE))
}

fn integration_etc_dir() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(ENV_DCC_MCP_ETC_DIR).filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(path));
    }
    home_dir().map(|home| home.join("dcc-mcp").join("etc"))
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

fn wecom_config_from_env() -> Result<Option<WebhookConfig>> {
    let Some(url) = std::env::var(ENV_WECOM_WEBHOOK_URL)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    let events = std::env::var(ENV_WECOM_EVENTS)
        .ok()
        .map(|raw| split_event_patterns(&raw))
        .filter(|events| !events.is_empty())
        .unwrap_or_else(default_wecom_events);
    Ok(Some(WebhookConfig {
        name: "wecom-message-push".into(),
        url,
        kind: Some("wecom".into()),
        events,
        headers: HashMap::new(),
        delivery: DeliveryConfig::default(),
        filters: Vec::new(),
        payload_template: None,
        message_template: Some(
            std::env::var(ENV_WECOM_TEMPLATE)
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| DEFAULT_WECOM_TEMPLATE.to_string()),
        ),
    }))
}

fn matches_expected(actual: Option<&Value>, expected: &Value) -> bool {
    let Some(actual) = actual else {
        return false;
    };
    match (actual, expected) {
        (Value::String(actual), Value::String(expected)) if expected.contains('*') => {
            wildcard_match(expected, actual)
        }
        _ => actual == expected,
    }
}

fn path_value<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = value;
    for segment in path.split('.') {
        if segment.is_empty() {
            return None;
        }
        current = current.get(segment)?;
    }
    Some(current)
}

fn render_template(template: &str, event: &Value) -> String {
    let mut rendered = String::with_capacity(template.len());
    let mut rest = template;
    loop {
        let Some(start) = rest.find("{{") else {
            rendered.push_str(rest);
            break;
        };
        rendered.push_str(&rest[..start]);
        let after_start = &rest[start + 2..];
        let Some(end) = after_start.find("}}") else {
            rendered.push_str(&rest[start..]);
            break;
        };
        let key = after_start[..end].trim();
        rendered.push_str(&template_value(event, key));
        rest = &after_start[end + 2..];
    }
    rendered
}

fn render_dollar_template(template: &str, event: &Value) -> String {
    let mut rendered = String::with_capacity(template.len());
    let mut chars = template.char_indices().peekable();
    while let Some((_, ch)) = chars.next() {
        if ch != '$' {
            rendered.push(ch);
            continue;
        }
        if chars.peek().is_some_and(|(_, next)| *next == '$') {
            let _ = chars.next();
            rendered.push('$');
            continue;
        }
        let mut key = String::new();
        while let Some((_, next)) = chars.peek().copied() {
            if next.is_ascii_alphanumeric() || matches!(next, '_' | '-' | '.') {
                key.push(next);
                let _ = chars.next();
            } else {
                break;
            }
        }
        if key.is_empty() {
            rendered.push('$');
        } else {
            rendered.push_str(&dollar_template_value(event, &key));
        }
    }
    rendered
}

fn dollar_template_value(event: &Value, key: &str) -> String {
    match key {
        "event" | "event-name" | "event_name" => template_value(event, "name"),
        "event-id" | "event_id" => template_value(event, "id"),
        "dcc-type" | "dcc_type" => template_value(event, "source.dcc_type"),
        "instance-id" | "instance_id" => template_value(event, "source.instance_id"),
        "tool" | "tool-slug" | "tool_slug" => template_value(event, "attributes.tool_slug"),
        "skill" | "skill-name" | "skill_name" => template_value(event, "attributes.skill_name"),
        "url" => first_template_value(
            event,
            &[
                "attributes.url",
                "attributes.access_url",
                "attributes.mcp_url",
                "source.url",
                "source.mcp_url",
            ],
        ),
        path => template_value(event, path),
    }
}

fn first_template_value(event: &Value, paths: &[&str]) -> String {
    paths
        .iter()
        .map(|path| template_value(event, path))
        .find(|value| !value.is_empty())
        .unwrap_or_default()
}

fn template_value(event: &Value, path: &str) -> String {
    let Some(value) = path_value(event, path) else {
        return String::new();
    };
    match value {
        Value::Null => String::new(),
        Value::String(value) => value.clone(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::Array(_) | Value::Object(_) => {
            serde_json::to_string(value).unwrap_or_else(|_| String::new())
        }
    }
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }

    let starts_with_wildcard = pattern.starts_with('*');
    let ends_with_wildcard = pattern.ends_with('*');
    let parts = pattern
        .split('*')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();

    if parts.is_empty() {
        return true;
    }

    let mut remainder = value;
    for (index, part) in parts.iter().enumerate() {
        let Some(position) = remainder.find(part) else {
            return false;
        };
        if index == 0 && !starts_with_wildcard && position != 0 {
            return false;
        }
        let next = position + part.len();
        remainder = &remainder[next..];
    }

    ends_with_wildcard || remainder.is_empty()
}

fn interpolate_env(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut rest = value;
    loop {
        let Some(start) = rest.find("${") else {
            output.push_str(rest);
            break;
        };
        output.push_str(&rest[..start]);
        let after_start = &rest[start + 2..];
        let Some(end) = after_start.find('}') else {
            output.push_str(&rest[start..]);
            break;
        };
        let name = &after_start[..end];
        output.push_str(&std::env::var(name).unwrap_or_default());
        rest = &after_start[end + 1..];
    }
    output
}

fn split_event_patterns(raw: &str) -> Vec<String> {
    raw.split([',', '\n'])
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn default_wecom_events() -> Vec<String> {
    vec!["tool.failed".into(), "webhook.delivery_failed".into()]
}

fn default_queue_capacity() -> usize {
    DEFAULT_QUEUE_CAPACITY
}

fn default_attempts() -> usize {
    DEFAULT_ATTEMPTS
}

fn default_timeout_ms() -> u64 {
    DEFAULT_TIMEOUT_MS
}

fn default_backoff_ms_vec() -> Vec<u64> {
    DEFAULT_BACKOFF_MS.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::extract::State;
    use axum::http::StatusCode;
    use axum::routing::post;
    use dcc_mcp_test_utils::EnvVarsGuard;
    use serde_json::Map;
    use tokio::net::TcpListener;

    #[test]
    fn filter_rules_match_dotted_paths_and_wildcards() {
        let rule = FilterRule::from_map(HashMap::from([
            (
                "attributes.skill_name".to_string(),
                Value::String("maya-*".to_string()),
            ),
            (
                "source.dcc_type".to_string(),
                Value::String("maya".to_string()),
            ),
        ]))
        .unwrap();
        let event = EventEnvelope::new(
            "tool.completed",
            "ev_1",
            json!({"dcc_type": "maya"}),
            json!({}),
            json!({"skill_name": "maya-modeling"}),
        )
        .to_value();

        assert!(rule.matches(&event));
        assert!(!matches_expected(
            path_value(&event, "attributes.skill_name"),
            &Value::String("zbrush-*".to_string())
        ));
    }

    #[test]
    fn payload_templates_replace_event_paths() {
        let event = EventEnvelope::new(
            "tool.completed",
            "ev_1",
            json!({"dcc_type": "photoshop"}),
            json!({"request_id": "req-1"}),
            json!({"tool_slug": "ps.layer__rename", "result_success": true}),
        )
        .to_value();

        let rendered = render_template(
            r#"{"text":"{{source.dcc_type}} {{attributes.tool_slug}} {{attributes.result_success}}"}"#,
            &event,
        );

        assert_eq!(rendered, r#"{"text":"photoshop ps.layer__rename true"}"#);
    }

    #[test]
    fn webhook_url_interpolates_environment_variables() {
        let _env = EnvVarsGuard::set(&[
            (ENV_WEBHOOKS_CONFIG, None),
            (ENV_DCC_MCP_ETC_DIR, None),
            (ENV_WECOM_WEBHOOK_URL, Some("abc123")),
            (ENV_WECOM_EVENTS, None),
            (ENV_WECOM_TEMPLATE, None),
        ]);
        let webhook = resolve_webhook(WebhookConfig {
            name: "wecom-alerts".into(),
            url:
                "https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=${DCC_MCP_WECOM_WEBHOOK_URL}"
                    .into(),
            kind: Some("wecom".into()),
            events: vec!["tool.failed".into()],
            headers: HashMap::new(),
            delivery: DeliveryConfig::default(),
            filters: Vec::new(),
            payload_template: None,
            message_template: None,
        })
        .expect("webhook url env interpolation should produce a valid URL");

        assert_eq!(
            webhook.url,
            "https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=abc123"
        );
    }

    #[test]
    fn wecom_payload_renders_markdown_with_dollar_variables() {
        let webhook = EventWebhook {
            name: "wecom-alerts".into(),
            url: "https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=abc".into(),
            kind: WebhookKind::WeCom,
            events: vec!["tool.*".into()],
            headers: Vec::new(),
            delivery: DeliveryPolicy::from_config(DeliveryConfig::default()).unwrap(),
            filters: Vec::new(),
            payload_template: None,
            message_template: Some(
                "DCC-MCP $event\nDCC: $dcc-type\nTool: $tool-slug\nURL: $url".into(),
            ),
        };
        let event = EventEnvelope::new(
            "tool.failed",
            "ev_1",
            json!({"dcc_type": "houdini", "instance_id": "h1"}),
            json!({"request_id": "req-1"}),
            json!({
                "tool_slug": "houdini.render__start",
                "url": "http://127.0.0.1:9765/mcp"
            }),
        );

        let payload: Value = serde_json::from_str(&webhook.render_payload(&event)).unwrap();

        assert_eq!(payload["msgtype"], "markdown");
        assert_eq!(
            payload["markdown"]["content"],
            "DCC-MCP tool.failed\nDCC: houdini\nTool: houdini.render__start\nURL: http://127.0.0.1:9765/mcp"
        );
    }

    #[tokio::test]
    async fn runtime_loads_default_webhooks_config_from_local_etc() -> Result<()> {
        let dir = tempfile::tempdir()?;
        std::fs::write(
            dir.path().join(DEFAULT_WEBHOOKS_CONFIG_FILE),
            r#"
webhooks:
  - name: local-default
    url: http://127.0.0.1:9/hook
    events: ["tool.failed"]
"#,
        )?;
        let etc_dir = dir.path().to_string_lossy().to_string();
        let _env = EnvVarsGuard::set(&[
            (ENV_WEBHOOKS_CONFIG, None),
            (ENV_DCC_MCP_ETC_DIR, Some(&etc_dir)),
            (ENV_WECOM_WEBHOOK_URL, None),
            (ENV_WECOM_EVENTS, None),
            (ENV_WECOM_TEMPLATE, None),
        ]);

        let runtime = EventWebhookRuntime::from_env(EventBus::new())?;

        assert!(runtime.is_some());
        Ok(())
    }

    #[tokio::test]
    async fn runtime_loads_admin_saved_wecom_yaml_from_local_etc() -> Result<()> {
        let (tx, mut rx) = mpsc::channel::<Value>(1);
        let app = Router::new()
            .route("/wecom", post(capture_body))
            .with_state(tx);
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let server = tokio::spawn(async move {
            if let Err(err) = axum::serve(listener, app).await {
                tracing::error!(error = %err, "test webhook server failed");
            }
        });

        let dir = tempfile::tempdir()?;
        std::fs::write(
            dir.path().join(DEFAULT_WEBHOOKS_CONFIG_FILE),
            format!(
                r#"
queue_capacity: 8
webhooks:
  - name: wecom-message-push
    kind: wecom
    url: http://{addr}/wecom
    events:
      - tool.failed
    message_template: |-
      DCC-MCP $event
      DCC: $dcc-type
      Tool: $tool-slug
      URL: $url
"#
            ),
        )?;
        let etc_dir = dir.path().to_string_lossy().to_string();
        let _env = EnvVarsGuard::set(&[
            (ENV_WEBHOOKS_CONFIG, None),
            (ENV_DCC_MCP_ETC_DIR, Some(&etc_dir)),
            (ENV_WECOM_WEBHOOK_URL, None),
            (ENV_WECOM_EVENTS, None),
            (ENV_WECOM_TEMPLATE, None),
        ]);

        let bus = EventBus::new();
        let runtime = EventWebhookRuntime::from_env(bus.clone())?
            .expect("admin-saved local WeCom webhooks.yaml should be loaded");
        assert_eq!(runtime.subscriptions[0].0, "tool.failed");

        let _ = bus.emit(
            "tool.failed",
            json!({"dcc_type": "houdini", "instance_id": "h1"}),
            json!({"request_id": "req-1"}),
            json!({
                "tool_slug": "houdini.render__start",
                "url": "http://127.0.0.1:9765/mcp"
            }),
        );

        let received = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .context("WeCom webhook request timed out")?
            .context("WeCom webhook receiver closed")?;
        assert_eq!(received["msgtype"], "markdown");
        assert_eq!(
            received["markdown"]["content"],
            "DCC-MCP tool.failed\nDCC: houdini\nTool: houdini.render__start\nURL: http://127.0.0.1:9765/mcp"
        );

        drop(runtime);
        server.abort();
        Ok(())
    }

    #[tokio::test]
    async fn empty_explicit_webhooks_config_env_falls_back_to_local_default() -> Result<()> {
        let dir = tempfile::tempdir()?;
        std::fs::write(
            dir.path().join(DEFAULT_WEBHOOKS_CONFIG_FILE),
            r#"
webhooks:
  - name: local-default
    url: http://127.0.0.1:9/hook
    events: ["tool.failed"]
"#,
        )?;
        let etc_dir = dir.path().to_string_lossy().to_string();
        let _env = EnvVarsGuard::set(&[
            (ENV_WEBHOOKS_CONFIG, Some("")),
            (ENV_DCC_MCP_ETC_DIR, Some(&etc_dir)),
            (ENV_WECOM_WEBHOOK_URL, None),
            (ENV_WECOM_EVENTS, None),
            (ENV_WECOM_TEMPLATE, None),
        ]);

        let runtime = EventWebhookRuntime::from_env(EventBus::new())?
            .expect("local default webhooks.yaml should be loaded");

        assert_eq!(runtime.subscriptions.len(), 1);
        assert_eq!(runtime.subscriptions[0].0, "tool.failed");
        Ok(())
    }

    #[tokio::test]
    async fn runtime_loads_wecom_webhook_from_environment_without_yaml() -> Result<()> {
        let _env = EnvVarsGuard::set(&[
            (ENV_WEBHOOKS_CONFIG, None),
            (ENV_DCC_MCP_ETC_DIR, None),
            (
                ENV_WECOM_WEBHOOK_URL,
                Some("https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=abc123"),
            ),
            (ENV_WECOM_EVENTS, Some("tool.failed, gateway.instance.*")),
            (ENV_WECOM_TEMPLATE, Some("DCC-MCP $event\nURL: $url")),
        ]);

        let runtime = EventWebhookRuntime::from_env(EventBus::new())?
            .expect("WeCom env should create a webhook runtime");
        let event_patterns = runtime
            .subscriptions
            .iter()
            .map(|(event, _)| event.as_str())
            .collect::<Vec<_>>();

        assert_eq!(event_patterns, vec!["tool.failed", "gateway.instance.*"]);
        Ok(())
    }

    #[tokio::test]
    async fn runtime_delivers_matching_events_to_http_endpoint() -> Result<()> {
        let (tx, mut rx) = mpsc::channel::<Value>(1);
        let app = Router::new()
            .route("/hook", post(capture_body))
            .with_state(tx);
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let server = tokio::spawn(async move {
            if let Err(err) = axum::serve(listener, app).await {
                tracing::error!(error = %err, "test webhook server failed");
            }
        });

        let config = WebhookConfigDocument {
            queue_capacity: 8,
            webhooks: vec![WebhookConfig {
                name: "test-webhook".to_string(),
                url: format!("http://{addr}/hook"),
                kind: None,
                events: vec!["tool.*".to_string()],
                headers: HashMap::new(),
                delivery: DeliveryConfig {
                    attempts: 1,
                    timeout_ms: 1_000,
                    backoff_ms: vec![1],
                },
                filters: vec![HashMap::from([(
                    "source.dcc_type".to_string(),
                    Value::String("maya".to_string()),
                )])],
                payload_template: None,
                message_template: None,
            }],
        };

        let bus = EventBus::new();
        let runtime = EventWebhookRuntime::from_document(bus.clone(), config)?;
        let emitted = bus.emit(
            "tool.completed",
            json!({"dcc_type": "maya"}),
            json!({"request_id": "req-1"}),
            json!({"tool_slug": "maya.scene__open"}),
        );

        let received = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .context("webhook request timed out")?
            .context("webhook receiver closed")?;
        assert_eq!(received["id"], emitted.id);
        assert_eq!(received["name"], "tool.completed");
        assert_eq!(received["attributes"]["tool_slug"], "maya.scene__open");

        drop(runtime);
        server.abort();
        Ok(())
    }

    #[tokio::test]
    async fn runtime_emits_delivery_failed_after_retries_exhaust() -> Result<()> {
        let app = Router::new().route("/hook", post(always_fail));
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let server = tokio::spawn(async move {
            if let Err(err) = axum::serve(listener, app).await {
                tracing::error!(error = %err, "test webhook server failed");
            }
        });

        let bus = EventBus::new();
        let (tx, mut rx) = mpsc::channel::<EventEnvelope>(1);
        let _failed_sub = bus.subscribe_event(DELIVERY_FAILED_EVENT.to_string(), move |event| {
            let _ = tx.try_send(event.clone());
        });
        let config = WebhookConfigDocument {
            queue_capacity: 8,
            webhooks: vec![WebhookConfig {
                name: "failing-webhook".to_string(),
                url: format!("http://{addr}/hook"),
                kind: None,
                events: vec!["tool.completed".to_string()],
                headers: HashMap::new(),
                delivery: DeliveryConfig {
                    attempts: 1,
                    timeout_ms: 1_000,
                    backoff_ms: vec![1],
                },
                filters: Vec::new(),
                payload_template: None,
                message_template: None,
            }],
        };

        let runtime = EventWebhookRuntime::from_document(bus.clone(), config)?;
        let emitted = bus.emit(
            "tool.completed",
            json!({"dcc_type": "maya"}),
            json!({"request_id": "req-1"}),
            json!({"tool_slug": "maya.scene__open"}),
        );

        let failed = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .context("delivery_failed event timed out")?
            .context("delivery_failed receiver closed")?;
        assert_eq!(failed.name, DELIVERY_FAILED_EVENT);
        assert_eq!(failed.correlation["event_id"], emitted.id);
        assert_eq!(failed.correlation["event_name"], "tool.completed");
        assert_eq!(failed.attributes["webhook"], "failing-webhook");
        assert_eq!(failed.attributes["attempts"], 1);

        drop(runtime);
        server.abort();
        Ok(())
    }

    async fn capture_body(State(tx): State<mpsc::Sender<Value>>, body: String) -> &'static str {
        let payload =
            serde_json::from_str::<Value>(&body).unwrap_or_else(|_| Value::Object(Map::new()));
        let _ = tx.send(payload).await;
        "ok"
    }

    async fn always_fail() -> (StatusCode, &'static str) {
        (StatusCode::INTERNAL_SERVER_ERROR, "fail")
    }
}
