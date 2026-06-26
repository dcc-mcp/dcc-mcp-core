use super::*;
use crate::gateway::admin::wecom_url::strict_looks_valid as strict_wecom_webhook_url_looks_valid;

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
fn wecom_webhook_url_validation_requires_robot_endpoint() {
    assert!(strict_wecom_webhook_url_looks_valid(
        "https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=abc123"
    ));
    assert!(strict_wecom_webhook_url_looks_valid(
        "https://qyapi.weixin.qq.com:443/cgi-bin/webhook/send?key=abc123"
    ));
    assert!(!strict_wecom_webhook_url_looks_valid(
        "http://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=abc123"
    ));
    assert!(!strict_wecom_webhook_url_looks_valid(
        "https://example.com/cgi-bin/webhook/send?key=abc123"
    ));
    assert!(!strict_wecom_webhook_url_looks_valid(
        "https://qyapi.weixin.qq.com/cgi-bin/webhook/send"
    ));
    assert!(!strict_wecom_webhook_url_looks_valid(
        "https://qyapi.weixin.qq.com/other?key=abc123"
    ));
}

#[test]
fn wecom_response_summary_does_not_echo_extra_fields() {
    let (_errcode, errmsg, summary) = summarize_wecom_response(
        r#"{"errcode":93000,"errmsg":"invalid webhook","secret_echo":"leak"}"#,
        StatusCode::OK,
    );

    assert_eq!(errmsg, "invalid webhook");
    assert_eq!(summary["errcode"], 93000);
    assert_eq!(summary["errmsg"], "invalid webhook");
    assert!(summary.get("secret_echo").is_none());
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
