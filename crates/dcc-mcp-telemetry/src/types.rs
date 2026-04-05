//! Core configuration and data types for dcc-mcp-telemetry.

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

// ── Exporter backend ─────────────────────────────────────────────────────────

/// Where to export telemetry data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ExporterBackend {
    /// Write traces/metrics to stdout (default, good for development).
    #[default]
    Stdout,
    /// OTLP gRPC exporter — sends to Jaeger, Grafana Tempo, etc.
    /// Requires the `otlp-exporter` feature.
    Otlp,
    /// No-op exporter — discard all telemetry (useful in tests).
    Noop,
}

// ── Log format ────────────────────────────────────────────────────────────────

/// Output format for structured logs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    /// Human-readable compact text format.
    #[default]
    Text,
    /// Machine-readable JSON format (recommended for production).
    Json,
}

// ── Telemetry config ──────────────────────────────────────────────────────────

/// Configuration for the telemetry provider.
///
/// Build via [`TelemetryConfig::builder`] and then call [`TelemetryConfig::init`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryConfig {
    /// Service name embedded in all spans and metrics (e.g. `"dcc-mcp-core"`).
    pub service_name: String,

    /// Service version string (e.g. `"0.12.2"`).
    pub service_version: String,

    /// Which exporter backend to use.
    pub exporter: ExporterBackend,

    /// OTLP endpoint URL (used when `exporter == Otlp`).
    /// Example: `"http://localhost:4317"`.
    pub otlp_endpoint: Option<String>,

    /// Log format.
    pub log_format: LogFormat,

    /// Maximum number of spans kept in the in-memory buffer before export.
    pub max_queue_size: usize,

    /// How long to wait before forcefully flushing a batch of spans.
    pub batch_timeout: Duration,

    /// Extra resource attributes applied to every span/metric.
    /// Key–value pairs, both strings.
    pub extra_attributes: HashMap<String, String>,

    /// Whether to enable metrics collection.
    pub enable_metrics: bool,

    /// Whether to enable distributed tracing.
    pub enable_tracing: bool,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        TelemetryConfig {
            service_name: "dcc-mcp-core".to_string(),
            service_version: env!("CARGO_PKG_VERSION").to_string(),
            exporter: ExporterBackend::default(),
            otlp_endpoint: None,
            log_format: LogFormat::default(),
            max_queue_size: 512,
            batch_timeout: Duration::from_secs(5),
            extra_attributes: HashMap::new(),
            enable_metrics: true,
            enable_tracing: true,
        }
    }
}

impl TelemetryConfig {
    /// Create a builder using the given service name.
    pub fn builder(service_name: impl Into<String>) -> TelemetryConfigBuilder {
        TelemetryConfigBuilder::new(service_name)
    }
}

// ── Builder ───────────────────────────────────────────────────────────────────

/// Fluent builder for [`TelemetryConfig`].
#[derive(Debug, Default)]
pub struct TelemetryConfigBuilder {
    inner: TelemetryConfig,
}

impl TelemetryConfigBuilder {
    /// Create a new builder with the given service name.
    pub fn new(service_name: impl Into<String>) -> Self {
        TelemetryConfigBuilder {
            inner: TelemetryConfig {
                service_name: service_name.into(),
                ..Default::default()
            },
        }
    }

    /// Set the service version.
    pub fn service_version(mut self, v: impl Into<String>) -> Self {
        self.inner.service_version = v.into();
        self
    }

    /// Use the stdout exporter (default — good for development).
    pub fn with_stdout_exporter(mut self) -> Self {
        self.inner.exporter = ExporterBackend::Stdout;
        self
    }

    /// Use the OTLP gRPC exporter (requires `otlp-exporter` feature).
    pub fn with_otlp_exporter(mut self, endpoint: impl Into<String>) -> Self {
        self.inner.exporter = ExporterBackend::Otlp;
        self.inner.otlp_endpoint = Some(endpoint.into());
        self
    }

    /// Use the no-op exporter (discard everything — useful in unit tests).
    pub fn with_noop_exporter(mut self) -> Self {
        self.inner.exporter = ExporterBackend::Noop;
        self
    }

    /// Set the log format.
    pub fn log_format(mut self, fmt: LogFormat) -> Self {
        self.inner.log_format = fmt;
        self
    }

    /// Set the maximum span queue size before exporting.
    pub fn max_queue_size(mut self, size: usize) -> Self {
        self.inner.max_queue_size = size;
        self
    }

    /// Set the batch flush timeout.
    pub fn batch_timeout(mut self, timeout: Duration) -> Self {
        self.inner.batch_timeout = timeout;
        self
    }

    /// Add an extra resource attribute.
    pub fn with_attribute(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.inner.extra_attributes.insert(key.into(), value.into());
        self
    }

    /// Enable or disable metrics.
    pub fn enable_metrics(mut self, enabled: bool) -> Self {
        self.inner.enable_metrics = enabled;
        self
    }

    /// Enable or disable distributed tracing.
    pub fn enable_tracing(mut self, enabled: bool) -> Self {
        self.inner.enable_tracing = enabled;
        self
    }

    /// Build the final [`TelemetryConfig`].
    pub fn build(self) -> TelemetryConfig {
        self.inner
    }
}

// ── Span attributes ───────────────────────────────────────────────────────────

/// Well-known span attribute keys used across the DCC-MCP ecosystem.
pub mod span_keys {
    /// The DCC application name (e.g. `"maya"`, `"blender"`).
    pub const DCC_NAME: &str = "dcc.name";
    /// The DCC application version (e.g. `"2025"`).
    pub const DCC_VERSION: &str = "dcc.version";
    /// The Action name being executed (e.g. `"create_sphere"`).
    pub const ACTION_NAME: &str = "action.name";
    /// The Skill name.
    pub const SKILL_NAME: &str = "skill.name";
    /// The transport protocol (e.g. `"named_pipe"`, `"tcp"`, `"http"`).
    pub const TRANSPORT_PROTOCOL: &str = "transport.protocol";
    /// Whether the operation succeeded.
    pub const OPERATION_SUCCESS: &str = "operation.success";
    /// Error category when `operation.success == false`.
    pub const ERROR_KIND: &str = "error.kind";
}

// ── Action metrics ────────────────────────────────────────────────────────────

/// A snapshot of per-Action performance metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionMetrics {
    /// Action name.
    pub action_name: String,
    /// Total number of invocations.
    pub invocation_count: u64,
    /// Number of successful invocations.
    pub success_count: u64,
    /// Number of failed invocations.
    pub failure_count: u64,
    /// Average execution duration in milliseconds.
    pub avg_duration_ms: f64,
    /// P95 execution duration in milliseconds.
    pub p95_duration_ms: f64,
    /// P99 execution duration in milliseconds.
    pub p99_duration_ms: f64,
}

impl ActionMetrics {
    /// Create an empty metrics snapshot for the given action.
    pub fn new(action_name: impl Into<String>) -> Self {
        ActionMetrics {
            action_name: action_name.into(),
            invocation_count: 0,
            success_count: 0,
            failure_count: 0,
            avg_duration_ms: 0.0,
            p95_duration_ms: 0.0,
            p99_duration_ms: 0.0,
        }
    }

    /// Success rate as a fraction in [0.0, 1.0].
    pub fn success_rate(&self) -> f64 {
        if self.invocation_count == 0 {
            0.0
        } else {
            self.success_count as f64 / self.invocation_count as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod test_exporter_backend {
        use super::*;

        #[test]
        fn default_is_stdout() {
            assert_eq!(ExporterBackend::default(), ExporterBackend::Stdout);
        }

        #[test]
        fn serialization_round_trip() {
            let v = ExporterBackend::Otlp;
            let s = serde_json::to_string(&v).unwrap();
            let back: ExporterBackend = serde_json::from_str(&s).unwrap();
            assert_eq!(back, ExporterBackend::Otlp);
        }
    }

    mod test_log_format {
        use super::*;

        #[test]
        fn default_is_text() {
            assert_eq!(LogFormat::default(), LogFormat::Text);
        }
    }

    mod test_config_builder {
        use super::*;

        #[test]
        fn builder_defaults() {
            let cfg = TelemetryConfig::builder("my-service").build();
            assert_eq!(cfg.service_name, "my-service");
            assert_eq!(cfg.exporter, ExporterBackend::Stdout);
            assert!(cfg.enable_metrics);
            assert!(cfg.enable_tracing);
        }

        #[test]
        fn builder_with_otlp() {
            let cfg = TelemetryConfig::builder("svc")
                .with_otlp_exporter("http://localhost:4317")
                .build();
            assert_eq!(cfg.exporter, ExporterBackend::Otlp);
            assert_eq!(cfg.otlp_endpoint.as_deref(), Some("http://localhost:4317"));
        }

        #[test]
        fn builder_with_noop() {
            let cfg = TelemetryConfig::builder("test")
                .with_noop_exporter()
                .build();
            assert_eq!(cfg.exporter, ExporterBackend::Noop);
        }

        #[test]
        fn builder_with_attributes() {
            let cfg = TelemetryConfig::builder("svc")
                .with_attribute("dcc.name", "maya")
                .with_attribute("env", "production")
                .build();
            assert_eq!(
                cfg.extra_attributes.get("dcc.name").map(String::as_str),
                Some("maya")
            );
            assert_eq!(
                cfg.extra_attributes.get("env").map(String::as_str),
                Some("production")
            );
        }

        #[test]
        fn builder_disable_metrics() {
            let cfg = TelemetryConfig::builder("svc")
                .enable_metrics(false)
                .build();
            assert!(!cfg.enable_metrics);
        }

        #[test]
        fn builder_max_queue_size() {
            let cfg = TelemetryConfig::builder("svc").max_queue_size(1024).build();
            assert_eq!(cfg.max_queue_size, 1024);
        }

        #[test]
        fn builder_service_version() {
            let cfg = TelemetryConfig::builder("svc")
                .service_version("1.2.3")
                .build();
            assert_eq!(cfg.service_version, "1.2.3");
        }
    }

    mod test_action_metrics {
        use super::*;

        #[test]
        fn new_has_zero_counts() {
            let m = ActionMetrics::new("create_sphere");
            assert_eq!(m.invocation_count, 0);
            assert_eq!(m.success_count, 0);
            assert_eq!(m.failure_count, 0);
        }

        #[test]
        fn success_rate_zero_when_no_invocations() {
            let m = ActionMetrics::new("x");
            assert_eq!(m.success_rate(), 0.0);
        }

        #[test]
        fn success_rate_full_when_all_succeed() {
            let m = ActionMetrics {
                action_name: "x".into(),
                invocation_count: 10,
                success_count: 10,
                failure_count: 0,
                avg_duration_ms: 1.0,
                p95_duration_ms: 2.0,
                p99_duration_ms: 3.0,
            };
            assert!((m.success_rate() - 1.0).abs() < f64::EPSILON);
        }

        #[test]
        fn success_rate_partial() {
            let m = ActionMetrics {
                action_name: "x".into(),
                invocation_count: 4,
                success_count: 3,
                failure_count: 1,
                avg_duration_ms: 1.0,
                p95_duration_ms: 2.0,
                p99_duration_ms: 3.0,
            };
            assert!((m.success_rate() - 0.75).abs() < f64::EPSILON);
        }
    }

    mod test_span_keys {
        use super::*;

        #[test]
        fn keys_are_non_empty() {
            assert!(!span_keys::DCC_NAME.is_empty());
            assert!(!span_keys::ACTION_NAME.is_empty());
            assert!(!span_keys::TRANSPORT_PROTOCOL.is_empty());
        }
    }
}
