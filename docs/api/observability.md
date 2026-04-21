# Observability — Prometheus `/metrics` exporter (issue #331)

`dcc-mcp-core` ships an **opt-in** Prometheus text-exposition exporter
built on top of [`dcc-mcp-telemetry`](../guide/telemetry.md). When
enabled it mounts a `GET /metrics` endpoint on the same Axum router
that serves `/mcp`, so a single TLS terminator / ingress rule covers
both.

> **Feature-gated.** The exporter is behind the `prometheus` Cargo
> feature and is **off by default**. When not compiled in, zero
> Prometheus code enters the wheel — matching the zero-runtime-cost
> contract in `pyproject.toml`.

## Enable in a wheel build

```bash
# From the repo root
maturin develop --features python-bindings,ext-module,workflow,prometheus
```

From Rust call sites:

```toml
# Cargo.toml
[dependencies]
dcc-mcp-http = { path = "...", features = ["prometheus"] }
```

## Enable at runtime

```python
from dcc_mcp_core import McpHttpConfig, McpHttpServer, ToolRegistry

cfg = McpHttpConfig(
    port=8765,
    server_name="maya-mcp",
    enable_prometheus=True,
    # Optional HTTP Basic auth guard — strongly recommended for any
    # deployment beyond a trusted localhost.
    prometheus_basic_auth=("scraper", "change-me"),
)

server = McpHttpServer(ToolRegistry(), cfg)
handle = server.start()
# Scrape: GET http://127.0.0.1:8765/metrics
```

If the wheel was **not** built with `prometheus`, the two flags are
accepted for forward compatibility but `GET /metrics` returns 404.

## Metrics surface

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `dcc_mcp_tool_calls_total` | counter | `tool`, `status` (`success`/`error`) | Tool invocations observed by the server. |
| `dcc_mcp_tool_duration_seconds` | histogram | `tool` | Wall-clock duration from dispatch to completion. |
| `dcc_mcp_jobs_in_flight` | gauge | `tool` | Long-running jobs (issue #316) currently executing. |
| `dcc_mcp_job_created_total` | counter | `tool`, `result` | Jobs ever created — `result` ∈ {`accepted`, `queue_full`, ...}. |
| `dcc_mcp_job_wait_seconds` | histogram | `tool` | Delay between job creation and first execution. |
| `dcc_mcp_notifications_sent_total` | counter | `channel` (`sse`, `ws`) | MCP notifications pushed to clients (issue #326). |
| `dcc_mcp_active_sessions` | gauge | — | Live MCP Streamable HTTP sessions. |
| `dcc_mcp_registered_tools` | gauge | — | Tools currently registered in `ActionRegistry`. |
| `dcc_mcp_build_info` | gauge | `version`, `crate` | Always `1`; labels identify the running binary. |

Histograms use a log-ish ladder of buckets appropriate for DCC tool
calls (1 ms → 30 s). The exporter publishes a single `dcc_mcp_build_info`
series on startup so scrapers always see a non-empty payload.

## Basic auth

When `prometheus_basic_auth=(user, pass)` is set the endpoint responds
with `401 Unauthorized` and a `WWW-Authenticate: Basic realm="dcc-mcp
metrics"` header to any request without matching credentials.
Comparison uses a short constant-time byte check to thwart trivial
timing attacks.

```bash
curl -u scraper:change-me http://127.0.0.1:8765/metrics
```

Without credentials configured, the endpoint is **open** — acceptable
for localhost-only development but never recommended for production.

## Prometheus scrape config

```yaml
# prometheus.yml
scrape_configs:
  - job_name: dcc-mcp
    scrape_interval: 15s
    static_configs:
      - targets: ["maya-host:8765", "blender-host:8765"]
    metrics_path: /metrics
    basic_auth:
      username: scraper
      password_file: /etc/prometheus/dcc-mcp.pass
```

## Grafana — example queries

Tool-level success rate (drop-in PromQL):

```promql
sum by (tool) (rate(dcc_mcp_tool_calls_total{status="success"}[5m]))
  /
sum by (tool) (rate(dcc_mcp_tool_calls_total[5m]))
```

P95 tool latency:

```promql
histogram_quantile(
  0.95,
  sum by (le, tool) (rate(dcc_mcp_tool_duration_seconds_bucket[5m]))
)
```

Active sessions per DCC host:

```promql
dcc_mcp_active_sessions
```

Dashboard JSON for Grafana is intentionally **not** shipped with this
PR — once the 2026 observability roadmap settles we will provide a
reference dashboard. Until then the queries above are stable.

## Design notes

- **One registry per server.** Multiple `McpHttpServer` instances in
  the same process (gateway + per-DCC) get independent registries so
  labels don't collide. The exporter does not use
  `prometheus::default_registry()`.
- **Zero-cost when off.** Every recording site is guarded by a cheap
  `Option::is_some` check on `AppState::prometheus`. With the Cargo
  feature disabled, the field and its usages are compiled out entirely.
- **Single recording site.** Tool-call counters advance only from the
  `tools/call` wrapper in `handler.rs`; `handle_call_action` recurses
  through the inner variant to avoid double-counting.
- **Background gauge updater.** A 5-second ticker refreshes
  `active_sessions` and `registered_tools` so scrapes don't need to
  rebuild the counts.

## Non-goals

- OpenTelemetry OTLP tracing — covered separately, see the OTLP
  section of [`docs/guide/telemetry.md`](../guide/telemetry.md).
- Alerting rules and a Grafana dashboard JSON — deferred.
- Prometheus push-gateway support — not planned.

## Related

- Source: `crates/dcc-mcp-telemetry/src/prometheus.rs`,
  `crates/dcc-mcp-http/src/metrics.rs`.
- Tests: `crates/dcc-mcp-http/tests/prometheus_endpoint.rs`,
  `tests/test_prometheus.py`.
- Issue: [#331](https://github.com/loonghao/dcc-mcp-core/issues/331).
