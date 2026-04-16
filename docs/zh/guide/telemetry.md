# 遥测指南

操作性能记录和可选的 OpenTelemetry 追踪/指标。

## 概述

提供：

- **操作记录** — 通过 `ToolRecorder` 记录每个操作的耗时和成功/失败计数
- **指标** — `ToolMetrics` 快照，包含延迟百分位数（p95/p99）和成功率
- **可选 OpenTelemetry** — stdout 导出器、JSON/文本日志、资源属性（可选）

## ToolRecorder

记录任何操作执行时间和结果。

### 快速开始

```python
from dcc_mcp_core import ToolRecorder

recorder = ToolRecorder("my-service")

# 使用 guard 记录
guard = recorder.start("create_sphere", "maya")
# ... 执行工作 ...
guard.finish(success=True)
```

### Guard 模式（推荐）

`start()` 返回的 guard 是一个 RAII 上下文管理器：

```python
# 除非抛出异常，否则自动 success=True
with recorder.start("create_sphere", "maya") as guard:
    # ... 执行工作 ...
# guard.finish(success=True) 自动调用
```

需要显式控制时的手动结束：

```python
guard = recorder.start("delete_mesh", "houdini")
try:
    delete_mesh("Cube")
    guard.finish(success=True)
except Exception:
    guard.finish(success=False)
    raise
```

### 查询指标

```python
# 获取特定操作的指标
metrics = recorder.metrics("create_sphere")
if metrics:
    print(f"操作: {metrics.action_name}")
    print(f"调用次数: {metrics.invocation_count}")
    print(f"成功率: {metrics.success_rate():.2%}")
    print(f"平均耗时: {metrics.avg_duration_ms:.2f}ms")
    print(f"P95: {metrics.p95_duration_ms:.2f}ms")
    print(f"P99: {metrics.p99_duration_ms:.2f}ms")
```

### 所有指标

```python
# 获取所有已记录操作的指标
all_metrics = recorder.all_metrics()

for m in all_metrics:
    print(f"{m.action_name}: {m.invocation_count} 次调用, {m.success_rate():.1%} 成功")
```

### 重置

```python
recorder.reset()  # 清除所有内存中统计数据
```

## TelemetryConfig

可选的 OpenTelemetry 配置。单独使用 `ToolRecorder` 时不需要。

### 基础设置

```python
from dcc_mcp_core import TelemetryConfig

cfg = TelemetryConfig("my-dcc-service")
cfg.init()
```

### 控制台导出器

```python
cfg = TelemetryConfig("my-dcc-service")
cfg.with_stdout_exporter()
cfg.init()
```

### JSON 日志（生产）

```python
cfg = TelemetryConfig("my-dcc-service")
cfg.with_stdout_exporter()
cfg.with_json_logs()
cfg.with_service_version("1.0.0")
cfg.init()
```

### 文本日志（默认）

```python
cfg = TelemetryConfig("my-dcc-service")
cfg.with_stdout_exporter()
cfg.with_text_logs()
cfg.init()
```

### No-op 导出器（测试）

```python
cfg = TelemetryConfig("my-dcc-service")
cfg.with_noop_exporter()
cfg.init()
```

### 自定义属性

```python
cfg = TelemetryConfig("my-dcc-service")
cfg.with_stdout_exporter()
cfg.with_attribute("dcc.name", "maya")
cfg.with_attribute("dcc.version", "2025")
cfg.init()
```

### 启用/禁用功能

```python
cfg = TelemetryConfig("my-dcc-service")
cfg.with_stdout_exporter()
cfg.set_enable_metrics(False)   # 禁用指标收集
cfg.set_enable_tracing(False)  # 禁用分布式追踪
cfg.init()
```

### 检查初始化

```python
from dcc_mcp_core import is_telemetry_initialized

if is_telemetry_initialized():
    print("遥测已激活")
```

## Maya 集成

```python
from dcc_mcp_core import ToolRecorder
import maya.cmds as cmds

recorder = ToolRecorder("maya")

def traced_create_sphere(radius=1.0, name=None):
    with recorder.start("create_sphere", "maya") as guard:
        sphere = cmds.polySphere(r=radius, n=name)[0]
        guard.finish(success=True)
        return sphere

def traced_delete(object_name):
    with recorder.start("delete_object", "maya") as guard:
        cmds.delete(object_name)
        guard.finish(success=True)
```

## Blender 集成

```python
from dcc_mcp_core import ToolRecorder
import bpy

recorder = ToolRecorder("blender")

def traced_blender_operation(operation_name, func):
    with recorder.start(operation_name, "blender") as guard:
        result = func()
        guard.finish(success=True)
        return result
```

## 多 DCC 追踪

```python
# 每个 DCC 一个记录器
maya_recorder = ToolRecorder("maya")
blender_recorder = ToolRecorder("blender")
houdini_recorder = ToolRecorder("houdini")

# 每个维护独立的指标
```

## 最佳实践

### 1. 使用上下文管理器

```python
with recorder.start("my_action", "maya") as guard:
    perform_action()
# 成功/失败自动记录
```

### 2. 显式捕获异常

```python
with recorder.start("risky_action", "maya") as guard:
    try:
        risky_operation()
        guard.finish(success=True)
    except Exception:
        guard.finish(success=False)
        raise
```

### 3. 批量操作后查询指标

```python
# 运行多个操作
for i in range(100):
    with recorder.start("batch_op", "maya"):
        do_work(i)

# 检查聚合指标
metrics = recorder.metrics("batch_op")
if metrics:
    print(f"P99 延迟: {metrics.p99_duration_ms}ms")
```
