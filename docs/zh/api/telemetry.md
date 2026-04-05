# Telemetry API

`dcc_mcp_core` (telemetry 模块)

OpenTelemetry 追踪、指标和结构化日志。

## 概述

为 DCC-MCP 生态系统提供可观测性：

- **追踪** — 操作执行的分布式追踪
- **指标** — 每个操作的耗时和成功率指标
- **日志** — 带 OpenTelemetry 集成的结构化日志
- **导出器** — 支持 OTLP gRPC（Jaeger、Grafana Tempo、Prometheus）

## TelemetryConfig

遥测提供者的配置。

### 构造函数

```python
from dcc_mcp_core import TelemetryConfig, ExporterBackend, LogFormat

config = TelemetryConfig.builder("my-dcc-service") \
    .with_exporter(ExporterBackend.CONSOLE) \
    .with_log_format(LogFormat.JSON) \
    .with_service_version("1.0.0") \
    .build()
```

### 导出器后端

| 后端 | 描述 |
|------|------|
| `NOOP` | 无操作导出器（用于测试） |
| `CONSOLE` | 打印到 stdout |
| `OTLP_GRPC` | 导出到 OTLP gRPC 端点 |

### 日志格式

| 格式 | 描述 |
|------|------|
| `JSON` | 结构化 JSON 日志 |
| `PRETTY` | 人类可读格式 |

## 初始化

```python
from dcc_mcp_core import init_telemetry, is_telemetry_initialized, shutdown_telemetry

# 启动时初始化
init_telemetry(config)

# 检查是否已初始化
if is_telemetry_initialized():
    print("遥测已激活")

# 退出时关闭
shutdown_telemetry()
```

## ActionRecorder

记录操作调用的指标。

### 构造函数

```python
from dcc_mcp_core import ActionRecorder

recorder = ActionRecorder("my-dcc-service")
```

### 记录操作

```python
# 开始记录
guard = recorder.start("create_sphere", "maya")

# ... 执行操作工作 ...

# 带成功或失败结束
guard.finish(success=True)
```

### 上下文管理器记录

```python
with recorder.record("list_objects", "blender") as metrics:
    # ... 执行工作 ...
    pass  # 自动以 success=True 结束

# 或显式结果
guard = recorder.record("delete_mesh", "houdini")
# ... 工作 ...
guard.finish(success=False, error="对象未找到")
```

### 查询指标

```python
metrics = recorder.metrics("create_sphere")
print(f"调用次数: {metrics.invocation_count}")
print(f"成功率: {metrics.success_rate():.2%}")
print(f"P50 延迟: {metrics.latency_p50_ms}ms")
print(f"P95 延迟: {metrics.latency_p95_ms}ms")
print(f"P99 延迟: {metrics.latency_p99_ms}ms")
```

### ActionMetrics

| 字段 | 类型 | 描述 |
|------|------|------|
| `action_name` | `str` | 操作名称 |
| `invocation_count` | `int` | 总调用次数 |
| `success_count` | `int` | 成功调用次数 |
| `failure_count` | `int` | 失败调用次数 |
| `success_rate()` | `float` | 成功比率 (0-1) |
| `latency_p50_ms` | `float` | 50 百分位延迟 |
| `latency_p95_ms` | `float` | 95 百分位延迟 |
| `latency_p99_ms` | `float` | 99 百分位延迟 |

## 追踪跨度

为详细追踪创建自定义跨度。

### Python 追踪

```python
from dcc_mcp_core import tracer, action_span

# 获取追踪器
t = tracer("my-component")

# 手动创建跨度
with t.start_as_current_span("my_operation") as span:
    span.set_attribute("key", "value")
    span.add_event("event_name", {"attr": "value"})
    # ... 工作 ...

# 使用 action_span 辅助函数
with action_span("create_sphere", dcc="maya") as span:
    span.set_attribute("radius", 1.0)
```

### 跨度属性

| 属性 | 类型 | 描述 |
|------|------|------|
| `dcc.name` | `str` | DCC 应用程序名称 |
| `dcc.version` | `str` | DCC 版本 |
| `action.name` | `str` | 操作名称 |
| `action.category` | `str` | 操作类别 |

## 错误处理

```python
from dcc_mcp_core import TelemetryError

try:
    init_telemetry(config)
except TelemetryError as e:
    print(f"遥测初始化失败: {e}")
```

## OTLP 导出

### OTLP gRPC 配置

```python
config = TelemetryConfig.builder("my-service") \
    .with_exporter(ExporterBackend.OTLP_GRPC) \
    .with_otlp_endpoint("http://localhost:4317") \
    .with_otlp_headers({"Authorization": "Bearer token"}) \
    .build()
```
