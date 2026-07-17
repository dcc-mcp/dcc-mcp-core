# 可观测性

dcc-mcp-core 提供四种互补的可观测性接口，适用于生产环境部署。

## 1. OTLP 分布式追踪（#768）

将 span 数据发送到任何兼容 OpenTelemetry 的后端（Jaeger、Grafana Tempo、DataDog、New Relic 等）。

### 激活方式

设置标准 `OTEL_*` 环境变量——无需修改代码：

```bash
OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317 \
OTEL_SERVICE_NAME=dcc-mcp-gateway \
  dcc-mcp-server ...
```

### 环境变量

| 变量 | 说明 |
|------|------|
| `OTEL_EXPORTER_OTLP_ENDPOINT` | Collector 端点——设置此变量后自动启用 OTLP |
| `OTEL_SERVICE_NAME` | 覆盖追踪中的服务名 |
| `OTEL_RESOURCE_ATTRIBUTES` | 额外资源属性（`key=val,key2=val2`） |
| `OTEL_EXPORTER_OTLP_HEADERS` | SaaS 后端的认证 Header（如 `api-key=...`） |
| `OTEL_TRACES_SAMPLER` | 采样器类型（`always_on`、`always_off`、`traceidratio`） |
| `OTEL_TRACES_SAMPLER_ARG` | 采样器参数（如 `0.1` 表示 10% 采样率） |

### DCC Span 属性

每次 `tools/call` 追踪均包含：

| 属性 | 示例 | 说明 |
|------|------|------|
| `dcc.type` | `"maya"` | DCC 应用类型 |
| `dcc.instance_id` | `"a1b2c3d4-..."` | DCC 实例唯一 UUID |
| `dcc.scene` | `"/projects/shot01.ma"` | 当前场景路径（已知时） |
| `dcc.job_id` | `"job-..."` | 作业 ID（被 `JobHandle` 包装时） |
| `mcp.method` | `"tools/call"` | MCP 方法名 |
| `mcp.tool_slug` | `"maya__open_scene"` | 完整工具名 |
| `mcp.affinity` | `"main"` | 线程亲和性要求 |
| `mcp.session_id` | `"sess-..."` | MCP 会话 ID |
| `mcp.request_id` | `"req-..."` | 每次请求的唯一 ID |

### Gateway Agent Workflow Spans（#1180）

当 agent 通过 REST 或 MCP 进行动态能力发现与调用时，网关会额外发送有界工作流 span：

| Span | 含义 |
|------|------|
| `gateway.search` | Agent 搜索工具或技能。 |
| `gateway.describe` | Agent 查看选中工具的 schema/说明。 |
| `gateway.load_skill` | Agent 加载搜索得到的技能。 |
| `gateway.call` | Agent 调用一个后端工具。 |
| `gateway.call_batch` | Agent 执行有序批量调用。 |

这些 span 使用 `openinference.span.kind`（search 为 `CHAIN`，describe/load/call 为 `TOOL`），并用 `dcc_mcp.*` 命名空间记录网关语义字段：

| 属性 | 说明 |
|------|------|
| `dcc_mcp.workflow.operation` | 上表中的 span 名称。 |
| `dcc_mcp.transport` | `rest` 或 `mcp`。 |
| `dcc_mcp.trace_id`、`dcc_mcp.request_id`、`dcc_mcp.parent_request_id`、`dcc_mcp.session_id` | 与 Admin trace/debug bundle 对齐的关联 ID。 |
| `dcc_mcp.actor.id`、`.name`、`.email_hash` | 有界的人类或服务账号 actor 归因。 |
| `dcc_mcp.agent.id`、`.name`、`.kind`、`.version`、`.model`、`.model_provider`、`.model_version`、`.reasoning_effort`、`.turn_id`、`.task`、`.tags` | 有界的 agent runtime 元数据。 |
| `dcc_mcp.client.platform`、`.os`、`.host`、`dcc_mcp.auth.subject` | 有界的客户端入口与认证主体元数据。 |
| `dcc_mcp.source.ip`、`dcc_mcp.forwarded_for` | 应用 proxy trust policy 后由服务端派生的网络来源字段。 |
| `dcc_mcp.agent.user_intent_summary`、`.reply_summary`、`.user_input_hash`、`.reply_hash`、`.user_input_chars`、`.reply_chars` | 用于评估关联的低敏 turn 摘要、哈希和长度。 |
| `dcc_mcp.dcc.type`、`dcc_mcp.instance.id`、`dcc_mcp.skill.name`、`dcc_mcp.tool.slug` | 选中的 DCC 路由与技能/工具身份。 |
| `dcc_mcp.search.id`、`.ranker_version`、`.selected_rank`、`.score`、`.match_reasons`、`.total`、`.zero_results` | 从 `/v1/search` 或 gateway `search` 继承的搜索质量上下文。 |
| `dcc_mcp.policy.outcome`、`.reason` | 网关策略是否允许、拒绝或限流，以及原因。 |
| `dcc_mcp.success`、`dcc_mcp.error.kind`、`dcc_mcp.batch.size` | 执行结果字段。 |

网关不会导出隐藏推理、原始 prompt、原始 agent 回复、无界请求体、secret 或任意 `agent_context` metadata。Agent 从搜索结果继续调用 `describe`、`load_skill`、`call` 或 `call_batch` 时，应保留 REST `meta.search_id` 或 MCP `_meta.search_id`，这样 OTLP trace 才能把 selected rank/score 和真实工具结果关联起来。评估需要关联单个 agent turn 时，在 `agent_context` 中加入 `turn_id` 与模型身份字段。

### 工作室级工具使用统计

三类可观测接口应按各自的交付契约使用：

- Gateway Admin SQLite 是工作站本地持久化事实源，并为
  `dcc-mcp-cli stats` 提供历史调用数据；
- OTLP 是工作室集中汇总的主通道，它会批量上报同一组 DCC、Skill、
  Tool、实例、会话、结果和耗时维度到 OpenTelemetry Collector；
- EventBus Webhook 适合告警和低频业务集成，不是权威分析流。它使用有界
  内存队列，队列饱和时会主动丢弃新事件，避免接收端阻塞 DCC 执行。

工作室部署应让各工作站连接本机或共享 Collector，由 Collector 统一负责
TLS、认证、隐私过滤、采样、重试/队列和后端路由。实例 ID、会话 ID 等高
基数信息保留为 Trace 属性；DCC 类型、Skill、Tool、结果等有界维度可在
Collector 中派生聚合指标。

```bash
OTEL_EXPORTER_OTLP_ENDPOINT=https://otel.example.com:4317 \
OTEL_EXPORTER_OTLP_HEADERS="authorization=Bearer%20${OTEL_TOKEN}" \
OTEL_SERVICE_NAME=dcc-mcp-gateway \
  dcc-mcp-server ...
```

存储与故障隔离决策参见
[ADR-013](../../adr/013-persistent-tool-call-analytics.md)。

### Rust 编程式配置

```rust
use dcc_mcp_telemetry::{TelemetryConfig, ExporterBackend};

TelemetryConfig::default()
    .with_otlp_exporter("http://localhost:4317")
    .init()?;
```

需要启用 `otlp-exporter` Cargo feature：

```toml
dcc-mcp-telemetry = { features = ["otlp-exporter"] }
```

### 快速启动：Jaeger All-in-One

```bash
docker run -d --name jaeger \
  -p 4317:4317 \   # OTLP gRPC
  -p 16686:16686 \ # Jaeger UI
  jaegertracing/all-in-one:latest

OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317 \
OTEL_SERVICE_NAME=dcc-mcp-gateway \
  dcc-mcp-server ...
```

打开 `http://localhost:16686` 查看追踪数据。

### 快速启动：Grafana Tempo

```bash
# docker-compose.yml
services:
  tempo:
    image: grafana/tempo:latest
    ports: ["4317:4317", "3200:3200"]
```

```bash
OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317 dcc-mcp-server ...
```

### 快速启动：通过 OTLP Collector 写入 Phoenix

Phoenix 可在 `/v1/traces` 接收 OTLP/HTTP traces。当前 Rust 网关 exporter 使用 OTLP/gRPC，因此建议用 OpenTelemetry Collector 做 gRPC 到 HTTP 的桥接：

```yaml
# otel-collector.yaml
receivers:
  otlp:
    protocols:
      grpc:
        endpoint: 0.0.0.0:4317

exporters:
  otlphttp/phoenix:
    traces_endpoint: http://phoenix:6006/v1/traces

service:
  pipelines:
    traces:
      receivers: [otlp]
      exporters: [otlphttp/phoenix]
```

```bash
docker run -d --name phoenix -p 6006:6006 arizephoenix/phoenix:latest
docker run --rm -p 4317:4317 \
  -v "$PWD/otel-collector.yaml:/etc/otelcol-contrib/config.yaml" \
  otel/opentelemetry-collector-contrib:latest

OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317 \
OTEL_SERVICE_NAME=dcc-mcp-gateway \
  dcc-mcp-server ...
```

---

## 2. 网关竞争事件（#766）

网关选举、驱逐和探针事件可通过有界 MCP 资源获取。

### MCP 资源

```python
# 读取最近 N 条竞争事件
result = resources.read("resources://gateway/events")
```

返回 JSONL 格式（每行一个 JSON 对象）：

```json
{"timestamp":"2026-05-05T10:00:00Z","event":"election_won","dcc_type":"maya","instance_id":"a1b2c3d4","reason":null}
{"timestamp":"2026-05-05T10:01:00Z","event":"ghost_reaped","dcc_type":"blender","instance_id":"b2c3d4e5","reason":"pid_dead"}
```

### 事件类型

| 事件 | 含义 |
|------|------|
| `election_won` | 本实例成为活跃网关 |
| `voluntary_yield` | 本实例主动让位给更新的候选者 |
| `ghost_reaped` | 清理了一条过期注册记录 |
| `probe_booting` | 后端正在启动中 |
| `probe_unreachable` | 健康探针失败 |
| `auto_deregister` | 实例在干净关闭时自注销 |

环形缓冲区保留**最近 1000 条事件**。

---

## 3. Admin 调用审计与 Dispatch Traces

获选网关在 `GET /admin` 提供只读 HTML 仪表盘，并向运维与 AI agent 暴露机器可读 JSON 端点：

| 端点 | 使用场景 |
|------|----------|
| `GET /admin/api/calls` | 按 `request_id`、工具 slug、DCC 类型、实例、错误摘要和耗时关联最近调用。 |
| `GET /admin/api/traces?limit=200` | 查看最近 dispatch waterfall、有界输入 payload（16 KiB）和有界输出 payload（64 KiB）。 |
| `GET /admin/api/traces/{request_id}` | 不扫描整个 trace ring，直接下钻某一次调用。 |
| `GET /admin/api/workflows?limit=200` | 将 retained searches、describes、skill loads、calls、traces 和 audits 聚合为 agent session/workflow 链。 |
| `GET /admin/api/stats?range=1h\|24h\|7d` | 基于 trace log 计算成功率、延迟分位数和 top tools/instances。 |
| `GET /admin/api/governance?limit=300` / `GET /v1/debug/governance` | 查看当前 policy、read-only 状态、traffic capture guardrail、redaction paths、中间件 quota 状态，以及最近 allowed/denied/throttled 决策。 |
| `GET /admin/api/workers` | 查看 live registry 中每个实例的卡片。 |

默认情况下这些缓冲区只保存在内存中。设置 `DCC_MCP_GATEWAY_AUDIT_DIR` 后会追加有界 JSONL 文件：

- `audit.jsonl` —— 支撑 `/admin/api/calls` 的调用行。
- `traces.jsonl` —— 支撑 `/admin/api/traces` 和 stats 的 trace 行。

`DCC_MCP_GATEWAY_AUDIT_MAX_ROWS`（默认 `5000`）限制每个文件保留行数。网关重启时会用这些文件回填内存中的 admin 缓冲区。持久化的 trace payload 使用与实时 API 相同的有界/已脱敏 `TracePayload`，不会保存无界原始请求体。

`/admin/api/workflows` 和 `/v1/debug/workflows` 复用相同存储，按 session、显式 workflow id、trace id 或 request chain 展示 workflow 行；每行包含有界 agent metadata、selected search rank、zero-result search、time-to-first-success，并把步骤链接回 trace detail、debug bundle、issue report、OpenAPI 与 docs。

---

## 4. Prometheus 指标（#766）

在 `prometheus` Cargo feature 下，网关竞争计数器通过 `/metrics` 端点暴露：

```toml
dcc-mcp-gateway = { features = ["prometheus"] }
```

### 计数器

```
# 网关端口选举结果
dcc_mcp_gateway_elections_total{outcome="won"}     12
dcc_mcp_gateway_elections_total{outcome="yielded"}  3
dcc_mcp_gateway_elections_total{outcome="lost"}     1

# 注册表驱逐事件
dcc_mcp_gateway_evictions_total{reason="stale"}    1
dcc_mcp_gateway_evictions_total{reason="ghost"}    0
dcc_mcp_gateway_evictions_total{reason="probe_fail"} 2

# 后端就绪探针结果
dcc_mcp_gateway_probes_total{outcome="ready"}      45
dcc_mcp_gateway_probes_total{outcome="booting"}     3
dcc_mcp_gateway_probes_total{outcome="unreachable"} 2

# Gateway governance 结果
dcc_mcp_gateway_governance_events_total{category="policy",outcome="denied"} 4
dcc_mcp_gateway_governance_events_total{category="rate-limit",outcome="throttled"} 3
```

标签基数有界——不含自由形式的 `instance_id` 标签。

### Grafana 示例查询

```promql
# 选举速率随时间变化
rate(dcc_mcp_gateway_elections_total[5m])

# 驱逐速率
rate(dcc_mcp_gateway_evictions_total[5m])

# 探针成功率
rate(dcc_mcp_gateway_probes_total{outcome="ready"}[5m])
  / rate(dcc_mcp_gateway_probes_total[5m])
```

---

## 参见

- [telemetry.md](telemetry.md) — `ToolMetrics`、`ToolRecorder`、旧版 Python 遥测
- [gateway-diagnostics.md](gateway-diagnostics.md) — 选举/驱逐事件的日志模板
- [production-deployment.md](production-deployment.md) — 生产环境监控检查清单
- [middleware.md](middleware.md) — 用于每次调用审计日志的 `AuditMiddleware`
