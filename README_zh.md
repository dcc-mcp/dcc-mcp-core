# DCC-MCP-Core

[![PyPI](https://img.shields.io/pypi/v/dcc-mcp-core)](https://pypi.org/project/dcc-mcp-core/)
[![Python](https://img.shields.io/pypi/pyversions/dcc-mcp-core)](https://www.python.org/)
[![License](https://img.shields.io/badge/License-MIT-green.svg)](https://opensource.org/licenses/MIT)
[![Downloads](https://static.pepy.tech/badge/dcc-mcp-core)](https://pepy.tech/project/dcc-mcp-core)
[![Coverage](https://img.shields.io/codecov/c/github/loonghao/dcc-mcp-core)](https://codecov.io/gh/loonghao/dcc-mcp-core)
[![Tests](https://img.shields.io/github/actions/workflow/status/loonghao/dcc-mcp-core/ci.yml?branch=main&label=Tests)](https://github.com/loonghao/dcc-mcp-core/actions)
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](http://makeapullrequest.com)
[![Latest Version](https://img.shields.io/github/v/tag/loonghao/dcc-mcp-core?label=Latest%20Version)](https://github.com/loonghao/dcc-mcp-core/releases)

[English](README.md) | [中文文档](README_zh.md)

DCC 模型上下文协议（Model Context Protocol，MCP）生态系统的基础库。提供 **Rust 核心引擎 + PyO3 Python 绑定**，交付高性能技能管理、技能发现、传输层、沙箱安全、共享内存、屏幕捕获、USD 支持和遥测 —— 所有这些均 **零运行时 Python 依赖**。

> **注意**：本项目处于积极开发中（v0.12+）。API 可能会演进；版本历史请参阅 CHANGELOG.md。

## 为什么选择 dcc-mcp-core？

| 特性 | 描述 |
|------|------|
| **高性能** | Rust 核心，rmp-serde 零拷贝序列化 & LZ4 压缩 |
| **类型安全** | 完整的 PyO3 绑定 + 全面 `.pyi` 类型存根（约 120 个公共符号） |
| **Skills 系统** | 零代码脚本注册为 MCP 工具（SKILL.md + scripts/） |
| **弹性传输** | IPC + 连接池、熔断器、重试策略 |
| **进程管理** | 启动/监控/自动恢复 DCC 进程 |
| **沙箱安全** | 基于策略的访问控制 + 审计日志 |
| **跨平台** | Windows、macOS、Linux 三平台测试 |

AI 友好文档：[AGENTS.md](AGENTS.md) | [CLAUDE.md](CLAUDE.md) | [GEMINI.md](GEMINI.md) | [`.agents/skills/dcc-mcp-core/SKILL.md`](.agents/skills/dcc-mcp-core/SKILL.md)

## 快速开始

### 安装

```bash
# 从 PyPI 安装（预编译 wheel，支持 Python 3.7+）
pip install dcc-mcp-core

# 或从源码安装（需要 Rust 工具链）
git clone https://github.com/loonghao/dcc-mcp-core.git
cd dcc-mcp-core
pip install -e .
```

### 基本用法

```python
import json
from dcc_mcp_core import (
    ActionRegistry, ActionDispatcher,
    EventBus, success_result, scan_and_load
)

# 1. 加载 Skills；scan_and_load 返回 2-tuple (skills, skipped_dirs)
skills, skipped = scan_and_load(dcc_name="maya")
print(f"已加载 {len(skills)} 个技能包")

# 2. 将发现的 Skills 注册到 ActionRegistry
registry = ActionRegistry()
from pathlib import Path
for skill in skills:
    for script_path in skill.scripts:
        stem = Path(script_path).stem
        skill_name = f"{skill.name.replace('-', '_')}__{stem}"
        registry.register(name=skill_name, description=skill.description, dcc=skill.dcc)

# 3. 创建调度器并注册处理函数
dispatcher = ActionDispatcher(registry)
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

## 核心概念

### ActionResultModel — AI 友好的结构化结果

所有技能执行结果使用 `ActionResultModel`，专为 AI 设计，带有结构化上下文和下一步建议：

```python
from dcc_mcp_core import ActionResultModel, success_result, error_result

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
result = ActionResultModel(
    success=True,
    message="操作完成",
    context={"key": "value"}
)

# 字段访问
result.success   # bool
result.message   # str
result.prompt    # Optional[str] — AI 下一步建议
result.error     # Optional[str] — 错误详情
result.context   # dict — 任意结构化数据

# 衍生新实例
result.with_error("something failed")   # 新实例，success=False
result.with_context(count=5)            # 新实例，带更新后的 context
result.to_dict()                        # -> dict
```

### ActionRegistry 与调度器 — 技能执行系统

```python
import json
from dcc_mcp_core import (
    ActionRegistry, ActionDispatcher,
    EventBus, SemVer, VersionedRegistry, VersionConstraint
)

# 带版本支持的注册表
registry = ActionRegistry()
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

# 带验证的调度器（ActionDispatcher 只接受 registry）
dispatcher = ActionDispatcher(registry)
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
result_v = vreg.resolve("my_action", dcc="maya", constraint=">=1.0.0")   # → version "2.0.0"
result_v1 = vreg.resolve("my_action", dcc="maya", constraint="^1.0.0")  # → version "1.2.0"
latest = vreg.latest_version("my_action", dcc="maya")                    # → "2.0.0"
```

## Skills 技能包系统 — 零代码 MCP 工具注册

**Skills 系统**是 dcc-mcp-core 最独特的功能：让你将任何脚本（Python、MEL、MaxScript、BAT、Shell 等）**零代码**注册为 MCP 可发现工具。复用 [OpenClaw Skills](https://docs.openclaw.ai/tools) 生态格式。

### 工作原理

```
SKILL.md（元数据）+ scripts/ 目录
       ↓  SkillScanner 发现并解析
每个技能包的 SkillMetadata（名称、描述、标签、脚本列表）
       ↓  技能注册到 ActionRegistry → AI 通过 MCP 可调用
```

### 快速示例

**1. 创建 Skill 目录：**

```
my-tool/
├── SKILL.md          # 元数据 + 描述
└── scripts/
    └── list.py      # 你的脚本
```

**2. 编写 `SKILL.md`：**

```yaml
---
name: my-tool
description: "我的自定义 DCC 自动化工具"
allowed-tools: ["Bash"]
tags: ["automation", "custom"]
dcc: maya
version: "1.0.0"
---
# My Tool

Maya 工作流优化自动化脚本。
```

**3. 设置环境变量并使用：**

```python
import os
os.environ["DCC_MCP_SKILL_PATHS"] = "/path/to/my-tool"

from dcc_mcp_core import scan_and_load, ActionRegistry

registry = ActionRegistry()
skills = scan_and_load(dcc_name="maya")
for s in skills:
    print(f"✓ {s.name}: {len(s.scripts)} 个脚本")

# 调用 Skill 动作：{skill_name}__{script_stem}
result = registry.call("my_tool__list", some_param="value")
```

### 技能命名规则

每个 `scripts/` 目录下的脚本成为一个技能入口，命名为 `{skill_name}__{script_stem}`：
- `maya-geometry/scripts/create_sphere.py` → `maya_geometry__create_sphere`
- `maya-geometry/scripts/batch_rename.mel` → `maya_geometry__batch_rename`

注意：技能名称中的连字符会替换为下划线。

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

查看 `examples/skills/` 获取 **9 个完整示例**：hello-world、maya-geometry、maya-pipeline、git-automation、ffmpeg-media、imagemagick-tools、usd-tools、clawhub-compat、multi-script。

### 内置技能包 — 零配置开箱即用

`dcc-mcp-core` 在 wheel 安装包内直接内置了 **5 个通用技能包**，`pip install dcc-mcp-core` 后无需任何路径配置即可使用。

| 技能包 | 工具 | 用途 |
|--------|------|------|
| `dcc-diagnostics` | `screenshot`、`audit_log`、`action_metrics`、`process_status` | 通用诊断与调试（适用所有 DCC） |
| `workflow` | `run_chain` | 多步骤 action 链式编排，支持上下文传递 |
| `git-automation` | `repo_stats`、`changelog_gen` | Git 仓库分析 |
| `ffmpeg-media` | `convert`、`probe`、`thumbnail` | 媒体格式转换（需要 ffmpeg） |
| `imagemagick-tools` | `resize`、`composite` | 图像处理（需要 ImageMagick） |

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

## 架构概览 — 11 个 Rust Crate 工作区

dcc-mcp-core 组织为 **11 个 Rust Crate 工作区**，通过 PyO3/maturin 编译成单个原生 Python 扩展（`_core`）：

| Crate | 职责 | 关键类型 |
|-------|------|---------|
| `dcc-mcp-models` | 数据模型 | `ActionResultModel`, `SkillMetadata` |
| `dcc-mcp-actions` | 技能执行生命周期 | `ActionRegistry`, `EventBus`, `ActionDispatcher`, `ActionValidator`, `ActionPipeline` |
| `dcc-mcp-skills` | 技能发现 | `SkillScanner`, `SkillWatcher`, 依赖解析器 |
| `dcc-mcp-protocols` | MCP 协议类型 | `ToolDefinition`, `ResourceDefinition`, `DccAdapter` |
| `dcc-mcp-transport` | IPC 通信 | `TransportManager`, `ConnectionPool`, `IpcListener`, `FramedChannel`, `CircuitBreaker` |
| `dcc-mcp-process` | 进程管理 | `PyDccLauncher`, `ProcessMonitor`, `CrashRecoveryPolicy` |
| `dcc-mcp-sandbox` | 安全沙箱 | `SandboxPolicy`, `InputValidator`, `AuditLog` |
| `dcc-mcp-shm` | 共享内存 | `SharedBuffer`, LZ4 压缩 |
| `dcc-mcp-capture` | 屏幕捕获 | `Capturer`, 跨平台后端 |
| `dcc-mcp-telemetry` | 可观测性 | `TelemetryConfig`, `ActionMetrics`, tracing |
| `dcc-mcp-usd` | USD 场景 | `UsdStage`, `UsdPrim`, 场景信息桥接 |
| `dcc-mcp-utils` | 基础设施 | 文件系统、类型封装、常量、JSON |

## 更多功能

### 传输层 — IPC 进程通信

```python
from dcc_mcp_core import (
    TransportManager, TransportAddress, TransportScheme,
    RoutingStrategy, IpcListener, connect_ipc, FramedChannel
)

# 服务端：监听连接
listener = IpcListener.new("/tmp/dcc-mcp-server.sock")
handle = listener.start(handler_fn=my_message_handler)

# 客户端：连接服务器
channel = connect_ipc("/tmp/dcc-mcp-server.sock")
response = channel.call({"method": "ping", "params": {}})

# 高级：连接池 + 弹性熔断
mgr = TransportManager()
mgr.configure_pool(min_size=2, max_size=10)
mgr.set_circuit_breaker(threshold=5, reset_timeout=30)
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
    print(f"{entry.timestamp} [{entry.action}] {entry.decision} → {entry.details}")
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
- **~120 个 Python 公共符号** + 完整 `.pyi` 类型存根
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

查看 [`examples/skills/`](examples/skills/) 目录获取 **9 个完整技能包示例**，以及 [VitePress 文档站](https://loonghao.github.io/dcc-mcp-core/) 获取各模块完整指南。

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
- **[.agents/skills/dcc-mcp-core/SKILL.md](.agents/skills/dcc-mcp-core/SKILL.md)** — 完整 API 技能定义，用于学习与使用此库
- **[python/dcc_mcp_core/__init__.py](python/dcc_mcp_core/__init__.py)** — 完整公共 API 表面（约 120 个符号）
- **[llms.txt](llms.txt)** — 精简 API 参考（LLM 优化格式）
- **[llms-full.txt](llms-full.txt)** — 完整 API 参考（LLM 优化格式）
