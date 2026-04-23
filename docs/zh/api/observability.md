# 可观测性 — Prometheus `/metrics` 导出器 (issue #331)

`dcc-mcp-core` 提供了一个**可选的** Prometheus text-exposition 导出器，
构建在 [`dcc-mcp-telemetry`](../guide/telemetry.md) 之上。启用时，
它在服务 `/mcp` 的同一个 Axum 路由器上挂载 `GET /metrics` 端点，
因此单个 TLS 终止器 / ingress 规则即可覆盖两者。

> **Feature-gated。** 导出器位于 `prometheus` Cargo feature 之后，
> **默认关闭**。未编译时，零 Prometheus 代码进入 wheel — 匹配
> `pyproject.toml` 中的零运行时成本约定。

## 在 wheel 构建中启用

```bash
# 从仓库根目录
maturin develop --features python-bindings,ext-module,workflow,prometheus
```

从 Rust 调用点：

```toml
# Cargo.toml
[dependencies]
dcc-mcp-http = { path = "...", features = ["prometheus"] }
```

## 运行时启用

```python
from dcc_mcp_core import McpHttpConfig, McpHttpServer, ToolRegistry

cfg = McpHttpConfig(
    port=8765,
    server_name="maya-mcp",
    enable_prometheus=True,
    # 可选 HTTP Basic auth 守卫 — 强烈建议用于任何超出可信 localhost 的部署。
    prometheus_basic_auth=("scraper", "change-me"),
)

server = McpHttpServer(ToolRegistry(), cfg)
handle = server.start()
# Scrape: GET http://127.0.0.1:8765/metrics
```

如果 wheel **没有**使用 `prometheus` 构建，两个标志仍会被接受以保持
向前兼容，但 `GET /metrics` 返回 404。

## 指标面

| 指标 | 类型 | 标签 | 说明 |
|------|------|------|------|
| `dcc_mcp_tool_calls_total` | counter | `tool`, `status` (`success`/`error`) | 服务器观察到的工具调用。 |
| `dcc_mcp_tool_duration_seconds` | histogram | `tool` | 从分派到完成的 wall-clock 耗时。 |
| `dcc_mcp_jobs_in_flight` | gauge | `tool` | 当前执行的长时间运行作业 (issue #316)。 |
| `dcc_mcp_job_created_total` | counter | `tool`, `result` | 曾创建的作业 — `result` ∈ {`accepted`, `queue_full`, ...}。 |
| `dcc_mcp_job_wait_seconds` | histogram | `tool` | 作业创建到首次执行之间的延迟。 |
| `dcc_mcp_notifications_sent_total` | counter | `channel` (`sse`, `ws`) | 推送到客户端的 MCP 通知 (issue #326)。 |
| `dcc_mcp_active_sessions` | gauge | — | 活跃的 MCP Streamable HTTP 会话。 |
| `dcc_mcp_registered_tools` | gauge | — | 当前在 `ActionRegistry` 中注册的工具。 |
| `dcc_mcp_build_info` | gauge | `version`, `crate` | 始终为 `1`；标签标识运行中的二进制文件。 |

Histogram 使用适合 DCC 工具调用的对数阶梯 bucket（1 ms → 30 s）。
导出器在启动时发布单个 `dcc_mcp_build_info` 序列，以便 scraper
始终看到非空 payload。

## Basic auth

当设置了 `prometheus_basic_auth=(user, pass)` 时，端点对任何没有
匹配凭证的请求回复 `401 Unauthorized` 和
`WWW-Authenticate: Basic realm="dcc-mcp metrics"` 头。
比较使用短常数时间字节检查以防止简单定时攻击。

```bash
curl -u scraper:change-me http://127.0.0.1:8765/metrics
```

未配置凭证时，端点是**开放的** — 对仅 localhost 的开发可以接受，
但绝不建议用于生产环境。

## Prometheus scrape 配置

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

## Grafana — 示例查询

工具级成功率（直接可用的 PromQL）：

```promql
sum by (tool) (rate(dcc_mcp_tool_calls_total{status="success"}[5m]))
  /
sum by (tool) (rate(dcc_mcp_tool_calls_total[5m]))
```

P95 工具延迟：

```promql
histogram_quantile(
  0.95,
  sum by (le, tool) (rate(dcc_mcp_tool_duration_seconds_bucket[5m]))
)
```

每 DCC 主机的活跃会话：

```promql
dcc_mcp_active_sessions
```

本 PR 故意**不**随附 Grafana Dashboard JSON — 一旦 2026 可观测性
路线图确定，我们将提供一个参考 dashboard。在此之前，
以上查询是稳定的。

## 设计说明

- **每服务器一个注册表。** 同一进程中的多个 `McpHttpServer` 实例
 （gateway + 每个 DCC）获得独立的注册表，因此标签不会冲突。
 导出器不使用 `prometheus::default_registry()`。
- **关闭时零成本。** 每个记录点都被 `AppState::prometheus` 上的廉价
  `Option::is_some` 检查守卫。禁用 Cargo feature 时，字段及其
  使用会被完全编译掉。
- **单一记录点。** 工具调用计数器仅从 `handler.rs` 中的 `tools/call`
 包装器推进；`handle_call_action` 通过内部变体递归以避免重复计数。
- **后台 gauge 更新器。** 一个 5 秒 ticker 刷新 `active_sessions` 和
  `registered_tools`，因此 scrape 不需要重建计数。

## 非目标

- OpenTelemetry OTLP tracing — 单独覆盖，参见 [`docs/guide/telemetry.md`](../guide/telemetry.md) 的 OTLP 部分。
- 告警规则和 Grafana dashboard JSON — 推迟。
- Prometheus push-gateway 支持 — 无计划。

## 相关

- 源码: `crates/dcc-mcp-telemetry/src/prometheus.rs`,
  `crates/dcc-mcp-http/src/metrics.rs`。
- 测试: `crates/dcc-mcp-http/tests/prometheus_endpoint.rs`,
  `tests/test_prometheus.py`。
- Issue: [#331](https://github.com/loonghao/dcc-mcp-core/issues/331)。
