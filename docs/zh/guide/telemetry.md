# 遥测指南

OpenTelemetry 追踪、指标和结构化日志。

## 概述

为 DCC-MCP 生态系统提供可观测性：

- **追踪** — 操作执行的分布式追踪
- **指标** — 每个操作的耗时和成功率指标
- **日志** — 带 OpenTelemetry 集成的结构化日志
- **导出器** — 支持 OTLP gRPC（Jaeger、Grafana Tempo、Prometheus）

## 快速开始

### 基础初始化

```python
from dcc_mcp_core import TelemetryConfig, init_telemetry, is_telemetry_initialized

# 初始化遥测
config = TelemetryConfig.builder("my-dcc-service") \
    .with_exporter("console") \
    .build()

init_telemetry(config)

print(f"已初始化: {is_telemetry_initialized()}")
```

### 记录操作

```python
from dcc_mcp_core import ActionRecorder

recorder = ActionRecorder("my-dcc-service")

# 记录一个操作
guard = recorder.start("create_sphere", "maya")
# ... 执行 Maya 操作 ...
guard.finish(success=True)
```

### 上下文管理器模式

```python
with recorder.record("list_objects", "blender") as metrics:
    # 操作自动执行
    # 上下文退出时记录指标
    pass

# 访问指标
print(f"成功率: {metrics.success_rate():.2%}")
```

## 配置

### 控制台导出器（开发）

```python
config = TelemetryConfig.builder("my-dcc-service") \
    .with_exporter("console") \
    .with_log_format("pretty") \
    .build()
```

### JSON 日志（生产）

```python
config = TelemetryConfig.builder("my-dcc-service") \
    .with_exporter("console") \
    .with_log_format("json") \
    .with_service_version("1.0.0") \
    .build()
```

### OTLP 导出（企业）

```python
config = TelemetryConfig.builder("my-dcc-service") \
    .with_exporter("otlp_grpc") \
    .with_otlp_endpoint("http://jaeger:4317") \
    .with_otlp_headers({"Authorization": "Bearer token"}) \
    .with_service_version("1.0.0") \
    .build()
```

## 操作指标

### 使用 Guard 记录

```python
recorder = ActionRecorder("maya-service")

# 开始记录
guard = recorder.start("create_sphere", "maya")

# ... 执行操作 ...

# 显式成功/失败结束
guard.finish(success=True)
```

### 自动记录

```python
with recorder.record("delete_mesh", "houdini") as metrics:
    # 执行操作
    delete_mesh("Cube")

# 无异常: success=True
# 有异常: success=False 自动
```

### 查询指标

```python
# 获取特定操作的指标
metrics = recorder.metrics("create_sphere")

print(f"操作: {metrics.action_name}")
print(f"调用次数: {metrics.invocation_count}")
print(f"成功率: {metrics.success_rate():.2%}")
print(f"P50 延迟: {metrics.latency_p50_ms}ms")
print(f"P95 延迟: {metrics.latency_p95_ms}ms")
print(f"P99 延迟: {metrics.latency_p99_ms}ms")
```

## 追踪

### 操作跨度

```python
from dcc_mcp_core import action_span

# 为操作创建跨度
with action_span("create_sphere", dcc="maya") as span:
    span.set_attribute("radius", 1.0)
    span.set_attribute("segments", 32)

    # ... 执行操作 ...

    span.add_event("completed", {"sphere_name": "sphere1"})
```

### 自定义跨度

```python
from dcc_mcp_core import tracer

# 获取组件的追踪器
t = tracer("my-component")

# 创建跨度
with t.start_as_current_span("my_operation") as span:
    span.set_attribute("key", "value")
    span.add_event("event_name", {"attr": "value"})

    # ... 工作 ...

    if error:
        span.record_exception(error)
```

## 集成示例

### Maya 集成

```python
from dcc_mcp_core import init_telemetry, ActionRecorder
import maya.cmds as cmds

# Maya 启动时初始化
init_telemetry(config)
recorder = ActionRecorder("maya")

def traced_create_sphere(radius=1.0, name=None):
    with recorder.record("create_sphere", "maya") as metrics:
        sphere = cmds.polySphere(r=radius, n=name)[0]
        return sphere
```

### Blender 集成

```python
import bpy
from dcc_mcp_core import init_telemetry, ActionRecorder

init_telemetry(config)
recorder = ActionRecorder("blender")

def traced_blender_operation(operation_name, func):
    with recorder.record(operation_name, "blender"):
        return func()

# 使用
result = traced_blender_operation("create_cube", lambda: bpy.ops.mesh.primitive_cube_add())
```

## 最佳实践

### 1. 尽早初始化

```python
# 应用程序启动时
def initialize_dcc_mcp():
    config = TelemetryConfig.builder("my-dcc") \
        .with_exporter("console") \
        .build()
    init_telemetry(config)
```

### 2. 使用上下文管理器

```python
# 自动清理和错误处理
with recorder.record("my_action", "maya") as metrics:
    perform_action()
# 成功/失败自动记录
```

### 3. 添加有意义的属性

```python
with action_span("create_sphere", dcc="maya") as span:
    span.set_attribute("radius", radius)
    span.set_attribute("segments", segments)
    span.set_attribute("user", current_user)
```

### 4. 优雅关闭

```python
import atexit

def cleanup():
    shutdown_telemetry()

atexit.register(cleanup)
```
