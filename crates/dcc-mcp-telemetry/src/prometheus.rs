//! Prometheus text-exposition exporter (issue #331).
//!
//! This module is compiled only when the `prometheus` Cargo feature is
//! enabled — when disabled, **zero** Prometheus code is pulled into the
//! wheel. The exporter sits on top of the existing in-memory state
//! tracked by [`crate::recorder::ActionRecorder`] / [`ToolMetrics`] and
//! a small set of additional counters / gauges that callers (the HTTP
//! server, the `JobManager`, the notification pipe) push into at the
//! points where they already emit tracing events.
//!
//! # Design
//!
//! We keep a local [`prometheus::Registry`] rather than using the global
//! `prometheus::default_registry()` so that multiple servers in the same
//! process (e.g. gateway + instance) do not clobber each other's labels.
//!
//! The HTTP crate wires the exporter to a `/metrics` endpoint on the
//! same Axum router; see `crates/dcc-mcp-http/src/server.rs` for that
//! wiring. The optional `basic_auth` guard is applied at the handler
//! layer, not here — this module only emits the wire format.
//!
//! # Metrics surface
//!
//! | Name | Type | Labels |
//! |------|------|--------|
//! | `dcc_mcp_tool_calls_total`          | counter   | `tool`, `status` |
//! | `dcc_mcp_tool_duration_seconds`     | histogram | `tool` |
//! | `dcc_mcp_jobs_in_flight`            | gauge     | `tool` |
//! | `dcc_mcp_job_created_total`         | counter   | `tool`, `result` |
//! | `dcc_mcp_job_wait_seconds`          | histogram | `tool` |
//! | `dcc_mcp_notifications_sent_total`  | counter   | `channel` |
//! | `dcc_mcp_active_sessions`           | gauge     | — |
//! | `dcc_mcp_registered_tools`          | gauge     | — |
//! | `dcc_mcp_build_info`                | gauge     | `version`, `crate` (always 1) |

use std::sync::Arc;

use parking_lot::Mutex;
use prometheus::{
    Encoder, Gauge, GaugeVec, HistogramOpts, HistogramVec, IntCounterVec, IntGauge, IntGaugeVec,
    Opts, Registry, TextEncoder,
};

use crate::recorder::ActionRecorder;

/// The content-type every Prometheus-compatible scraper expects.
pub const PROMETHEUS_CONTENT_TYPE: &str = "text/plain; version=0.0.4; charset=utf-8";

/// The default histogram buckets we publish for tool execution duration
/// (seconds). Covers 1 ms to 30 s on a roughly-logarithmic ladder, which
/// is appropriate for the mixture of DCC tool calls we see in practice
/// (short scene inspections to multi-second scene mutations).
const DURATION_BUCKETS_SECONDS: &[f64] = &[
    0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0,
];

/// Prometheus exporter for the DCC-MCP stack.
///
/// Construct once per server instance, clone freely (internally
/// reference-counted), and call [`render`](Self::render) at scrape time.
/// The exporter is safe to share across threads.
#[derive(Clone)]
pub struct PrometheusExporter {
    inner: Arc<Inner>,
}

struct Inner {
    registry: Registry,

    tool_calls_total: IntCounterVec,
    tool_duration_seconds: HistogramVec,

    jobs_in_flight: IntGaugeVec,
    job_created_total: IntCounterVec,
    job_wait_seconds: HistogramVec,

    notifications_sent_total: IntCounterVec,

    active_sessions: IntGauge,
    registered_tools: IntGauge,

    instances_total: IntGaugeVec,
    tools_total: IntGaugeVec,
    request_duration_seconds: HistogramVec,
    requests_failed_total: IntCounterVec,

    #[allow(dead_code)]
    build_info: GaugeVec,

    /// Optional bridge into the existing ActionRecorder. When set, a
    /// scrape will refresh Prometheus counters from ActionRecorder
    /// aggregate state for tools that the exporter has not yet seen
    /// directly (e.g. tools that recorded calls before the exporter was
    /// attached). Not a hard dependency — the exporter works fine with
    /// it unset, and `ActionRecorder` works fine without the exporter.
    recorder: Mutex<Option<ActionRecorder>>,
}

impl PrometheusExporter {
    /// Build a new exporter with its own private registry. Emits a
    /// `dcc_mcp_build_info{version, crate}` gauge so scrapers can track
    /// which build is serving them.
    pub fn new() -> Self {
        let registry = Registry::new();

        let tool_calls_total = IntCounterVec::new(
            Opts::new(
                "dcc_mcp_tool_calls_total",
                "Total number of tool/action invocations observed by the server.",
            ),
            &["tool", "status"],
        )
        .expect("static metric definition");
        registry
            .register(Box::new(tool_calls_total.clone()))
            .expect("unique registration");

        let tool_duration_seconds = HistogramVec::new(
            HistogramOpts::new(
                "dcc_mcp_tool_duration_seconds",
                "Tool/action execution duration in seconds.",
            )
            .buckets(DURATION_BUCKETS_SECONDS.to_vec()),
            &["tool"],
        )
        .expect("static metric definition");
        registry
            .register(Box::new(tool_duration_seconds.clone()))
            .expect("unique registration");

        let jobs_in_flight = IntGaugeVec::new(
            Opts::new(
                "dcc_mcp_jobs_in_flight",
                "Number of asynchronous jobs currently running, keyed by tool.",
            ),
            &["tool"],
        )
        .expect("static metric definition");
        registry
            .register(Box::new(jobs_in_flight.clone()))
            .expect("unique registration");

        let job_created_total = IntCounterVec::new(
            Opts::new(
                "dcc_mcp_job_created_total",
                "Total number of asynchronous jobs created, keyed by tool and result.",
            ),
            &["tool", "result"],
        )
        .expect("static metric definition");
        registry
            .register(Box::new(job_created_total.clone()))
            .expect("unique registration");

        let job_wait_seconds = HistogramVec::new(
            HistogramOpts::new(
                "dcc_mcp_job_wait_seconds",
                "Wait time (seconds) between job creation and first execution.",
            )
            .buckets(DURATION_BUCKETS_SECONDS.to_vec()),
            &["tool"],
        )
        .expect("static metric definition");
        registry
            .register(Box::new(job_wait_seconds.clone()))
            .expect("unique registration");

        // TODO(#326): wire this up to JobNotifier once the SSE
        // notification pipe lands. For now the counter stays at 0,
        // which is intentional — scrapers see the label set and know
        // the metric exists even before notifications are flowing.
        let notifications_sent_total = IntCounterVec::new(
            Opts::new(
                "dcc_mcp_notifications_sent_total",
                "Total number of MCP notifications pushed to clients, keyed by channel.",
            ),
            &["channel"],
        )
        .expect("static metric definition");
        registry
            .register(Box::new(notifications_sent_total.clone()))
            .expect("unique registration");

        let active_sessions = IntGauge::with_opts(Opts::new(
            "dcc_mcp_active_sessions",
            "Number of active MCP sessions (Streamable HTTP).",
        ))
        .expect("static metric definition");
        registry
            .register(Box::new(active_sessions.clone()))
            .expect("unique registration");

        let registered_tools = IntGauge::with_opts(Opts::new(
            "dcc_mcp_registered_tools",
            "Number of tools currently registered in the ActionRegistry.",
        ))
        .expect("static metric definition");
        registry
            .register(Box::new(registered_tools.clone()))
            .expect("unique registration");

        let build_info = GaugeVec::new(
            Opts::new(
                "dcc_mcp_build_info",
                "Always 1; labels carry build information about the running binary.",
            ),
            &["version", "crate"],
        )
        .expect("static metric definition");
        registry
            .register(Box::new(build_info.clone()))
            .expect("unique registration");
        // Publish a single series so scrapers always see the build info.
        build_info
            .with_label_values(&[env!("CARGO_PKG_VERSION"), env!("CARGO_PKG_NAME")])
            .set(1.0);

        let instances_total = IntGaugeVec::new(
            Opts::new(
                "dcc_mcp_instances_total",
                "Number of registered DCC instances by status.",
            ),
            &["status"],
        )
        .expect("static metric definition");
        registry
            .register(Box::new(instances_total.clone()))
            .expect("unique registration");

        let tools_total = IntGaugeVec::new(
            Opts::new(
                "dcc_mcp_tools_total",
                "Number of tools exposed by DCC type.",
            ),
            &["dcc_type"],
        )
        .expect("static metric definition");
        registry
            .register(Box::new(tools_total.clone()))
            .expect("unique registration");

        let request_duration_seconds = HistogramVec::new(
            HistogramOpts::new(
                "dcc_mcp_request_duration_seconds",
                "Gateway request duration in seconds.",
            )
            .buckets(DURATION_BUCKETS_SECONDS.to_vec()),
            &["method"],
        )
        .expect("static metric definition");
        registry
            .register(Box::new(request_duration_seconds.clone()))
            .expect("unique registration");

        let requests_failed_total = IntCounterVec::new(
            Opts::new(
                "dcc_mcp_requests_failed_total",
                "Total number of failed gateway requests by method.",
            ),
            &["method"],
        )
        .expect("static metric definition");
        registry
            .register(Box::new(requests_failed_total.clone()))
            .expect("unique registration");

        Self {
            inner: Arc::new(Inner {
                registry,
                tool_calls_total,
                tool_duration_seconds,
                jobs_in_flight,
                job_created_total,
                job_wait_seconds,
                notifications_sent_total,
                active_sessions,
                registered_tools,
                instances_total,
                tools_total,
                request_duration_seconds,
                requests_failed_total,
                build_info,
                recorder: Mutex::new(None),
            }),
        }
    }

    /// Attach an [`ActionRecorder`] so scrapes can reconcile any counts
    /// that were recorded on the recorder before the exporter was
    /// attached. Optional — call sites that record directly via
    /// [`record_tool_call`](Self::record_tool_call) do not need this.
    pub fn with_recorder(self, recorder: ActionRecorder) -> Self {
        *self.inner.recorder.lock() = Some(recorder);
        self
    }

    /// Record a completed tool call.
    ///
    /// * `tool`     — fully-qualified tool name (matches what the MCP
    ///                client called).
    /// * `status`   — `"success"` or `"error"`. Any other value is
    ///                passed through unchanged to Prometheus.
    /// * `duration` — wall-clock duration from dispatch to completion.
    pub fn record_tool_call(&self, tool: &str, status: &str, duration: std::time::Duration) {
        self.inner
            .tool_calls_total
            .with_label_values(&[tool, status])
            .inc();
        self.inner
            .tool_duration_seconds
            .with_label_values(&[tool])
            .observe(duration.as_secs_f64());
    }

    /// Record a newly-created job. `result` is a short machine-readable
    /// string such as `"accepted"`, `"queue_full"`, `"rejected"`.
    pub fn record_job_created(&self, tool: &str, result: &str) {
        self.inner
            .job_created_total
            .with_label_values(&[tool, result])
            .inc();
    }

    /// Observe how long a job waited between creation and first
    /// execution. Typically called from the dispatcher when a job
    /// transitions from Pending → Running.
    pub fn observe_job_wait(&self, tool: &str, wait: std::time::Duration) {
        self.inner
            .job_wait_seconds
            .with_label_values(&[tool])
            .observe(wait.as_secs_f64());
    }

    /// Increment the in-flight job gauge for a tool.
    pub fn inc_jobs_in_flight(&self, tool: &str) {
        self.inner.jobs_in_flight.with_label_values(&[tool]).inc();
    }

    /// Decrement the in-flight job gauge for a tool.
    pub fn dec_jobs_in_flight(&self, tool: &str) {
        self.inner.jobs_in_flight.with_label_values(&[tool]).dec();
    }

    /// Record a notification pushed to a client channel.
    ///
    /// `channel` is typically `"sse"` or `"ws"`. This is the counter
    /// referenced in issue #326 — if the notifier is not yet wired,
    /// callers will simply not invoke it and the counter stays at 0.
    pub fn record_notification_sent(&self, channel: &str) {
        self.inner
            .notifications_sent_total
            .with_label_values(&[channel])
            .inc();
    }

    /// Set the active session gauge to an absolute value.
    pub fn set_active_sessions(&self, n: i64) {
        self.inner.active_sessions.set(n);
    }

    /// Set the registered-tool gauge to an absolute value.
    pub fn set_registered_tools(&self, n: i64) {
        self.inner.registered_tools.set(n);
    }

    /// Set the instance count gauge for a given status label.
    pub fn set_instances_total(&self, status: &str, n: i64) {
        self.inner
            .instances_total
            .with_label_values(&[status])
            .set(n);
    }

    /// Set the tool count gauge for a given DCC type label.
    pub fn set_tools_total(&self, dcc_type: &str, n: i64) {
        self.inner.tools_total.with_label_values(&[dcc_type]).set(n);
    }

    /// Observe a gateway request duration.
    pub fn observe_request_duration(&self, method: &str, duration: std::time::Duration) {
        self.inner
            .request_duration_seconds
            .with_label_values(&[method])
            .observe(duration.as_secs_f64());
    }

    /// Increment the failed request counter for a method.
    pub fn inc_requests_failed(&self, method: &str) {
        self.inner
            .requests_failed_total
            .with_label_values(&[method])
            .inc();
    }

    /// Render the current metric state as a Prometheus text-exposition
    /// payload. This is what `/metrics` hands back to scrapers.
    ///
    /// Always succeeds — the error paths from the encoder are
    /// unreachable in practice (see `prometheus` crate source), but we
    /// still surface them via `io::Result` for symmetry with the
    /// encoder's API.
    pub fn render(&self) -> std::io::Result<String> {
        self.maybe_reconcile_from_recorder();
        let metric_families = self.inner.registry.gather();
        let mut buf = Vec::with_capacity(4 * 1024);
        let encoder = TextEncoder::new();
        encoder
            .encode(&metric_families, &mut buf)
            .map_err(std::io::Error::other)?;
        String::from_utf8(buf).map_err(std::io::Error::other)
    }

    /// Access the underlying registry — primarily for tests and for
    /// callers that want to register additional custom metrics.
    pub fn registry(&self) -> &Registry {
        &self.inner.registry
    }

    fn maybe_reconcile_from_recorder(&self) {
        // If no recorder has been attached, nothing to reconcile. The
        // exporter is driven solely by `record_tool_call` invocations
        // in that case.
        let guard = self.inner.recorder.lock();
        let Some(recorder) = guard.as_ref() else {
            return;
        };
        // Reconcile the tool_calls counter only for *newly seen* tools.
        // We cannot retroactively increment a Prometheus counter without
        // breaking monotonicity, so we publish a gauge-like snapshot by
        // computing delta versus the counter's current value. In
        // practice: when the exporter is attached before any tool calls
        // flow (the expected path) this is a no-op. This exists as a
        // safety net for the "I forgot to wire record_tool_call at one
        // of the dispatch sites" case, so metrics still show up.
        for metrics in recorder.all_metrics() {
            let tool = metrics.action_name.as_str();
            let current_success = self
                .inner
                .tool_calls_total
                .with_label_values(&[tool, "success"])
                .get();
            let current_failure = self
                .inner
                .tool_calls_total
                .with_label_values(&[tool, "error"])
                .get();
            if metrics.success_count > current_success {
                self.inner
                    .tool_calls_total
                    .with_label_values(&[tool, "success"])
                    .inc_by(metrics.success_count - current_success);
            }
            if metrics.failure_count > current_failure {
                self.inner
                    .tool_calls_total
                    .with_label_values(&[tool, "error"])
                    .inc_by(metrics.failure_count - current_failure);
            }
        }
    }
}

impl Default for PrometheusExporter {
    fn default() -> Self {
        Self::new()
    }
}

/// Suppresses the unused-import warning for [`Gauge`] when building with
/// the `prometheus` feature but no direct gauge construction. Kept as a
/// marker for future expansion (e.g. per-DCC gauges).
#[allow(dead_code)]
type _GaugeMarker = Gauge;

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Prime every metric vector with a single observation so the
    /// encoder emits its HELP/TYPE headers. The Prometheus Rust client
    /// suppresses headers for label vectors that have never been
    /// observed (the `_sum`/`_count` of an empty histogram is also
    /// suppressed) — in production this is fine because `tools/list`
    /// and the first `tools/call` always warm the vectors before the
    /// first scrape, but tests need an explicit seed.
    fn seed_all(exp: &PrometheusExporter) {
        exp.record_tool_call("seed", "success", Duration::from_millis(1));
        exp.inc_jobs_in_flight("seed");
        exp.dec_jobs_in_flight("seed");
        exp.record_job_created("seed", "accepted");
        exp.observe_job_wait("seed", Duration::from_millis(1));
        exp.record_notification_sent("seed");
    }

    #[test]
    fn render_contains_all_metric_names() {
        let exp = PrometheusExporter::new();
        seed_all(&exp);
        let out = exp.render().unwrap();

        for name in [
            "dcc_mcp_tool_calls_total",
            "dcc_mcp_tool_duration_seconds",
            "dcc_mcp_jobs_in_flight",
            "dcc_mcp_job_created_total",
            "dcc_mcp_job_wait_seconds",
            "dcc_mcp_notifications_sent_total",
            "dcc_mcp_active_sessions",
            "dcc_mcp_registered_tools",
            "dcc_mcp_build_info",
        ] {
            assert!(
                out.contains(name),
                "rendered output missing metric `{name}`:\n{out}"
            );
        }
    }

    #[test]
    fn render_contains_help_and_type_headers() {
        let exp = PrometheusExporter::new();
        seed_all(&exp);
        let out = exp.render().unwrap();
        // Every metric must publish a HELP + TYPE line for promtool
        // `check metrics` to accept the payload.
        assert!(out.contains("# HELP dcc_mcp_tool_calls_total"));
        assert!(out.contains("# TYPE dcc_mcp_tool_calls_total counter"));
        assert!(out.contains("# TYPE dcc_mcp_tool_duration_seconds histogram"));
        assert!(out.contains("# TYPE dcc_mcp_active_sessions gauge"));
    }

    #[test]
    fn record_tool_call_increments_counter() {
        let exp = PrometheusExporter::new();
        exp.record_tool_call("create_sphere", "success", Duration::from_millis(17));
        exp.record_tool_call("create_sphere", "success", Duration::from_millis(23));
        exp.record_tool_call("create_sphere", "error", Duration::from_millis(5));

        let out = exp.render().unwrap();
        assert!(
            out.contains(r#"dcc_mcp_tool_calls_total{status="success",tool="create_sphere"} 2"#)
        );
        assert!(out.contains(r#"dcc_mcp_tool_calls_total{status="error",tool="create_sphere"} 1"#));
        // Histogram must publish at least one bucket and a _count line.
        assert!(out.contains("dcc_mcp_tool_duration_seconds_bucket"));
        assert!(out.contains("dcc_mcp_tool_duration_seconds_count{"));
    }

    #[test]
    fn jobs_in_flight_increments_and_decrements() {
        let exp = PrometheusExporter::new();
        exp.inc_jobs_in_flight("render");
        exp.inc_jobs_in_flight("render");
        exp.dec_jobs_in_flight("render");

        let out = exp.render().unwrap();
        assert!(out.contains(r#"dcc_mcp_jobs_in_flight{tool="render"} 1"#));
    }

    #[test]
    fn gauges_are_absolute() {
        let exp = PrometheusExporter::new();
        exp.set_active_sessions(7);
        exp.set_registered_tools(42);
        exp.set_active_sessions(3);

        let out = exp.render().unwrap();
        assert!(out.contains("dcc_mcp_active_sessions 3"));
        assert!(out.contains("dcc_mcp_registered_tools 42"));
    }

    #[test]
    fn notifications_and_job_counters() {
        let exp = PrometheusExporter::new();
        exp.record_notification_sent("sse");
        exp.record_job_created("bake_simulation", "accepted");
        exp.observe_job_wait("bake_simulation", Duration::from_millis(120));

        let out = exp.render().unwrap();
        assert!(out.contains(r#"dcc_mcp_notifications_sent_total{channel="sse"} 1"#));
        assert!(
            out.contains(
                r#"dcc_mcp_job_created_total{result="accepted",tool="bake_simulation"} 1"#
            )
        );
        assert!(out.contains("dcc_mcp_job_wait_seconds_bucket"));
    }

    #[test]
    fn build_info_is_always_one() {
        let exp = PrometheusExporter::new();
        let out = exp.render().unwrap();
        // Series value is 1 — scrapers use the labels to track versions.
        assert!(out.contains("dcc_mcp_build_info{"));
        assert!(out.contains("} 1"));
    }

    #[test]
    fn reconcile_from_recorder_back_fills_counter() {
        let recorder = ActionRecorder::new("test-scope");
        recorder.start("my_tool", "maya").finish(true);
        recorder.start("my_tool", "maya").finish(true);
        recorder.start("my_tool", "maya").finish(false);

        let exp = PrometheusExporter::new().with_recorder(recorder);
        let out = exp.render().unwrap();

        assert!(out.contains(r#"dcc_mcp_tool_calls_total{status="success",tool="my_tool"} 2"#));
        assert!(out.contains(r#"dcc_mcp_tool_calls_total{status="error",tool="my_tool"} 1"#));
    }
}
