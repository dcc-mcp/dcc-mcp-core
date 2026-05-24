// ── Drift-detection tests ─────────────────────────────────────────────────────
//
// Every field in `McpHttpConfig` that Python callers should be able to read
// must have a matching getter on `PyMcpHttpConfig`. All are now hand-written
// in the `#[pymethods]` block above.
//
// When you add a new field:
//   1. Add a hand-written getter (and setter if needed) in the `#[pymethods]` block.
//   2. Add `let _ = cfg.<getter>();` to the test below.
//
// The test fails to **compile** if a getter is removed — that is the intended
// safety net against silent drift between the Rust config and the Python API.
use super::*;
use dcc_mcp_http_types::config::McpHttpConfig;

fn default_cfg() -> PyMcpHttpConfig {
    PyMcpHttpConfig {
        inner: McpHttpConfig::default(),
        sandbox_policy: None,
    }
}

#[test]
fn all_mcp_http_config_fields_have_py_getters() {
    let cfg = default_cfg();

    // ── ServerConfig (read-only) ────────────────────────────────
    let _ = cfg.port();
    let _ = cfg.host();
    let _ = cfg.endpoint_path();
    let _ = cfg.server_name();
    let _ = cfg.server_version();
    let _ = cfg.max_sessions();
    let _ = cfg.request_timeout_ms();
    let _ = cfg.enable_cors();

    // ── ServerConfig (read-write, hand-written) ──────────────────
    let _ = cfg.spawn_mode();
    let _ = cfg.self_probe_timeout_ms();

    // ── SessionConfig ────────────────────────────────────────────
    let _ = cfg.session_ttl_secs();
    let _ = cfg.enable_tool_cache();

    // ── TelemetryConfig ──────────────────────────────────────────
    let _ = cfg.enable_prometheus();
    let _ = cfg.prometheus_basic_auth();

    // ── FeatureFlags ────────────────────────────────────────────
    let _ = cfg.lazy_actions();
    let _ = cfg.enable_workflows();
    let _ = cfg.shutdown_on_drop();
    let _ = cfg.enable_job_notifications();
    let _ = cfg.bare_tool_names();
    let _ = cfg.exclude_skill_stubs_from_tools_list();
    let _ = cfg.exclude_group_stubs_from_tools_list();
    let _ = cfg.enable_resources();
    let _ = cfg.enable_artefact_resources();
    let _ = cfg.enable_prompts();

    // ── GatewayConfig ────────────────────────────────────────────
    let _ = cfg.gateway_port();
    let _ = cfg.gateway_remote_host();
    let _ = cfg.gateway_remote_port();
    let _ = cfg.registry_dir();
    let _ = cfg.stale_timeout_secs();
    let _ = cfg.heartbeat_secs();
    let _ = cfg.backend_timeout_ms();
    let _ = cfg.gateway_async_dispatch_timeout_ms();
    let _ = cfg.gateway_wait_terminal_timeout_ms();
    let _ = cfg.gateway_route_ttl_secs();
    let _ = cfg.gateway_max_routes_per_session();
    let _ = cfg.adapter_version();
    let _ = cfg.adapter_dcc();
    let _ = cfg.allow_unknown_tools();
    let _ = cfg.gateway_read_only();
    let _ = cfg.allowed_dcc_types();
    let _ = cfg.allowed_skill_names();
    let _ = cfg.allowed_skill_families();
    let _ = cfg.allowed_tool_slugs();
    let _ = cfg.allowed_tool_slug_prefixes();

    // ── QueueConfig ─────────────────────────────────────────────
    let _ = cfg.deferred_queue_depth();
    let _ = cfg.bridge_queue_depth();
    let _ = cfg.host_queue_depth();
    let _ = cfg.queue_send_timeout_ms();

    // ── InstanceConfig ──────────────────────────────────────────
    let _ = cfg.dcc_type();
    let _ = cfg.dcc_version();
    let _ = cfg.scene();
    let _ = cfg.instance_metadata();
    let _ = cfg.declared_capabilities();

    // ── WorkflowConfig ──────────────────────────────────────────
    let _ = cfg.enable_scheduler();
    let _ = cfg.schedules_dir();

    // ── JobConfig ───────────────────────────────────────────────
    let _ = cfg.job_storage_path();
    let _ = cfg.job_recovery();

    // ── In-process sandbox (issue #1001) ──────────────────────────
    let _ = pyo3::Python::try_attach(|py| cfg.sandbox_policy(py));
}

#[test]
fn repr_contains_port() {
    let cfg = PyMcpHttpConfig {
        inner: McpHttpConfig::default().with_port(1234),
        sandbox_policy: None,
    };
    assert!(cfg.__repr__().contains("1234"));
}

#[test]
fn repr_contains_class_name() {
    let cfg = default_cfg();
    assert!(cfg.__repr__().contains("McpHttpConfig"));
}
