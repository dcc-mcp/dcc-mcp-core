# dcc-mcp-core

[![PyPI](https://img.shields.io/pypi/v/dcc-mcp-core)](https://pypi.org/project/dcc-mcp-core/)
[![Python](https://img.shields.io/pypi/pyversions/dcc-mcp-core)](https://www.python.org/)
[![License](https://img.shields.io/badge/License-MIT-green.svg)](https://opensource.org/licenses/MIT)
[![Downloads](https://static.pepy.tech/badge/dcc-mcp-core)](https://pepy.tech/project/dcc-mcp-core)
[![Coverage](https://img.shields.io/codecov/c/github/loonghao/dcc-mcp-core)](https://codecov.io/gh/loonghao/dcc-mcp-core)
[![Tests](https://img.shields.io/github/actions/workflow/status/loonghao/dcc-mcp-core/ci.yml?branch=main&label=Tests)](https://github.com/loonghao/dcc-mcp-core/actions)
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](http://makeapullrequest.com)
[![Latest Version](https://img.shields.io/github/v/tag/loonghao/dcc-mcp-core?label=Latest%20Version)](https://github.com/loonghao/dcc-mcp-core/releases)

English | [中文](README_zh.md)

**面向 AI 辅助 DCC 工作流的生产级基础库**，结合了 **模型上下文协议（MCP）** 与 **零代码 Skills 系统**。提供 **Rust 驱动核心 + PyO3 Python 绑定**，交付企业级性能、安全性和可扩展性——所有这些均**零运行时 Python 依赖**。支持 Python 3.7–3.13。

> **注意**：本项目处于积极开发中（v0.14+）。API 可能会演进；版本历史请参阅 CHANGELOG.md。

---

## 问题与解决方案

### 为什么不直接用 CLI？
**CLI 工具对 DCC 状态一无所知。** 它们无法看到当前场景、选中对象或视口内容，在隔离环境中执行，迫使 AI：
- 多次往返才能收集上下文
- 从 CLI 输出重建状态（脆弱、缓慢）
- 缺乏来自视口的视觉反馈
- 随请求增长，上下文爆炸问题严重

### 为什么选 MCP（模型上下文协议）？
**MCP 是 AI 原生的**，但标准 MCP 缺少 DCC 自动化的两个关键能力：
1. **上下文爆炸** — MCP 没有机制将工具限定到特定会话或实例，在多 DCC 场景下导致请求膨胀
2. **无生命周期控制** — 无法发现实例状态（活跃场景、文档、进程健康）或控制启动/关闭

### 我们的方案：MCP + Skills 系统

我们**复用并扩展**现有 MCP 生态系统，新增：

| 能力 | 收益 |
|------|------|
| **网关选举与版本感知** | 多实例负载均衡；更新 DCC 启动时自动接管 |
| **会话隔离** | 每个 AI 会话与自己的 DCC 实例通信；防止上下文污染 |
| **Skills 系统（零代码）** | 以 `SKILL.md` + scripts/ 定义工具；无需 Python 胶水代码 |
| **渐进式发现** | 按 DCC 类型、实例、场景、产品过滤工具；防止上下文爆炸 |
| **实例追踪** | 了解活跃文档、PID、显示名称；实现智能路由 |
| **结构化结果** | 每个工具返回 `(success, message, context, next_steps)` 便于 AI 推理 |

这不是重新发明 MCP——而是**解决 MCP 在桌面自动化中的盲点**。

---

## 为什么选 dcc-mcp-core 而非其他方案？

| 方面 | dcc-mcp-core | 通用 MCP | CLI 工具 | 浏览器扩展 |
|------|-------------|---------|---------|-----------|
| **DCC 状态感知** | 场景、文档、实例 ID | 无 | 无 | 部分 |
| **多实例支持** | 网关选举 + 会话隔离 | 单一端点 | 无 | 无 |
| **上下文范围控制** | 按 DCC/场景/产品 | 全局工具 | 无 | 有限 |
| **零代码工具** | SKILL.md + scripts | 需要完整 Python | 仅脚本 | 无 |
| **性能** | Rust + 零拷贝 + IPC | Python 开销 | 进程开销 | 网络开销 |
| **安全性** | 沙箱 + 审计日志 | 手动 | 手动 | 无 |
| **跨平台** | Windows/macOS/Linux | 是 | 有限 | 仅浏览器 |

AI 友好文档：[AGENTS.md](AGENTS.md) | [CLAUDE.md](CLAUDE.md) | [GEMINI.md](GEMINI.md) | [CODEBUDDY.md](CODEBUDDY.md) | [`.agents/skills/dcc-mcp-core/SKILL.md`](.agents/skills/dcc-mcp-core/SKILL.md)

## 架构：三层体系

```
+-----------------------------------------------------------------+
|  AI Agent (Claude、GPT 等)                                       |
|  通过 MCP 协议调用工具 (tools/list、tools/call)                    |
+-------------------------------+---------------------------------+
                                |
                        MCP Streamable HTTP
                                |
+-------------------------------v---------------------------------+
|  网关服务器 (Rust/HTTP)                                           |
|  +-- 版本感知实例选举                                              |
|  +-- 会话隔离与路由                                                |
|  +-- 工具发现 (Skills 派生)                                       |
|  +-- 取消与通知 (SSE)                                             |
+-------------------------------+---------------------------------+
                                |
                 IPC (命名管道 / Unix Socket / TCP)
                                |
          +---------------------+---------------------+
          |                     |                     |
  +-------v-------+   +-------v-------+   +-------v-------+
  |  Maya 桥接     |   | Blender 桥接   |   | Houdini 桥接  |
  |  插件 (Rust)   |   | 插件 (Rust)    |   | 插件 (Rust)   |
  +-------+--------+   +-------+--------+   +-------+-------+
          |                     |                     |
    Python 3.7+           Python 3.7+           Python 3.7+
    (零依赖)              (零依赖)              (零依赖)
```

**第一层：AI 智能体** — 通过标准 MCP 协议调用工具（tools/list、tools/call、notifications）。

**第二层：网关** — 协调发现、会话隔离和请求路由。维护 `__gateway__` 哨兵用于版本感知选举。

**第三层：DCC 适配器** — 带嵌入式 Skills 系统的 Python 桥接插件（Maya、Blender、Photoshop）。每个插件注册文档、场景状态和活跃进程信息。WebView 宿主适配器（AuroraView、浏览器面板）使用更窄的能力表面。

---

## 解决 MCP 上下文爆炸

**问题：** 标准 MCP 在 `tools/list` 中返回所有工具，即使与用户当前任务或 DCC 实例无关。3 个 DCC 实例 x 50 个技能 x 5 个脚本 = **750 个工具**，上下文窗口立即填满。

**我们的解决方案——渐进式发现：**

1. **实例感知** — 每个 DCC 注册活跃文档、PID、显示名称、作用域级别
2. **智能工具范围** — 工具按以下维度过滤：
   - **DCC 类型** — 使用 Maya 时只显示 Maya 工具
   - **作用域** — Repo 技能 < 用户技能 < 系统技能
   - **产品** — 某些工具仅适用于 Houdini，不适用于 Maya
   - **策略** — 隐式调用技能单独分组
3. **会话隔离** — AI 会话绑定到一个 DCC 实例；只能看到其工具

```
不使用 dcc-mcp-core（标准 MCP）：
tools/list 包含：
- 100 个 Maya 工具
- 100 个 Houdini 工具
- 100 个 Blender 工具
+ 250 个共享工具
= 上下文中 550 个工具定义

使用 dcc-mcp-core：
tools/list 按会话实例过滤：
与 Maya 实例 #1 通信的 AI -> 看到 100 个 Maya 工具 + 50 个共享工具
与 Houdini 实例 #1 通信的 AI -> 看到 100 个 Houdini 工具 + 50 个共享工具
= 上下文中 150 个工具（减少 71%）
```

---

## 快速开始

### 安装

```bash
# 从 PyPI 安装（预编译 wheel，支持 Python 3.7+）
pip install dcc-mcp-core

# 或从源码安装（需要 Rust 1.85+）
git clone https://github.com/loonghao/dcc-mcp-core.git
cd dcc-mcp-core
pip install -e .
```

### 基本用法——加载与执行 Skills

```python
import json
from pathlib import Path
from dcc_mcp_core import (
    ToolRegistry, ToolDispatcher,
    EventBus, success_result, scan_and_load
)

# 1. 发现技能（扫描 SKILL.md + scripts/）
skills, skipped = scan_and_load(dcc_name="maya")
print(f"已加载 {len(skills)} 个技能包，跳过 {len(skipped)} 个")

# 2. 将发现的 Skills 注册到 ToolRegistry
registry = ToolRegistry()
from pathlib import Path
for skill in skills:
    for script_path in skill.scripts:
        stem = Path(script_path).stem
        skill_name = f"{skill.name.replace('-', '_')}__{stem}"
        registry.register(name=skill_name, description=skill.description, dcc=skill.dcc)

# 3. 创建调度器并注册处理函数
dispatcher = ToolDispatcher(registry)
dispatcher.register_handler(
    "maya_geometry__create_sphere",
    lambda params: {"object_name": "pSphere1", "radius": params.get("radius", 1.0)},
)

# 4. 订阅生命周期事件
bus = EventBus()
bus.subscribe("action.after_execute", lambda **kw: print(f"事件: {kw}"))

# 5. 调度技能
result = dispatcher.dispatch(
    "maya_geometry__create_sphere",
    json.dumps({"radius": 2.0}),
)
output = result["output"]
print(f"已创建: {output.get('object_name')}")
```

## Skills 系统——零代码 MCP 工具注册

**Skills 系统**是 dcc-mcp-core 的核心创新：让你用**零 Python 代码**将任意脚本注册为 MCP 工具。复用现有的 [OpenClaw Skills](https://docs.openclaw.ai/tools) 格式。

### 五分钟创建第一个 Skill

**1. 创建 `my-tool/SKILL.md`：**

```yaml
---
name: maya-cleanup
description: "场景优化与清理工具"
version: "1.0.0"
dcc: maya
scope: repo              # 信任级别：repo < user < system < admin
tags: ["maintenance", "quality"]
policy:
  allow_implicit_invocation: false  # 需要先显式调用 load_skill
  products: ["maya", "houdini"]     # 仅在这些 DCC 中显示
---
# Maya 场景清理

用于优化和验证 Maya 场景的自动化工具。
```

**2. 创建 `my-tool/scripts/cleanup.py`：**

```python
#!/usr/bin/env python
"""清理场景中的未使用节点。"""
import json, sys
result = {"success": True, "message": "已清理 42 个未使用节点"}
print(json.dumps(result))
sys.exit(0)
```

**3. 注册并调用：**

```python
import os
os.environ["DCC_MCP_SKILL_PATHS"] = "/path/to/my-tool"

from dcc_mcp_core import scan_and_load, ToolRegistry, ToolDispatcher

skills, _ = scan_and_load(dcc_name="maya")
# 工具为：maya_cleanup__cleanup，带结构化结果
```

**就这样。** 无 Python 胶水代码。元数据 + 脚本 = 完成。

### 新特性：SkillPolicy、SkillScope、SkillDependencies

```python
from dcc_mcp_core import SkillMetadata
import json

md = SkillMetadata("maya-scene-publisher")

# 策略：禁止隐式调用（需要用户明确加载）
md.policy = json.dumps({"allow_implicit_invocation": False, "products": ["maya"]})

# 检查：
md.is_implicit_invocation_allowed()   # -> False
md.matches_product("maya")            # -> True
md.matches_product("blender")         # -> False

# 外部依赖声明
deps = {"tools": [
    {"type": "env_var", "value": "PIPELINE_ROOT"},
    {"type": "bin", "value": "mayapy"},
]}
md.external_deps = json.dumps(deps)
```

### Skills 目录结构

```
my-skills/
+-- maya-geometry/
|   +-- SKILL.md          <- 元数据 + 策略
|   +-- scripts/
|       +-- create_sphere.py
|       +-- bevel.py
|       +-- export_fbx.bat
+-- houdini-vex/
|   +-- SKILL.md
|   +-- scripts/
|       +-- compile.py
+-- shared-utils/
    +-- SKILL.md
    +-- scripts/
        +-- screenshot.py
```

---

## 核心概念

### ToolResult — AI 友好的结构化结果

所有技能执行结果使用 `ToolResult`，专为 AI 设计，带有结构化上下文和下一步建议：

```python
from dcc_mcp_core import ToolResult, success_result, error_result

# 工厂函数（推荐）
ok = success_result(
    "球体已创建",
    prompt="考虑添加材质或调整 UV",
    object_name="sphere1", position=[0, 1, 0]
)
# ok.context == {"object_name": "sphere1", "position": [0, 1, 0]}

err = error_result(
    "创建球体失败",
    "半径必须为正数"
)

# 直接构造
result = ToolResult(
    success=True,
    message="操作完成",
    context={"key": "value"}
)

# 字段访问
result.success   # bool
result.message   # str
result.prompt    # Optional[str] -- AI 下一步建议
result.error     # Optional[str] -- 错误详情
result.context   # dict -- 任意结构化数据

# 衍生新实例
result.with_error("something failed")   # 新实例，success=False
result.with_context(count=5)            # 新实例，带更新后的 context
result.to_dict()                        # -> dict
```

### ToolRegistry 与调度器 — 技能执行系统

```python
import json
from dcc_mcp_core import (
    ToolRegistry, ToolDispatcher,
    EventBus, SemVer, VersionedRegistry, VersionConstraint
)

# 带版本支持的注册表
registry = ToolRegistry()
registry.register(
    name="my_skill",
    description="执行某技能",
    dcc="maya",
    version="1.0.0",
    input_schema='{"type": "object", "properties": {"param": {"type": "string"}}}',
)

# 查询
meta = registry.get_action("my_skill")
meta = registry.get_action("my_skill", dcc_name="maya")  # DCC 作用域查找
names = registry.list_actions_for_dcc("maya")
all_skills = registry.list_actions()
dccs = registry.get_all_dccs()

# 带验证的调度器（ToolDispatcher 只接受 registry）
dispatcher = ToolDispatcher(registry)
dispatcher.register_handler("my_skill", lambda params: {"done": True, "param": params.get("param")})
result = dispatcher.dispatch("my_skill", json.dumps({"param": "value"}))
# result == {"action": "my_skill", "output": {"done": True, "param": "value"}, "validation_skipped": False}

# 事件驱动架构
bus = EventBus()
sub_id = bus.subscribe("action.before_execute", lambda **kw: print(f"before: {kw}"))
bus.publish("action.before_execute", action_name="test")
bus.unsubscribe("action.before_execute", sub_id)

# 版本感知注册表
vreg = VersionedRegistry()
vreg.register_versioned("my_action", dcc="maya", version="1.2.0")
vreg.register_versioned("my_action", dcc="maya", version="2.0.0")
result_v = vreg.resolve("my_action", dcc="maya", constraint=">=1.0.0")   # -> version "2.0.0"
result_v1 = vreg.resolve("my_action", dcc="maya", constraint="^1.0.0")  # -> version "1.2.0"
latest = vreg.latest_version("my_action", dcc="maya")                    # -> "2.0.0"
```

### 支持的脚本类型

| 扩展名 | 类型 | 执行方式 |
|--------|------|---------|
| `.py` | Python | `subprocess` |
| `.mel` | MEL (Maya) | DCC 适配器 |
| `.ms` | MaxScript | DCC 适配器 |
| `.bat`, `.cmd` | Batch | `cmd /c` |
| `.sh`, `.bash` | Shell | `bash` |
| `.ps1` | PowerShell | `powershell -File` |
| `.js`, `.jsx` | JavaScript | `node` |
| `.ts` | TypeScript | `node`（通过 ts-node 或 tsx） |

查看 `examples/skills/` 获取 **11 个完整示例**：hello-world、maya-geometry、maya-pipeline、git-automation、ffmpeg-media、imagemagick-tools、usd-tools、clawhub-compat、multi-script、dcc-diagnostics、workflow。

### 内置技能包 — 零配置开箱即用

`dcc-mcp-core` 在 wheel 安装包内直接内置了 **2 个核心技能包**，`pip install dcc-mcp-core` 后无需任何路径配置即可使用。

| 技能包 | 工具 | 用途 |
|--------|------|------|
| `dcc-diagnostics` | `screenshot`、`audit_log`、`tool_metrics`、`process_status` | 通用诊断与调试（适用所有 DCC） |
| `workflow` | `run_chain` | 多步骤 action 链式编排，支持上下文传递 |

```python
from dcc_mcp_core import get_bundled_skills_dir, get_bundled_skill_paths

# 获取内置技能包目录（wheel 安装包内）
print(get_bundled_skills_dir())
# /path/to/site-packages/dcc_mcp_core/skills

# 返回 [bundled_dir] 或 []，可直接扩展搜索路径
paths = get_bundled_skill_paths()                       # 默认开启
paths = get_bundled_skill_paths(include_bundled=False)  # 按需禁用
```

DCC 适配器（如 `dcc-mcp-maya`）默认自动加载内置技能包。如需禁用：`start_server(include_bundled=False)`。

## 架构概览 — 15 个 Rust Crate 工作区

dcc-mcp-core 组织为 **15 个 Rust Crate 工作区**，通过 PyO3/maturin 编译成单个原生 Python 扩展（`_core`）：

| Crate | 职责 | 关键类型 |
|-------|------|---------|
| `dcc-mcp-models` | 数据模型 | `ToolResult`, `SkillMetadata` |
| `dcc-mcp-actions` | 技能执行生命周期 | `ToolRegistry`, `EventBus`, `ToolDispatcher`, `ToolValidator`, `ToolPipeline` |
| `dcc-mcp-skills` | 技能发现与加载 | `SkillScanner`, `SkillCatalog`, `SkillWatcher`, 依赖解析器 |
| `dcc-mcp-protocols` | MCP 协议类型 | `ToolDefinition`, `ResourceDefinition`, `DccAdapter`, `BridgeKind` |
| `dcc-mcp-transport` | IPC 通信 | `DccLinkFrame`, `IpcChannelAdapter`, `GracefulIpcChannelAdapter`, `SocketServerAdapter`, `TransportAddress` |
| `dcc-mcp-process` | 进程管理 | `PyDccLauncher`, `ProcessMonitor`, `CrashRecoveryPolicy` |
| `dcc-mcp-sandbox` | 安全沙箱 | `SandboxPolicy`, `InputValidator`, `AuditLog` |
| `dcc-mcp-shm` | 共享内存 | `SharedBuffer`, LZ4 压缩 |
| `dcc-mcp-capture` | 屏幕捕获 | `Capturer`, 跨平台后端 |
| `dcc-mcp-telemetry` | 可观测性 | `TelemetryConfig`, `ToolMetrics`, tracing |
| `dcc-mcp-usd` | USD 场景 | `UsdStage`, `UsdPrim`, 场景信息桥接 |
| `dcc-mcp-http` | MCP Streamable HTTP 服务器 | `McpHttpServer`, `McpHttpConfig`, `McpServerHandle`, Gateway（首个获胜竞争） |
| `dcc-mcp-server` | 二进制入口点 | `dcc-mcp-server` CLI、Gateway 运行器 |
| `dcc-mcp-utils` | 基础设施 | 文件系统、类型封装、常量、JSON |

## 更多功能

### 传输层 — DccLink IPC 进程通信

```python
from dcc_mcp_core import (
    DccLinkFrame, IpcChannelAdapter, GracefulIpcChannelAdapter,
    SocketServerAdapter, TransportAddress
)

# 服务端：创建 IPC 端点并等待客户端
server = IpcChannelAdapter.create("my-dcc-server")
server.wait_for_client()
frame = server.recv_frame()
response = DccLinkFrame(msg_type=2, seq=frame.seq, body=b'result')
server.send_frame(response)

# 客户端：连接到服务端
client = IpcChannelAdapter.connect("my-dcc-server")
request = DccLinkFrame(msg_type=1, seq=0, body=b'ping')
client.send_frame(request)
reply = client.recv_frame()

# TCP 服务器（跨机器通信）
tcp_server = SocketServerAdapter("/tmp/dcc-mcp.sock")
print(tcp_server.socket_path)
```

### 进程管理 — DCC 生命周期控制

```python
from dcc_mcp_core import (
    PyDccLauncher, PyProcessMonitor, PyProcessWatcher,
    PyCrashRecoveryPolicy
)

# 启动 DCC 应用程序
launcher = PyDccLauncher(dcc_type="maya", version="2025")
process = launcher.launch(
    script_path="/path/to/startup.py",
    working_dir="/project",
    env_vars={"MAYA_RENDER_THREADS": "4"}
)

# 监控健康状态
monitor = PyProcessMonitor()
monitor.track(process)
stats = monitor.stats(process)  # CPU、内存、运行时间

# 崩溃后自动重启
watcher = PyProcessWatcher(
    recovery_policy=PyCrashRecoveryPolicy(max_restarts=3, cooldown_sec=10)
)
watcher.watch(process)
```

### 沙箱安全 — 基于策略的访问控制

```python
from dcc_mcp_core import SandboxContext, SandboxPolicy, InputValidator, AuditLog

policy = (
    SandboxPolicy.builder()
    .allow_read(["/safe/paths/*"])
    .allow_write(["/temp/*"])
    .deny_pattern(["*.critical"])
    .require_approval_for("delete_*")
    .build()
)

ctx = SandboxContext(policy=policy)
validator = InputValidator(ctx)

if not validator.validate_action("delete_all_files"):
    print("被策略阻止！")
else:
    print("允许执行")

# 审计追踪
audit = AuditLog.load()
for entry in audit.entries:
    print(f"{entry.timestamp} [{entry.action}] {entry.decision} -> {entry.details}")
```

## 主要特性

- **Rust 高性能引擎**：rmp-serde 零拷贝序列化、LZ4 共享内存、无锁并发结构
- **零运行时 Python 依赖**：全部编译进原生扩展
- **Skills 系统**：零代码 MCP 工具注册（SKILL.md + scripts/）
- **验证调度**：执行前输入参数验证管道
- **弹性 IPC**：连接池、熔断器、自动重试
- **进程管理**：启动/监控/自动恢复 DCC 进程
- **沙箱安全**：基于策略的访问控制 + 审计日志
- **屏幕捕获**：跨平台 DCC 视口捕获，AI 视觉反馈
- **USD 集成**：通用场景描述读写桥接
- **结构化遥测**：Tracing & 录制可观测性
- **~154 个 Python 公共符号** + 完整 `.pyi` 类型存根
- **兼容 OpenClaw Skills**：直接复用生态格式

## 安装

```bash
# 从 PyPI 安装（预编译 wheel，支持 Python 3.7+）
pip install dcc-mcp-core

# 或从源码安装（需要 Rust 1.85+ 工具链）
git clone https://github.com/loonghao/dcc-mcp-core.git
cd dcc-mcp-core
pip install -e .
```

## 开发环境设置

```bash
# 克隆仓库
git clone https://github.com/loonghao/dcc-mcp-core.git
cd dcc-mcp-core

# 推荐：使用 vx（通用开发工具管理器）
# 安装: https://github.com/loonghao/vx
vx just install     # 安装所有项目依赖
vx just dev         # 构建安装开发 wheel
vx just test        # 运行 Python 测试
vx just lint        # 全量 lint（Rust + Python）
```

### 不使用 vx

```bash
python -m venv venv
source venv/bin/activate   # Windows: venv\Scripts\activate
pip install maturin pytest pytest-cov ruff mypy
maturin develop --features python-bindings,ext-module
pytest tests/ -v
ruff check python/ tests/ examples/
cargo clippy --workspace -- -D warnings
```

## 运行测试

```bash
vx just test           # 所有 Python 测试
vx just test-rust      # 所有 Rust 单元测试
vx just test-cov       # 带覆盖率报告
vx just ci             # 完整 CI 流水线
vx just preflight      # 仅 pre-commit 检查
```

## 更多示例

查看 [`examples/skills/`](examples/skills/) 目录获取 **11 个完整技能包示例**，以及 [VitePress 文档站](https://loonghao.github.io/dcc-mcp-core/) 获取各模块完整指南。

## 版本发布流程

本项目使用 [Release Please](https://github.com/googleapis/release-please) 自动化版本管理：

1. **开发**：从 `main` 创建分支，使用 Conventional Commits 提交
2. **合并**：提交 PR 到 `main`
3. **发布 PR**：Release Please 自动创建/更新发布 PR（含版本号 + CHANGELOG）
4. **发布**：合并后自动创建 GitHub Release 并发布到 PyPI

### 提交信息格式

| 前缀 | 描述 | 版本变更 |
|------|------|---------|
| `feat:` | 新功能 | Minor (`0.x.0`) |
| `fix:` | Bug 修复 | Patch (`0.0.x`) |
| `feat!:` 或 `BREAKING CHANGE:` | 破坏性变更 | Major (`x.0.0`) |
| `docs:` / `chore:` / `ci:` / `refactor:` / `test:` | 不触发发布 |

## 贡献

欢迎贡献！详见 [CONTRIBUTING.md](CONTRIBUTING.md)。快速开始：

1. Fork 并克隆仓库
2. 创建分支：`git checkout -b feat/my-feature`
3. 开发（遵循编码规范）
4. 运行检查：`vx just preflight && vx just test`
5. Conventional Commits 格式提交
6. Push 并提 PR 到 `main`

## 许可证

本项目采用 MIT 许可证 — 详见 [LICENSE](LICENSE) 文件。

## AI Agent 资源

如果你是 AI 编码代理，请同时参考：
- **[AGENTS.md](AGENTS.md)** — 所有 AI 代理综合指南（架构、命令、API 参考、陷阱规避）
- **[CLAUDE.md](CLAUDE.md)** — Claude 专用指令与工作流
- **[GEMINI.md](GEMINI.md)** — Gemini 专用指令与工作流
- **[CODEBUDDY.md](CODEBUDDY.md)** — CodeBuddy Code 专用指令与工作流
- **[.agents/skills/dcc-mcp-core/SKILL.md](.agents/skills/dcc-mcp-core/SKILL.md)** — 完整 API 技能定义，用于学习与使用此库
- **[python/dcc_mcp_core/__init__.py](python/dcc_mcp_core/__init__.py)** — 完整公共 API 表面（约 177 个符号）
- **[llms.txt](llms.txt)** — 精简 API 参考（LLM 优化格式）
- **[llms-full.txt](llms-full.txt)** — 完整 API 参考（LLM 优化格式）
