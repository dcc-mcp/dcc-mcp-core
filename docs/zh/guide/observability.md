# 可观测性

dcc-mcp-core 提供三种互补的可观测性接口，适用于生产环境部署。

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

## 3. Prometheus 指标（#766）

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
