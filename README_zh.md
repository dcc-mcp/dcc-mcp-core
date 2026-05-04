# dcc-mcp-core

[![PyPI](https://img.shields.io/pypi/v/dcc-mcp-core)](https://pypi.org/project/dcc-mcp-core/)
[![Python](https://img.shields.io/pypi/pyversions/dcc-mcp-core)](https://www.python.org/)
[![License](https://img.shields.io/badge/License-MIT-green.svg)](https://opensource.org/licenses/MIT)
[![Downloads](https://static.pepy.tech/badge/dcc-mcp-core)](https://pepy.tech/project/dcc-mcp-core)
[![Coverage](https://img.shields.io/codecov/c/github/loonghao/dcc-mcp-core)](https://codecov.io/gh/loonghao/dcc-mcp-core)
[![Tests](https://img.shields.io/github/actions/workflow/status/loonghao/dcc-mcp-core/ci.yml?branch=main&label=Tests)](https://github.com/loonghao/dcc-mcp-core/actions)
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](http://makeapullrequest.com)
[![Latest Version](https://img.shields.io/github/v/tag/loonghao/dcc-mcp-core?label=Latest%20Version)](https://github.com/loonghao/dcc-mcp-core/releases)

[English](README.md) | 中文

**面向 AI 辅助 DCC 工作流的生产级基础库** —— 结合 **模型上下文协议（MCP 2025-03-26 Streamable HTTP）** 与 **零代码 Skills 系统**，后者遵循 [agentskills.io 1.0](https://agentskills.io/specification) 规范。**Rust 核心 + PyO3 Python 绑定**，交付企业级性能、安全性与可扩展性；**运行时零 Python 依赖**。支持 Python 3.7–3.13。

> **注意**：本项目处于积极开发中（v0.14+），API 可能演进；版本历史参见 `CHANGELOG.md`。

---

## 问题与解决方案

### 为什么不直接用 CLI？

**CLI 工具对 DCC 状态一无所知。** 它们无法看到当前场景、选中对象或视口内容，只能在隔离环境中执行，迫使 AI：

- 多次往返才能收集上下文
- 从 CLI 输出重建状态（脆弱、缓慢）
- 缺乏来自视口的视觉反馈
- 随请求增长上下文爆炸严重

### 为什么选 MCP（模型上下文协议）？

**MCP 是 AI 原生的**，但标准 MCP 在 DCC 自动化上缺两项关键能力：

1. **上下文爆炸** —— MCP 没有把工具限定到具体会话或实例的机制，多 DCC 场景下请求膨胀。
2. **无生命周期控制** —— 无法发现实例状态（活跃场景、文档、进程健康）或控制启动/关闭。

### 我们的方案：MCP + Skills 系统

我们**复用并扩展**现有 MCP 生态，新增：

| 能力 | 收益 |
|---|---|
| **网关选举与版本感知** | 多实例负载均衡；新 DCC 启动时自动接管 |
| **会话隔离** | 每个 AI 会话对接自己的 DCC 实例；避免上下文串扰 |
| **Skills 系统（零代码）** | 用 `SKILL.md` + 同级 YAML / 脚本定义工具，无需 Python 胶水 |
| **渐进式发现** | 按 DCC 类型、实例、场景、产品过滤工具；防止上下文爆炸 |
| **实例追踪** | 了解活跃文档、PID、显示名称；实现智能路由 |
| **结构化结果** | 每个工具返回 `(success, message, context, prompt)` 便于 AI 推理 |
| **Workflow 原语** | 声明式多步工作流：重试 / 超时 / 幂等键 / 审批闸门 |
| **Artefact 交接** | 基于内容寻址（SHA-256）在工具和工作流步骤间传递文件 |
| **Job 生命周期 + SSE** | `tools/call` 可选异步派发，`$/dcc.jobUpdated` 通知，SQLite 持久化 |

这不是重新发明 MCP —— 而是**解决 MCP 在桌面自动化中的盲点**。

---

## 为什么选 dcc-mcp-core？

| 方面 | dcc-mcp-core | 通用 MCP | CLI 工具 | 浏览器扩展 |
|---|---|---|---|---|
| **DCC 状态感知** | 场景、文档、实例 ID | 否 | 否 | 部分 |
| **多实例支持** | 网关选举 + 会话隔离 | 单端点 | 否 | 否 |
| **上下文限定** | 按 DCC / 场景 / 产品 | 全局工具 | 否 | 有限 |
| **零代码工具** | `SKILL.md` + 同级文件 | 需要完整 Python | 仅脚本 | 否 |
| **性能** | Rust + 零拷贝 + IPC | Python 开销 | 进程开销 | 网络开销 |
| **安全性** | 沙箱 + 审计日志 | 手动 | 手动 | 无 |
| **跨平台** | Windows / macOS / Linux | 是 | 有限 | 仅浏览器 |

AI 友好文档：[AGENTS.md](AGENTS.md) · [`docs/guide/agents-reference.md`](docs/guide/agents-reference.md) · [`.agents/skills/dcc-mcp-core/SKILL.md`](.agents/skills/dcc-mcp-core/SKILL.md)

---

## 架构：三层栈

```
+-----------------------------------------------------------------+
|  AI Agent (Claude, GPT, 等)                                     |
|  通过 MCP 协议调用工具 (tools/list, tools/call)                 |
+-------------------------------+---------------------------------+
                                |
                        MCP Streamable HTTP
                                |
+-------------------------------v---------------------------------+
|  Gateway Server (Rust / HTTP)                                   |
|  +-- 版本感知实例选举                                            |
|  +-- 会话隔离与路由                                              |
|  +-- 工具发现 (从 Skills 派生)                                   |
|  +-- Job 生命周期 + SSE 通知                                     |
|  +-- 工作流执行引擎                                              |
+-------------------------------+---------------------------------+
                                |
                 IPC (Named Pipe / Unix Socket) via DccLink
                                |
          +---------------------+---------------------+
          |                     |                     |
  +-------v-------+     +-------v-------+     +-------v-------+
  |  Maya 适配器   |     | Blender 适配器 |     | Houdini 适配器|
  |  (_core.pyd)   |     |  (_core.so)    |     |  (_core.so)   |
  +-------+--------+     +-------+--------+     +-------+-------+
          |                      |                      |
    Python 3.7+             Python 3.7+            Python 3.7+
    (零依赖)                (零依赖)                (零依赖)
```

- **第一层 —— AI Agent**：通过标准 MCP 协议（`tools/list` / `tools/call` / notifications）调用工具。
- **第二层 —— 网关**：编排工具发现、会话隔离、请求路由、Job 生命周期与工作流执行；维护 `__gateway__` sentinel 做版本感知选举。
- **第三层 —— DCC 适配器**：DCC 侧的 Python 包（Maya / Blender / Photoshop / Houdini…）内嵌 `_core` 原生扩展和 Skills 系统。WebView 宿主适配器（AuroraView、Electron 面板）和 WebSocket 桥接器（Photoshop、ZBrush）使用更窄的能力面。

---

## 快速开始

### 安装

```bash
# 从 PyPI 安装（Python 3.7+ 预构建 wheel）
pip install dcc-mcp-core

# 从源码构建（需要 Rust 1.95+）
git clone https://github.com/loonghao/dcc-mcp-core.git
cd dcc-mcp-core
vx just dev           # 推荐 —— 使用项目标准 feature 集合
# 或：pip install -e .
```

### 将 DCC 暴露为 MCP 服务 —— Skills-First（推荐）

`create_skill_server` 提供完整的 Skills-First 入口：`tools/list` 返回六个核心工具加每个未加载 skill 的 stub，Agent 通过 `search_skills` → `load_skill` 激活实际需要的工具，让上下文窗口保持精简。

```python
from dcc_mcp_core import create_skill_server, McpHttpConfig

server = create_skill_server(
    "maya",
    McpHttpConfig(port=8765),
)
handle = server.start()
print(handle.mcp_url())   # "http://127.0.0.1:8765/mcp"
# ... 其他逻辑 ...
handle.shutdown()
```

### 底层 API：手动注册工具

```python
import json
from dcc_mcp_core import (
    ToolRegistry, ToolDispatcher, EventBus,
    McpHttpServer, McpHttpConfig,
    success_result, scan_and_load,
)

skills, skipped = scan_and_load(dcc_name="maya")
print(f"加载 {len(skills)} 个 skills，跳过 {len(skipped)} 个")

registry = ToolRegistry()
registry.register(
    name="get_scene",
    description="返回当前 Maya 场景路径",
    category="scene",
    dcc="maya",
    version="1.0.0",
)

dispatcher = ToolDispatcher(registry)
dispatcher.register_handler(
    "get_scene",
    lambda params: success_result("OK", path="/proj/shots/sh010.ma").to_dict(),
)

# 可选：观察生命周期事件
bus = EventBus()
bus.subscribe("action.after_execute", lambda **kw: print(f"完成：{kw['action_name']}"))

result = dispatcher.dispatch("get_scene", json.dumps({}))
print(result["output"])   # {"success": True, "message": "OK", "context": {"path": ...}}

# 通过 MCP 暴露注册表（必须在 .start() 之前注册所有 handler）
server = McpHttpServer(registry, McpHttpConfig(port=8765))
handle = server.start()
```

---

## 核心概念

### ToolResult —— 面向 AI 的结构化结果

所有 skill 执行结果都使用 `ToolResult`，它专为 AI 友好而设计，带结构化上下文和后续建议。

```python
from dcc_mcp_core import ToolResult, success_result, error_result

# 工厂函数（推荐）。额外 kwargs 放入 context。
ok = success_result(
    "球体已创建",
    prompt="接下来可以添加材质或调整 UV",
    object_name="sphere1",
    position=[0, 1, 0],
)
# ok.context == {"object_name": "sphere1", "position": [0, 1, 0]}

err = error_result(
    "创建球体失败",
    "半径必须为正数",
)

# 直接构造
result = ToolResult(
    success=True,
    message="操作完成",
    context={"key": "value"},
)

result.success   # bool
result.message   # str
result.prompt    # Optional[str] —— AI 下一步建议
result.error     # Optional[str] —— 错误详情
result.context   # dict —— 任意结构化数据
result.to_json() # JSON 安全序列化
```

### ToolRegistry 与 Dispatcher

```python
import json
from dcc_mcp_core import ToolRegistry, ToolDispatcher, EventBus

registry = ToolRegistry()
registry.register(name="my_tool", description="我的工具", category="tools", version="1.0.0")

dispatcher = ToolDispatcher(registry)
dispatcher.register_handler("my_tool", lambda params: {"done": True})

result = dispatcher.dispatch("my_tool", json.dumps({}))
# result == {"action": "my_tool", "output": {"done": True}, "validation_skipped": True}

bus = EventBus()
sub_id = bus.subscribe("action.before_execute", lambda **kw: print(f"before: {kw}"))
bus.publish("action.before_execute", action_name="test")
bus.unsubscribe("action.before_execute", sub_id)
```

---

## Skills 系统 —— 零代码 MCP 工具注册

Skills 系统允许你把任何脚本（Python、MEL、MaxScript、Batch、Shell、PowerShell、JavaScript、TypeScript）注册为 MCP 工具，**完全不写 Python 胶水代码**。对齐 [agentskills.io 1.0](https://agentskills.io/specification) 规范。

### 架构规则 —— 同级文件模式（v0.15+）

dcc-mcp-core 的每一项扩展（`tools`、`groups`、`workflows`、`prompts`、`next-tools` 等）都以 `metadata.dcc-mcp.<feature>` 键指向**同级文件**。`SKILL.md` frontmatter 本身只保留六个标准 agentskills.io 字段（`name`、`description`、`license`、`compatibility`、`metadata`、`allowed-tools`）。

```
my-automation/
├── SKILL.md                      # frontmatter + 人类可读正文
├── tools.yaml                    # 工具定义 + annotations + groups
├── workflows/
│   └── vendor_intake.workflow.yaml
├── prompts/
│   └── review_scene.prompt.yaml
└── scripts/
    ├── cleanup.py
    └── publish.sh
```

### 五分钟上手你的第一个 Skill

**1. 创建 `maya-cleanup/SKILL.md`：**

```yaml
---
name: maya-cleanup
description: >-
  Domain skill —— Maya 场景优化与清理工具。
  不用于新建几何体 —— 请改用 maya-geometry。
license: MIT
compatibility: "Maya 2024+, Python 3.7+"
metadata:
  dcc-mcp:
    layer: domain
    dcc: maya
    tools: tools.yaml
    search-hint: "cleanup, optimise, unused nodes"
    depends: [dcc-diagnostics]
---
# Maya 场景清理

自动化工具用于优化和校验 Maya 场景。
```

**2. 创建 `maya-cleanup/tools.yaml`：**

```yaml
tools:
  - name: cleanup
    description: "清理活跃场景中的未使用节点。"
    script: scripts/cleanup.py
    annotations:
      read_only_hint: false
      destructive_hint: true
      idempotent_hint: true
    next-tools:
      on-success: [maya_cleanup__validate]
      on-failure: [dcc_diagnostics__screenshot, dcc_diagnostics__audit_log]

  - name: validate
    description: "清理后校验场景完整性。"
    script: scripts/validate.mel
    annotations:
      read_only_hint: true
```

**3. 创建 `maya-cleanup/scripts/cleanup.py`：**

```python
#!/usr/bin/env python
"""清理场景中的未使用节点。"""
from __future__ import annotations

import json
import sys


def main() -> int:
    result = {"success": True, "message": "清理了 42 个未使用节点"}
    print(json.dumps(result))
    return 0


if __name__ == "__main__":
    sys.exit(main())
```

**4. 注册并调用：**

```python
import os
os.environ["DCC_MCP_SKILL_PATHS"] = "/path/to/maya-cleanup/.."

from dcc_mcp_core import create_skill_server, McpHttpConfig

server = create_skill_server("maya", McpHttpConfig(port=8765))
handle = server.start()
# Agent 调用 search_skills("cleanup") → load_skill("maya-cleanup") → maya_cleanup__cleanup
```

就这么简单 —— 零 Python 胶水代码，只要 `SKILL.md` + `tools.yaml` + 脚本。

### 支持的脚本类型

| 扩展名 | 类型 | 执行方式 |
|---|---|---|
| `.py` | Python | 通过系统 Python `subprocess` |
| `.mel` | MEL (Maya) | 通过 DCC 适配器 |
| `.ms` | MaxScript | 通过 DCC 适配器 |
| `.bat`, `.cmd` | Batch | `cmd /c` |
| `.sh`, `.bash` | Shell | `bash` |
| `.ps1` | PowerShell | `powershell -File` |
| `.js`, `.jsx` | JavaScript | `node` |
| `.ts` | TypeScript | `node`（通过 ts-node 或 tsx） |

完整示例请看 [`examples/skills/`](examples/skills/)。

### 内置 Skills —— 零配置开箱即用

`dcc-mcp-core` wheel 内置 **两个核心 skills**，`pip install dcc-mcp-core` 后立即可用，无需克隆仓库或配置 `DCC_MCP_SKILL_PATHS`。

| Skill | 工具 | 用途 |
|---|---|---|
| `dcc-diagnostics` | `screenshot`、`audit_log`、`tool_metrics`、`process_status` | 任意 DCC 的可观测性与调试 |
| `workflow` | `run_chain` | 多步 action 串联，上下文透传 |

```python
from dcc_mcp_core import get_bundled_skills_dir, get_bundled_skill_paths

print(get_bundled_skills_dir())
# /path/to/site-packages/dcc_mcp_core/skills

paths = get_bundled_skill_paths()                       # 默认开启
paths = get_bundled_skill_paths(include_bundled=False)  # 显式关闭
```

DCC 适配器（如 `dcc-mcp-maya`）默认包含内置 skills。关闭：`start_server(include_bundled=False)`。

---

## 解决 MCP 上下文爆炸

**问题**：标准 MCP 在 `tools/list` 里返回*所有*工具，连跟当前任务/实例无关的也一起。3 个 DCC 实例 × 50 个 skills × 5 个脚本 = **750 个工具**，上下文窗口瞬间爆满。

**渐进式发现** —— dcc-mcp-core 把这个数字压到 Agent 实际需要的量级：

1. **Skill stubs** —— `tools/list` 只返回六个元工具 + 每个未加载 skill 一个 stub（`__skill__<name>`）。Agent 调用 `search_skills(query)` → `load_skill(name)` 才激活真正的工具。
2. **实例感知** —— 每个 DCC 注册活跃文档、PID、显示名、作用域。
3. **工具作用域过滤** —— 按 DCC 类型、信任作用域（Repo < User < System < Admin）、产品白名单和策略过滤。
4. **会话隔离** —— AI 会话固定到一个 DCC 实例，只看到该实例的工具。
5. **网关选举** —— 新版本 DCC 启动时自动接管流量。

标准 MCP：

```
tools/list 响应：
  100 Maya + 100 Houdini + 100 Blender + 250 共享 = 550 个工具定义
```

dcc-mcp-core (Skills-First)：

```
tools/list 响应（Maya 会话、尚未加载任何 skill）：
  6 个核心工具 + 22 个 skill stub = 28 条
→ Agent 只加载需要的 3 个 skill → 上下文约 ~30 个工具
```

---

## 能力亮点

- **Rust 驱动性能** —— 零拷贝序列化（`rmp-serde`）、LZ4 共享内存、无锁数据结构。
- **运行时零 Python 依赖** —— 一切编译进原生扩展。
- **Skills-First MCP 服务器** —— `create_skill_server()` 提供开箱即用的 MCP 2025-03-26 Streamable HTTP 端点，内置渐进式发现。
- **Workflow 原语** —— `WorkflowSpec` / `WorkflowExecutor`：声明式多步工作流，支持重试、超时、幂等键、审批闸门、foreach / parallel / branch 步骤、SQLite 恢复。
- **调度器** —— Cron + Webhook（HMAC-SHA256）触发的工作流，通过同级 `schedules.yaml`（可选 feature）。
- **Artefact 交接** —— 基于内容寻址（SHA-256）的 `FileRef` + `ArtefactStore`，在工具和工作流步骤间传递文件。
- **Job 生命周期与通知** —— 可选异步 `tools/call`、SSE 通道（`notifications/progress`、`$/dcc.jobUpdated`、`$/dcc.workflowUpdated`）、可选 SQLite 持久化（重启保活）。
- **Resources / Prompts 原语** —— 暴露 DCC 实时状态（`scene://current`、`capture://current_window`、`audit://recent`、`artefact://sha256/<hex>`）与同级 YAML 里的可复用 prompt 模板。
- **线程亲和** —— `DeferredExecutor` 安全地把主线程工具派发到 DCC 事件循环；其余由 Tokio 工作线程处理。
- **网关与多实例** —— 版本感知先到先得选举、会话间 SSE 多路复用、异步派发 + wait-for-terminal 透传。
- **鲁棒 IPC** —— 基于 `ipckit` 的 DccLink 帧协议（Named Pipe / Unix Socket）：`IpcChannelAdapter`、`GracefulIpcChannelAdapter`、`SocketServerAdapter`。
- **进程管理** —— 启动、监控、自动恢复 DCC 进程。
- **沙箱安全** —— 基于策略的访问控制 + 审计日志、`ToolAnnotations` 安全提示、`ToolValidator` schema 校验。
- **屏幕捕获** —— 全屏或单窗口（HWND `PrintWindow`）视口捕获，供 AI 视觉反馈。
- **USD 集成** —— Universal Scene Description 读写桥。
- **结构化遥测** —— 追踪、录制，可选 Prometheus `/metrics` 导出器。
- **~180 个公开 Python 符号** —— 通过顶层重导出提供；`_core.pyi` 是 stub-gen/dev 构建后的生成产物，不是手写源码。

---

## 架构总览 —— 30 个 Workspace 成员

`dcc-mcp-core` 组织为 **30 个成员的 Rust workspace**（29 个功能 crate + `workspace-hack`），通过 PyO3 / maturin 编译为单个原生 Python 扩展（`_core`）。精选 crate：

| Crate | 职责 | 关键类型 |
|---|---|---|
| `dcc-mcp-naming` | SEP-986 命名校验 | `validate_tool_name`、`validate_action_id`、`TOOL_NAME_RE` |
| `dcc-mcp-models` | 数据模型 | `ToolResult`、`SkillMetadata`、`ToolDeclaration` |
| `dcc-mcp-actions` | 工具执行生命周期 | `ToolRegistry`、`ToolDispatcher`、`ToolValidator`、`ToolPipeline`、`EventBus` |
| `dcc-mcp-skills` | Skills 发现与加载 | `SkillScanner`、`SkillCatalog`、`SkillWatcher`、依赖解析器 |
| `dcc-mcp-protocols` | MCP 协议类型 | `ToolDefinition`、`ResourceDefinition`、`PromptDefinition`、`ToolAnnotations`、`BridgeKind` |
| `dcc-mcp-transport` | IPC 通信 | `DccLinkFrame`、`IpcChannelAdapter`、`GracefulIpcChannelAdapter`、`SocketServerAdapter`、`FileRegistry` |
| `dcc-mcp-process` | 进程管理 | `PyDccLauncher`、`PyProcessMonitor`、`PyProcessWatcher`、`PyCrashRecoveryPolicy`、`HostDispatcher` |
| `dcc-mcp-sandbox` | 安全 | `SandboxPolicy`、`SandboxContext`、`InputValidator`、`AuditLog` |
| `dcc-mcp-shm` | 共享内存 | `PySharedBuffer`、`PySharedSceneBuffer`、LZ4 压缩 |
| `dcc-mcp-capture` | 屏幕捕获 | `Capturer`、`WindowFinder`、HWND / DXGI / X11 / Mock 后端 |
| `dcc-mcp-telemetry` | 可观测性 | `TelemetryConfig`、`ToolRecorder`、`ToolMetrics`、可选 Prometheus |
| `dcc-mcp-usd` | USD 集成 | `UsdStage`、`UsdPrim`、`scene_info_json_to_stage` |
| `dcc-mcp-http` | MCP Streamable HTTP 服务器 | `McpHttpServer`、`McpHttpConfig`、`McpServerHandle`、网关、job manager |
| `dcc-mcp-server` | 二进制入口 | `dcc-mcp-server` CLI、网关 runner |
| `dcc-mcp-workflow` | 工作流引擎（可选） | `WorkflowSpec`、`WorkflowExecutor`、`WorkflowHost`、`StepPolicy`、`RetryPolicy` |
| `dcc-mcp-scheduler` | Cron + Webhook 调度器（可选） | `ScheduleSpec`、`TriggerSpec`、`SchedulerService`、HMAC 校验 |
| `dcc-mcp-artefact` | 内容寻址 artefact 存储 | `FileRef`、`FilesystemArtefactStore`、`InMemoryArtefactStore` |
| `dcc-mcp-logging` | 滚动文件日志 | `FileLoggingConfig`、日志保留辅助函数 |
| `dcc-mcp-paths` | 平台路径辅助 | cache/config/data 目录辅助函数 |
| `dcc-mcp-pybridge` | PyO3 桥接辅助 | repr/to-dict 宏、JSON/YAML bridge |
| `dcc-mcp-host` | Host execution bridge | 面向 adapter 的执行契约 |
| `dcc-mcp-tunnel-*` | Remote MCP relay | tunnel protocol、relay、本地 agent |

---

## 精选 API

### 传输层 —— 进程间通信

```python
from dcc_mcp_core import DccLinkFrame, IpcChannelAdapter, SocketServerAdapter

# 服务端：创建通道并等待客户端
server = IpcChannelAdapter.create("dcc-mcp-maya")
server.wait_for_client()

# 客户端：连接服务端
client = IpcChannelAdapter.connect("dcc-mcp-maya")
client.send_frame(DccLinkFrame(msg_type="Call", seq=1, body=b'{"method":"ping"}'))
reply = client.recv_frame()      # DccLinkFrame(msg_type, seq, body)

# 多客户端 socket 服务器（给 bridge 模式 DCC 使用）
sock_server = SocketServerAdapter("/tmp/dcc-mcp.sock",
                                  max_connections=10,
                                  connection_timeout_secs=30)
```

### 进程管理 —— DCC 生命周期控制

```python
from dcc_mcp_core import (
    PyDccLauncher, PyProcessMonitor, PyProcessWatcher, PyCrashRecoveryPolicy,
)

launcher = PyDccLauncher(dcc_type="maya", version="2025")
process = launcher.launch(
    script_path="/path/to/startup.py",
    working_dir="/project",
    env_vars={"MAYA_RENDER_THREADS": "4"},
)

monitor = PyProcessMonitor()
monitor.track(process)
stats = monitor.stats(process)     # CPU、内存、uptime

watcher = PyProcessWatcher(
    recovery_policy=PyCrashRecoveryPolicy(max_restarts=3, cooldown_sec=10),
)
watcher.watch(process)
```

### 沙箱安全 —— 基于策略的访问控制

```python
from dcc_mcp_core import SandboxContext, SandboxPolicy, InputValidator

policy = SandboxPolicy()
ctx = SandboxContext(policy)
validator = InputValidator(ctx)

allowed, reason = validator.validate("delete_all_files")
if not allowed:
    print(f"被策略阻止：{reason}")

# 审计日志
for entry in ctx.audit_log.entries():
    print(f"{entry.action} -> {entry.outcome}")
```

### 工作流与 Artefact 交接 (v0.14+)

```python
from dcc_mcp_core import (
    WorkflowSpec, BackoffKind,
    artefact_put_bytes, artefact_get_bytes,
)

spec = WorkflowSpec.from_yaml_str(yaml_text)
spec.validate()                    # 静态 idempotency_key + 模板引用检查
print(spec.steps[0].policy.retry.next_delay_ms(2))

ref = artefact_put_bytes(b"hello", mime="text/plain")
print(ref.uri)                     # "artefact://sha256/<hex>"
assert artefact_get_bytes(ref.uri) == b"hello"
```

完整 feature 矩阵和决策树请看 [AGENTS.md](AGENTS.md)。

---

## 开发环境

```bash
git clone https://github.com/loonghao/dcc-mcp-core.git
cd dcc-mcp-core

# 推荐：使用 vx（通用开发工具管理器）—— https://github.com/loonghao/vx
vx just dev            # 编译 + 安装 dev wheel（使用项目标准 feature 集合）
vx just test           # 运行 Python 测试
vx just test-rust      # 运行 Rust 单元/集成测试
vx just lint           # 完整 lint 检查（Rust + Python）
vx just preflight      # 预提交检查（cargo check + clippy + fmt + test-rust）
vx just ci             # 完整本地 CI pipeline
```

### 不使用 `vx`

```bash
python -m venv venv
source venv/bin/activate   # Windows：venv\Scripts\activate
pip install maturin pytest pytest-cov ruff mypy

# feature 列表的唯一真实源在根 justfile —— 看 `just print-dev-features`
maturin develop --features "$(just print-dev-features)"
pytest tests/ -v
ruff check python/ tests/ examples/
cargo clippy --workspace -- -D warnings
```

feature 列表的**唯一真实源在 `justfile`**（`OPT_FEATURES`、`DEV_FEATURES`、`WHEEL_FEATURES`、`WHEEL_FEATURES_PY37`）。CI、本地开发、发布 wheel 都从同一处读取。

---

## 发版流程

本项目使用 [Release Please](https://github.com/googleapis/release-please) 自动化版本与发版：

1. **开发**：从 `main` 拉分支，使用 [Conventional Commits](https://www.conventionalcommits.org/) 提交。
2. **合并**：开 PR，合入 `main`。
3. **发布 PR**：Release Please 自动创建 / 更新一个发布 PR，升版本并更新 `CHANGELOG.md`。
4. **发布**：发布 PR 合入后，自动创建 GitHub Release 并发布 wheel 到 PyPI。

### 提交信息格式

| 前缀 | 描述 | 版本升级 |
|---|---|---|
| `feat:` | 新特性 | Minor (`0.x.0`) |
| `fix:` | Bug 修复 | Patch (`0.0.x`) |
| `feat!:` / `BREAKING CHANGE:` | 破坏性变更 | Major (`x.0.0`) |
| `docs:` | 仅文档 | 无发版 |
| `chore:` | 杂务 | 无发版 |
| `ci:` | CI/CD 变更 | 无发版 |
| `refactor:` | 代码重构 | 无发版 |
| `test:` | 测试相关 | 无发版 |
| `build:` | 构建系统 / 依赖变更 | 无发版 |

```bash
git commit -m "feat: add batch skill execution support"
git commit -m "fix: resolve middleware chain ordering issue"
git commit -m "feat!: redesign skill registry API"
git commit -m "feat(skills): add PowerShell script support"
git commit -m "docs: update API reference"
```

---

## 贡献

欢迎贡献 —— 请提 Pull Request。

1. Fork 仓库并克隆到本地。
2. 创建特性分支：`git checkout -b feat/my-feature`。
3. 按照下面的编码规范修改代码。
4. 运行测试与 lint：
   ```bash
   vx just lint        # 代码风格检查
   vx just test        # 运行测试
   vx just preflight   # 所有预提交检查
   ```
5. 使用 [Conventional Commits](https://www.conventionalcommits.org/) 格式提交。
6. 推送并基于 `main` 发起 Pull Request。

### 编码规范

- **风格**：Rust 用 `cargo fmt`，Python 用 `ruff format`（行宽 120、双引号）。
- **类型注解**：所有公开 Python API 必须有类型注解；Rust 用 `thiserror` 做错误、`tracing` 做日志。
- **Docstring**：所有公开模块、类、函数使用 Google 风格 docstring。
- **测试**：新特性必须带测试；保持或提升覆盖率。
- **导入顺序（Python）**：首行 `from __future__ import annotations`，然后 stdlib → 第三方 → 本地，并用段落注释分隔。

---

## 许可证

MIT —— 详情见 [LICENSE](LICENSE)。

---

## AI Agent 资源

如果你是 AI 编码 Agent，同时请阅读：

- [AGENTS.md](AGENTS.md) —— AI Agent 导航图（入口、决策表、Top traps）。
- [`docs/guide/agents-reference.md`](docs/guide/agents-reference.md) —— Agent 详细规则、陷阱、代码风格与项目专属架构约束。
- [`.agents/skills/dcc-mcp-core/SKILL.md`](.agents/skills/dcc-mcp-core/SKILL.md) —— 完整 API skill 定义。
- [`python/dcc_mcp_core/__init__.py`](python/dcc_mcp_core/__init__.py) —— 完整公开 API（~180 符号）。
- [`python/dcc_mcp_core/_core.pyi`](python/dcc_mcp_core/_core.pyi) —— 真实类型 stub（参数名、类型、签名）。
- [`llms.txt`](llms.txt) —— LLM 优化的简洁 API 参考。
- [`llms-full.txt`](llms-full.txt) —— LLM 优化的完整 API 参考。
- [CONTRIBUTING.md](CONTRIBUTING.md) —— 开发流程与编码规范。
