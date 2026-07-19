//! Prometheus text-exposition exporter (issue #331).
//!
//! This module is compiled only when the `prometheus` Cargo feature is
//! enabled — when disabled, **zero** Prometheus code is pulled into the
//! wheel. The exporter sits on top of the existing in-memory state
//! tracked by [`crate::recorder::ToolRecorder`] / [`ToolMetrics`] and
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
//! | `dcc_mcp_gateway_backend_errors_total` | counter | `kind` (gateway -> backend hop) |
//! | `dcc_mcp_gateway_searches_total` | counter | `result` (`zero`, `nonzero`) |
//! | `dcc_mcp_gateway_search_followups_total` | counter | `kind`, `rank_bucket` |
//! | `dcc_mcp_gateway_search_reformulations_total` | counter | - |
//! | `dcc_mcp_gateway_search_time_to_first_success_seconds` | histogram | - |
//! | `dcc_mcp_gateway_governance_events_total` | counter | `category`, `outcome` |
//! | `dcc_mcp_build_info`                | gauge     | `version`, `crate` (always 1) |

use std::sync::Arc;

use parking_lot::Mutex;
use prometheus::{
    Encoder, Gauge, GaugeVec, Histogram, HistogramOpts, HistogramVec, IntCounter, IntCounterVec,
    IntGauge, IntGaugeVec, Opts, Registry, TextEncoder,
};

use crate::recorder::ToolRecorder;

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

    /// Gateway-only: backend hop failures by coarse error class (`transport`,
    /// `http_5xx`, `jsonrpc_backend`, …). See `PrometheusExporter::record_gateway_backend_error`.
    gateway_backend_errors_total: IntCounterVec,
    gateway_searches_total: IntCounterVec,
    gateway_search_followups_total: IntCounterVec,
    gateway_search_reformulations_total: IntCounter,
    gateway_search_time_to_first_success_seconds: Histogram,
    gateway_governance_events_total: IntCounterVec,

    // ── PIP-2751: Thread/queue metrics ────────────────────────────────────
    /// Queue wait time (seconds) from enqueue to dispatch start.
    queue_wait_seconds: HistogramVec,
    /// Host (DCC) execution time (seconds) excluding queue wait.
    host_execution_seconds: HistogramVec,
    /// Current queue depth.
    queue_depth: IntGauge,
    /// Currently in-flight requests.
    in_flight_requests: IntGauge,

    // ── PIP-2751: Coverage metrics ────────────────────────────────────────
    /// Total observed requests (gateway-proxied).
    observed_requests_total: IntCounter,
    /// Total unobserved requests (CLI direct, not proxied).
    unobserved_requests_total: IntCounter,

    // ── PIP-2751: Stability metrics ───────────────────────────────────────
    /// Crashes by type (host, gpu, other).
    crashes_total: IntCounterVec,
    /// Reconnects total.
    reconnects_total: IntCounter,
    /// Recoveries total (successful reconnects).
    recoveries_total: IntCounter,
    /// Timestamp of last successful tool call (ms since epoch).
    last_successful_call_timestamp: IntGauge,

    // ── PIP-2751: Capability funnel metrics ───────────────────────────────
    /// Skill searches by result (zero, nonzero).
    skill_searches_total: IntCounterVec,
    /// Skill loads.
    skill_loads_total: IntCounter,
    /// Skill calls by result (success, error).
    skill_calls_total: IntCounterVec,
    /// Time from search to first successful call (seconds).
    skill_first_success_duration_seconds: Histogram,
    /// Script fallback calls.
    skill_script_fallbacks_total: IntCounter,
    /// UI control fallback calls.
    skill_ui_control_fallbacks_total: IntCounter,

    // ── PIP-2751: Artifact metrics ────────────────────────────────────────
    /// Artifacts generated by type (mesh, texture, animation, scene, other).
    artifacts_generated_total: IntCounterVec,
    /// Artifacts saved.
    artifacts_saved_total: IntCounter,
    /// Artifacts exported.
    artifacts_exported_total: IntCounter,
    /// Artifacts validated by result (ok, fail).
    artifacts_validated_total: IntCounterVec,
    /// Task results by status (ok, fail, cancelled).
    task_results_total: IntCounterVec,

    // ── PIP-2751: Resource efficiency metrics ─────────────────────────────
    /// CPU usage percent (0-100).
    cpu_usage_percent: Gauge,
    /// Memory usage in bytes.
    memory_usage_bytes: Gauge,
    /// GPU usage percent (0-100).
    gpu_usage_percent: Gauge,
    /// Token usage total.
    token_usage_total: IntCounter,
    /// Network bytes total.
    network_bytes_total: IntCounter,
    /// Response compression ratio.
    response_compression_ratio: Gauge,

    #[allow(dead_code)]
    build_info: GaugeVec,

    /// Optional bridge into the existing ToolRecorder. When set, a
    /// scrape will refresh Prometheus counters from ToolRecorder
    /// aggregate state for tools that the exporter has not yet seen
    /// directly (e.g. tools that recorded calls before the exporter was
    /// attached). Not a hard dependency — the exporter works fine with
    /// it unset, and `ToolRecorder` works fine without the exporter.
    recorder: Mutex<Option<ToolRecorder>>,
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
            "Number of tools currently registered in the ToolRegistry.",
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

        let gateway_backend_errors_total = IntCounterVec::new(
            Opts::new(
                "dcc_mcp_gateway_backend_errors_total",
                "Gateway-to-backend call failures by error class (transport, HTTP class, JSON-RPC, …).",
            ),
            &["kind"],
        )
        .expect("static metric definition");
        registry
            .register(Box::new(gateway_backend_errors_total.clone()))
            .expect("unique registration");

        let gateway_searches_total = IntCounterVec::new(
            Opts::new(
                "dcc_mcp_gateway_searches_total",
                "Gateway search requests by bounded result class.",
            ),
            &["result"],
        )
        .expect("static metric definition");
        registry
            .register(Box::new(gateway_searches_total.clone()))
            .expect("unique registration");

        let gateway_search_followups_total = IntCounterVec::new(
            Opts::new(
                "dcc_mcp_gateway_search_followups_total",
                "Gateway search follow-up operations by type and selected-rank bucket.",
            ),
            &["kind", "rank_bucket"],
        )
        .expect("static metric definition");
        registry
            .register(Box::new(gateway_search_followups_total.clone()))
            .expect("unique registration");

        let gateway_search_reformulations_total = IntCounter::with_opts(Opts::new(
            "dcc_mcp_gateway_search_reformulations_total",
            "Gateway searches that reformulate a recent unsuccessful query.",
        ))
        .expect("static metric definition");
        registry
            .register(Box::new(gateway_search_reformulations_total.clone()))
            .expect("unique registration");

        let gateway_search_time_to_first_success_seconds = Histogram::with_opts(
            HistogramOpts::new(
                "dcc_mcp_gateway_search_time_to_first_success_seconds",
                "Seconds from a gateway search to the first successful correlated tool call.",
            )
            .buckets(DURATION_BUCKETS_SECONDS.to_vec()),
        )
        .expect("static metric definition");
        registry
            .register(Box::new(
                gateway_search_time_to_first_success_seconds.clone(),
            ))
            .expect("unique registration");

        let gateway_governance_events_total = IntCounterVec::new(
            Opts::new(
                "dcc_mcp_gateway_governance_events_total",
                "Gateway governance outcomes by bounded category and outcome.",
            ),
            &["category", "outcome"],
        )
        .expect("static metric definition");
        registry
            .register(Box::new(gateway_governance_events_total.clone()))
            .expect("unique registration");

        // ── PIP-2751: Thread/queue metrics ────────────────────────────────
        let queue_wait_seconds = HistogramVec::new(
            HistogramOpts::new(
                "dcc_mcp_queue_wait_seconds",
                "Queue wait time in seconds (enqueue to dispatch start).",
            )
            .buckets(DURATION_BUCKETS_SECONDS.to_vec()),
            &["tool"],
        )
        .expect("static metric definition");
        registry
            .register(Box::new(queue_wait_seconds.clone()))
            .expect("unique registration");

        let host_execution_seconds = HistogramVec::new(
            HistogramOpts::new(
                "dcc_mcp_host_execution_seconds",
                "Host (DCC) execution time in seconds (excluding queue wait).",
            )
            .buckets(DURATION_BUCKETS_SECONDS.to_vec()),
            &["tool"],
        )
        .expect("static metric definition");
        registry
            .register(Box::new(host_execution_seconds.clone()))
            .expect("unique registration");

        let queue_depth = IntGauge::with_opts(Opts::new(
            "dcc_mcp_queue_depth",
            "Current depth of the tool dispatch queue.",
        ))
        .expect("static metric definition");
        registry
            .register(Box::new(queue_depth.clone()))
            .expect("unique registration");

        let in_flight_requests = IntGauge::with_opts(Opts::new(
            "dcc_mcp_in_flight_requests",
            "Number of currently in-flight requests.",
        ))
        .expect("static metric definition");
        registry
            .register(Box::new(in_flight_requests.clone()))
            .expect("unique registration");

        // ── PIP-2751: Coverage metrics ────────────────────────────────────
        let observed_requests_total = IntCounter::with_opts(Opts::new(
            "dcc_mcp_observed_requests_total",
            "Total observed requests (gateway-proxied).",
        ))
        .expect("static metric definition");
        registry
            .register(Box::new(observed_requests_total.clone()))
            .expect("unique registration");

        let unobserved_requests_total = IntCounter::with_opts(Opts::new(
            "dcc_mcp_unobserved_requests_total",
            "Total unobserved requests (CLI direct, not proxied).",
        ))
        .expect("static metric definition");
        registry
            .register(Box::new(unobserved_requests_total.clone()))
            .expect("unique registration");

        // ── PIP-2751: Stability metrics ───────────────────────────────────
        let crashes_total = IntCounterVec::new(
            Opts::new(
                "dcc_mcp_crashes_total",
                "Total crashes by type (host, gpu, other).",
            ),
            &["type"],
        )
        .expect("static metric definition");
        registry
            .register(Box::new(crashes_total.clone()))
            .expect("unique registration");

        let reconnects_total = IntCounter::with_opts(Opts::new(
            "dcc_mcp_reconnects_total",
            "Total number of reconnection attempts.",
        ))
        .expect("static metric definition");
        registry
            .register(Box::new(reconnects_total.clone()))
            .expect("unique registration");

        let recoveries_total = IntCounter::with_opts(Opts::new(
            "dcc_mcp_recoveries_total",
            "Total number of successful recoveries.",
        ))
        .expect("static metric definition");
        registry
            .register(Box::new(recoveries_total.clone()))
            .expect("unique registration");

        let last_successful_call_timestamp = IntGauge::with_opts(Opts::new(
            "dcc_mcp_last_successful_call_timestamp",
            "Unix timestamp (ms) of the last successful tool call.",
        ))
        .expect("static metric definition");
        registry
            .register(Box::new(last_successful_call_timestamp.clone()))
            .expect("unique registration");

        // ── PIP-2751: Capability funnel metrics ───────────────────────────
        let skill_searches_total = IntCounterVec::new(
            Opts::new(
                "dcc_mcp_skill_searches_total",
                "Total skill searches by result (zero, nonzero).",
            ),
            &["result"],
        )
        .expect("static metric definition");
        registry
            .register(Box::new(skill_searches_total.clone()))
            .expect("unique registration");

        let skill_loads_total = IntCounter::with_opts(Opts::new(
            "dcc_mcp_skill_loads_total",
            "Total number of skill loads.",
        ))
        .expect("static metric definition");
        registry
            .register(Box::new(skill_loads_total.clone()))
            .expect("unique registration");

        let skill_calls_total = IntCounterVec::new(
            Opts::new(
                "dcc_mcp_skill_calls_total",
                "Total skill calls by result (success, error).",
            ),
            &["result"],
        )
        .expect("static metric definition");
        registry
            .register(Box::new(skill_calls_total.clone()))
            .expect("unique registration");

        let skill_first_success_duration_seconds = Histogram::with_opts(
            HistogramOpts::new(
                "dcc_mcp_skill_first_success_duration_seconds",
                "Seconds from skill search to first successful call.",
            )
            .buckets(DURATION_BUCKETS_SECONDS.to_vec()),
        )
        .expect("static metric definition");
        registry
            .register(Box::new(skill_first_success_duration_seconds.clone()))
            .expect("unique registration");

        let skill_script_fallbacks_total = IntCounter::with_opts(Opts::new(
            "dcc_mcp_skill_script_fallbacks_total",
            "Total script fallback calls.",
        ))
        .expect("static metric definition");
        registry
            .register(Box::new(skill_script_fallbacks_total.clone()))
            .expect("unique registration");

        let skill_ui_control_fallbacks_total = IntCounter::with_opts(Opts::new(
            "dcc_mcp_skill_ui_control_fallbacks_total",
            "Total UI control fallback calls.",
        ))
        .expect("static metric definition");
        registry
            .register(Box::new(skill_ui_control_fallbacks_total.clone()))
            .expect("unique registration");

        // ── PIP-2751: Artifact metrics ────────────────────────────────────
        let artifacts_generated_total = IntCounterVec::new(
            Opts::new(
                "dcc_mcp_artifacts_generated_total",
                "Total artifacts generated by type.",
            ),
            &["type"],
        )
        .expect("static metric definition");
        registry
            .register(Box::new(artifacts_generated_total.clone()))
            .expect("unique registration");

        let artifacts_saved_total = IntCounter::with_opts(Opts::new(
            "dcc_mcp_artifacts_saved_total",
            "Total artifacts saved.",
        ))
        .expect("static metric definition");
        registry
            .register(Box::new(artifacts_saved_total.clone()))
            .expect("unique registration");

        let artifacts_exported_total = IntCounter::with_opts(Opts::new(
            "dcc_mcp_artifacts_exported_total",
            "Total artifacts exported.",
        ))
        .expect("static metric definition");
        registry
            .register(Box::new(artifacts_exported_total.clone()))
            .expect("unique registration");

        let artifacts_validated_total = IntCounterVec::new(
            Opts::new(
                "dcc_mcp_artifacts_validated_total",
                "Total artifacts validated by result.",
            ),
            &["result"],
        )
        .expect("static metric definition");
        registry
            .register(Box::new(artifacts_validated_total.clone()))
            .expect("unique registration");

        let task_results_total = IntCounterVec::new(
            Opts::new(
                "dcc_mcp_task_results_total",
                "Total task results by status.",
            ),
            &["status"],
        )
        .expect("static metric definition");
        registry
            .register(Box::new(task_results_total.clone()))
            .expect("unique registration");

        // ── PIP-2751: Resource efficiency metrics ─────────────────────────
        let cpu_usage_percent = Gauge::with_opts(Opts::new(
            "dcc_mcp_cpu_usage_percent",
            "CPU usage percent (0-100).",
        ))
        .expect("static metric definition");
        registry
            .register(Box::new(cpu_usage_percent.clone()))
            .expect("unique registration");

        let memory_usage_bytes = Gauge::with_opts(Opts::new(
            "dcc_mcp_memory_usage_bytes",
            "Memory usage in bytes.",
        ))
        .expect("static metric definition");
        registry
            .register(Box::new(memory_usage_bytes.clone()))
            .expect("unique registration");

        let gpu_usage_percent = Gauge::with_opts(Opts::new(
            "dcc_mcp_gpu_usage_percent",
            "GPU usage percent (0-100).",
        ))
        .expect("static metric definition");
        registry
            .register(Box::new(gpu_usage_percent.clone()))
            .expect("unique registration");

        let token_usage_total =
            IntCounter::with_opts(Opts::new("dcc_mcp_token_usage_total", "Total token usage."))
                .expect("static metric definition");
        registry
            .register(Box::new(token_usage_total.clone()))
            .expect("unique registration");

        let network_bytes_total = IntCounter::with_opts(Opts::new(
            "dcc_mcp_network_bytes_total",
            "Total network bytes.",
        ))
        .expect("static metric definition");
        registry
            .register(Box::new(network_bytes_total.clone()))
            .expect("unique registration");

        let response_compression_ratio = Gauge::with_opts(Opts::new(
            "dcc_mcp_response_compression_ratio",
            "Response compression ratio.",
        ))
        .expect("static metric definition");
        registry
            .register(Box::new(response_compression_ratio.clone()))
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
                gateway_backend_errors_total,
                gateway_searches_total,
                gateway_search_followups_total,
                gateway_search_reformulations_total,
                gateway_search_time_to_first_success_seconds,
                gateway_governance_events_total,
                queue_wait_seconds,
                host_execution_seconds,
                queue_depth,
                in_flight_requests,
                observed_requests_total,
                unobserved_requests_total,
                crashes_total,
                reconnects_total,
                recoveries_total,
                last_successful_call_timestamp,
                skill_searches_total,
                skill_loads_total,
                skill_calls_total,
                skill_first_success_duration_seconds,
                skill_script_fallbacks_total,
                skill_ui_control_fallbacks_total,
                artifacts_generated_total,
                artifacts_saved_total,
                artifacts_exported_total,
                artifacts_validated_total,
                task_results_total,
                cpu_usage_percent,
                memory_usage_bytes,
                gpu_usage_percent,
                token_usage_total,
                network_bytes_total,
                response_compression_ratio,
                build_info,
                recorder: Mutex::new(None),
            }),
        }
    }

    /// Attach an [`ToolRecorder`] so scrapes can reconcile any counts
    /// that were recorded on the recorder before the exporter was
    /// attached. Optional — call sites that record directly via
    /// [`record_tool_call`](Self::record_tool_call) do not need this.
    pub fn with_recorder(self, recorder: ToolRecorder) -> Self {
        *self.inner.recorder.lock() = Some(recorder);
        self
    }

    /// Record a completed tool call.
    ///
    /// * `tool`    — fully-qualified tool name (matches what the MCP
    ///   client called).
    /// * `status`  — `"success"` or `"error"`. Any other value is
    ///   passed through unchanged to Prometheus.
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

    /// Record a gateway → backend failure for `/metrics` (`dcc_mcp_gateway_backend_errors_total`).
    ///
    /// `kind` must stay a **small fixed vocabulary** (e.g. `transport`, `http_5xx`,
    /// `jsonrpc_backend`) so scrapers do not suffer unbounded cardinality.
    pub fn record_gateway_backend_error(&self, kind: &str) {
        self.inner
            .gateway_backend_errors_total
            .with_label_values(&[kind])
            .inc();
    }

    /// Record a gateway search request by bounded result class (`zero` or `nonzero`).
    pub fn record_gateway_search(&self, result: &str) {
        self.inner
            .gateway_searches_total
            .with_label_values(&[result])
            .inc();
    }

    /// Record a follow-up selected from a prior search.
    pub fn record_gateway_search_followup(&self, kind: &str, rank_bucket: &str) {
        self.inner
            .gateway_search_followups_total
            .with_label_values(&[kind, rank_bucket])
            .inc();
    }

    /// Record a query reformulation after a recent unsuccessful search.
    pub fn record_gateway_search_reformulation(&self) {
        self.inner.gateway_search_reformulations_total.inc();
    }

    /// Observe the time from search response to first successful correlated call.
    pub fn observe_gateway_search_time_to_first_success(&self, duration: std::time::Duration) {
        self.inner
            .gateway_search_time_to_first_success_seconds
            .observe(duration.as_secs_f64());
    }

    /// Record a bounded governance event for policy, capture, privacy, or pressure controls.
    pub fn record_gateway_governance_event(&self, category: &str, outcome: &str) {
        self.inner
            .gateway_governance_events_total
            .with_label_values(&[category, outcome])
            .inc();
    }

    // ── PIP-2751: Thread/queue metric recording ───────────────────────────

    /// Observe queue wait time for a tool call.
    pub fn observe_queue_wait(&self, tool: &str, duration: std::time::Duration) {
        self.inner
            .queue_wait_seconds
            .with_label_values(&[tool])
            .observe(duration.as_secs_f64());
    }

    /// Observe host execution time for a tool call (excludes queue wait).
    pub fn observe_host_execution(&self, tool: &str, duration: std::time::Duration) {
        self.inner
            .host_execution_seconds
            .with_label_values(&[tool])
            .observe(duration.as_secs_f64());
    }

    /// Set the queue depth gauge.
    pub fn set_queue_depth(&self, n: i64) {
        self.inner.queue_depth.set(n);
    }

    /// Set the in-flight requests gauge.
    pub fn set_in_flight_requests(&self, n: i64) {
        self.inner.in_flight_requests.set(n);
    }

    /// Increment the in-flight requests gauge.
    pub fn inc_in_flight_requests(&self) {
        self.inner.in_flight_requests.inc();
    }

    /// Decrement the in-flight requests gauge.
    pub fn dec_in_flight_requests(&self) {
        self.inner.in_flight_requests.dec();
    }

    // ── PIP-2751: Coverage metric recording ───────────────────────────────

    /// Record an observed request (gateway-proxied).
    pub fn record_observed_request(&self) {
        self.inner.observed_requests_total.inc();
    }

    /// Record an unobserved request (CLI direct, not proxied).
    pub fn record_unobserved_request(&self) {
        self.inner.unobserved_requests_total.inc();
    }

    // ── PIP-2751: Stability metric recording ──────────────────────────────

    /// Record a crash by type.
    pub fn record_crash(&self, crash_type: &str) {
        self.inner
            .crashes_total
            .with_label_values(&[crash_type])
            .inc();
    }

    /// Record a reconnect attempt.
    pub fn record_reconnect(&self) {
        self.inner.reconnects_total.inc();
    }

    /// Record a successful recovery.
    pub fn record_recovery(&self) {
        self.inner.recoveries_total.inc();
    }

    /// Record the timestamp of the last successful tool call.
    pub fn set_last_successful_call_timestamp(&self, timestamp_ms: i64) {
        self.inner.last_successful_call_timestamp.set(timestamp_ms);
    }

    // ── PIP-2751: Capability funnel metric recording ──────────────────────

    /// Record a skill search result.
    pub fn record_skill_search(&self, result: &str) {
        self.inner
            .skill_searches_total
            .with_label_values(&[result])
            .inc();
    }

    /// Record a skill load.
    pub fn record_skill_load(&self) {
        self.inner.skill_loads_total.inc();
    }

    /// Record a skill call result.
    pub fn record_skill_call(&self, result: &str) {
        self.inner
            .skill_calls_total
            .with_label_values(&[result])
            .inc();
    }

    /// Observe the time from skill search to first successful call.
    pub fn observe_skill_first_success_duration(&self, duration: std::time::Duration) {
        self.inner
            .skill_first_success_duration_seconds
            .observe(duration.as_secs_f64());
    }

    /// Record a script fallback.
    pub fn record_skill_script_fallback(&self) {
        self.inner.skill_script_fallbacks_total.inc();
    }

    /// Record a UI control fallback.
    pub fn record_skill_ui_control_fallback(&self) {
        self.inner.skill_ui_control_fallbacks_total.inc();
    }

    // ── PIP-2751: Artifact metric recording ───────────────────────────────

    /// Record a generated artifact by type.
    pub fn record_artifact_generated(&self, artifact_type: &str) {
        self.inner
            .artifacts_generated_total
            .with_label_values(&[artifact_type])
            .inc();
    }

    /// Record a saved artifact.
    pub fn record_artifact_saved(&self) {
        self.inner.artifacts_saved_total.inc();
    }

    /// Record an exported artifact.
    pub fn record_artifact_exported(&self) {
        self.inner.artifacts_exported_total.inc();
    }

    /// Record a validated artifact by result.
    pub fn record_artifact_validated(&self, result: &str) {
        self.inner
            .artifacts_validated_total
            .with_label_values(&[result])
            .inc();
    }

    /// Record a task result by status.
    pub fn record_task_result(&self, status: &str) {
        self.inner
            .task_results_total
            .with_label_values(&[status])
            .inc();
    }

    // ── PIP-2751: Resource efficiency metric recording ────────────────────

    /// Set the CPU usage percent gauge.
    pub fn set_cpu_usage_percent(&self, pct: f64) {
        self.inner.cpu_usage_percent.set(pct);
    }

    /// Set the memory usage bytes gauge.
    pub fn set_memory_usage_bytes(&self, bytes: f64) {
        self.inner.memory_usage_bytes.set(bytes);
    }

    /// Set the GPU usage percent gauge.
    pub fn set_gpu_usage_percent(&self, pct: f64) {
        self.inner.gpu_usage_percent.set(pct);
    }

    /// Add token usage.
    pub fn add_token_usage(&self, tokens: i64) {
        self.inner.token_usage_total.inc_by(tokens as u64);
    }

    /// Add network bytes.
    pub fn add_network_bytes(&self, bytes: i64) {
        self.inner.network_bytes_total.inc_by(bytes as u64);
    }

    /// Set the response compression ratio.
    pub fn set_response_compression_ratio(&self, ratio: f64) {
        self.inner.response_compression_ratio.set(ratio);
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
        exp.record_gateway_backend_error("seed");
        exp.record_gateway_search("nonzero");
        exp.record_gateway_search_followup("describe", "top1");
        exp.record_gateway_search_reformulation();
        exp.observe_gateway_search_time_to_first_success(Duration::from_millis(10));
        exp.record_gateway_governance_event("policy", "denied");
        // PIP-2751: seed new metrics
        exp.observe_queue_wait("seed", Duration::from_millis(1));
        exp.observe_host_execution("seed", Duration::from_millis(1));
        exp.set_queue_depth(0);
        exp.set_in_flight_requests(0);
        exp.record_observed_request();
        exp.record_unobserved_request();
        exp.record_crash("host");
        exp.record_reconnect();
        exp.record_recovery();
        exp.set_last_successful_call_timestamp(1_700_000_000_000);
        exp.record_skill_search("nonzero");
        exp.record_skill_load();
        exp.record_skill_call("success");
        exp.observe_skill_first_success_duration(Duration::from_millis(10));
        exp.record_skill_script_fallback();
        exp.record_skill_ui_control_fallback();
        exp.record_artifact_generated("mesh");
        exp.record_artifact_saved();
        exp.record_artifact_exported();
        exp.record_artifact_validated("ok");
        exp.record_task_result("ok");
        exp.set_cpu_usage_percent(50.0);
        exp.set_memory_usage_bytes(1024.0 * 1024.0 * 1024.0);
        exp.set_gpu_usage_percent(75.0);
        exp.add_token_usage(1000);
        exp.add_network_bytes(50000);
        exp.set_response_compression_ratio(0.3);
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
            "dcc_mcp_gateway_backend_errors_total",
            "dcc_mcp_gateway_searches_total",
            "dcc_mcp_gateway_search_followups_total",
            "dcc_mcp_gateway_search_reformulations_total",
            "dcc_mcp_gateway_search_time_to_first_success_seconds",
            "dcc_mcp_gateway_governance_events_total",
            "dcc_mcp_build_info",
            // PIP-2751: thread/queue
            "dcc_mcp_queue_wait_seconds",
            "dcc_mcp_host_execution_seconds",
            "dcc_mcp_queue_depth",
            "dcc_mcp_in_flight_requests",
            // PIP-2751: coverage
            "dcc_mcp_observed_requests_total",
            "dcc_mcp_unobserved_requests_total",
            // PIP-2751: stability
            "dcc_mcp_crashes_total",
            "dcc_mcp_reconnects_total",
            "dcc_mcp_recoveries_total",
            "dcc_mcp_last_successful_call_timestamp",
            // PIP-2751: funnel
            "dcc_mcp_skill_searches_total",
            "dcc_mcp_skill_loads_total",
            "dcc_mcp_skill_calls_total",
            "dcc_mcp_skill_first_success_duration_seconds",
            "dcc_mcp_skill_script_fallbacks_total",
            "dcc_mcp_skill_ui_control_fallbacks_total",
            // PIP-2751: artifacts
            "dcc_mcp_artifacts_generated_total",
            "dcc_mcp_artifacts_saved_total",
            "dcc_mcp_artifacts_exported_total",
            "dcc_mcp_artifacts_validated_total",
            "dcc_mcp_task_results_total",
            // PIP-2751: resources
            "dcc_mcp_cpu_usage_percent",
            "dcc_mcp_memory_usage_bytes",
            "dcc_mcp_gpu_usage_percent",
            "dcc_mcp_token_usage_total",
            "dcc_mcp_network_bytes_total",
            "dcc_mcp_response_compression_ratio",
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
        assert!(out.contains("# TYPE dcc_mcp_gateway_searches_total counter"));
        assert!(out.contains("# TYPE dcc_mcp_gateway_governance_events_total counter"));
        assert!(
            out.contains("# TYPE dcc_mcp_gateway_search_time_to_first_success_seconds histogram")
        );
    }

    #[test]
    fn gateway_search_metrics_are_recorded() {
        let exp = PrometheusExporter::new();
        exp.record_gateway_search("zero");
        exp.record_gateway_search_followup("call", "top3");
        exp.record_gateway_search_reformulation();
        exp.observe_gateway_search_time_to_first_success(Duration::from_millis(250));
        exp.record_gateway_governance_event("rate-limit", "throttled");

        let out = exp.render().unwrap();
        assert!(out.contains(r#"dcc_mcp_gateway_searches_total{result="zero"} 1"#));
        assert!(out.contains(
            r#"dcc_mcp_gateway_search_followups_total{kind="call",rank_bucket="top3"} 1"#
        ));
        assert!(out.contains("dcc_mcp_gateway_search_reformulations_total 1"));
        assert!(out.contains("dcc_mcp_gateway_search_time_to_first_success_seconds_count 1"));
        assert!(
            out.contains(
                r#"dcc_mcp_gateway_governance_events_total{category="rate-limit",outcome="throttled"} 1"#
            )
        );
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
        let recorder = ToolRecorder::new("test-scope");
        recorder.start("my_tool", "maya").finish(true);
        recorder.start("my_tool", "maya").finish(true);
        recorder.start("my_tool", "maya").finish(false);

        let exp = PrometheusExporter::new().with_recorder(recorder);
        let out = exp.render().unwrap();

        assert!(out.contains(r#"dcc_mcp_tool_calls_total{status="success",tool="my_tool"} 2"#));
        assert!(out.contains(r#"dcc_mcp_tool_calls_total{status="error",tool="my_tool"} 1"#));
    }
}
