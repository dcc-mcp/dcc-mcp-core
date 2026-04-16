# 遥测 API

`dcc_mcp_core` (telemetry 模块)

操作性能记录和可选的 OpenTelemetry 追踪/指标。

## 概述

提供：

- **操作记录** — 通过 `ToolRecorder` 记录每个操作的耗时和成功/失败计数
- **指标** — `ToolMetrics` 快照，包含延迟百分位数（p95/p99）和成功率
- **可选 OpenTelemetry** — stdout 导出器、JSON/文本日志、资源属性（可选）

## ToolRecorder

记录任何操作执行时间和结果。

### 构造函数

```python
from dcc_mcp_core import ToolRecorder

recorder = ToolRecorder("my-service")
```

### 方法

| 方法 | 返回 | 描述 |
|------|------|------|
| `start(action_name, dcc_name)` | `RecordingGuard` | 开始计时操作 |
| `metrics(action_name)` | `ToolMetrics \| None` | 获取操作指标 |
| `all_metrics()` | `list[ToolMetrics]` | 获取所有操作指标 |
| `reset()` | `None` | 重置所有统计数据 |

### 使用 Guard 记录

```python
guard = recorder.start("create_sphere", "maya")
# ... 执行工作 ...
guard.finish(success=True)
```

### 上下文管理器用法

```python
with recorder.start("create_sphere", "maya") as guard:
    # ... 执行工作 ...
# guard.finish(success=True) 在成功时自动调用
# guard.finish(success=False) 在异常时自动调用
```

## ToolMetrics

每个操作性能指标的只读快照。

### 属性

| 属性 | 类型 | 描述 |
|------|------|------|
| `action_name` | `str` | 操作名称 |
| `invocation_count` | `int` | 总调用次数 |
| `success_count` | `int` | 成功调用次数 |
| `failure_count` | `int` | 失败调用次数 |
| `avg_duration_ms` | `float` | 平均耗时（毫秒） |
| `p95_duration_ms` | `float` | P95 耗时 |
| `p99_duration_ms` | `float` | P99 耗时 |

### 方法

| 方法 | 返回 | 描述 |
|------|------|------|
| `success_rate()` | `float` | 成功率 (0.0-1.0) |

### 示例

```python
metrics = recorder.metrics("create_sphere")
if metrics:
    print(f"调用次数: {metrics.invocation_count}")
    print(f"成功率: {metrics.success_rate():.2%}")
    print(f"P95: {metrics.p95_duration_ms:.2f}ms")
```

## RecordingGuard

`ToolRecorder.start()` 返回的 RAII guard。

### 方法

| 方法 | 返回 | 描述 |
|------|------|------|
| `finish(success)` | `None` | 以成功标志结束记录 |
| `__enter__()` | `RecordingGuard` | 上下文管理器入口 |
| `__exit__()` | `None` | 上下文管理器出口（在异常时设置 success=False） |

## TelemetryConfig

可选的 OpenTelemetry 配置。

### 构造函数

```python
from dcc_mcp_core import TelemetryConfig

cfg = TelemetryConfig("my-dcc-service")
```

### 方法

| 方法 | 返回 | 描述 |
|------|------|------|
| `with_stdout_exporter()` | `TelemetryConfig` | 使用 stdout 导出器 |
| `with_noop_exporter()` | `TelemetryConfig` | 使用 no-op 导出器（测试） |
| `with_json_logs()` | `TelemetryConfig` | 使用 JSON 日志格式 |
| `with_text_logs()` | `TelemetryConfig` | 使用文本日志格式 |
| `with_attribute(key, value)` | `TelemetryConfig` | 添加资源属性 |
| `with_service_version(version)` | `TelemetryConfig` | 设置服务版本 |
| `set_enable_metrics(enabled)` | `TelemetryConfig` | 启用/禁用指标 |
| `set_enable_tracing(enabled)` | `TelemetryConfig` | 启用/禁用追踪 |
| `init()` | `None` | 安装为全局提供者 |

### 示例

```python
cfg = TelemetryConfig("my-dcc-service")
cfg.with_stdout_exporter()
cfg.with_json_logs()
cfg.with_service_version("1.0.0")
cfg.init()
```

## is_telemetry_initialized()

检查全局遥测提供者是否已安装。

```python
from dcc_mcp_core import is_telemetry_initialized

if is_telemetry_initialized():
    print("遥测已激活")
```

## 集成示例

### Maya 集成

```python
from dcc_mcp_core import ToolRecorder
import maya.cmds as cmds

recorder = ToolRecorder("maya")

def traced_create_sphere(radius=1.0, name=None):
    with recorder.start("create_sphere", "maya") as guard:
        sphere = cmds.polySphere(r=radius, n=name)[0]
        guard.finish(success=True)
        return sphere
```
