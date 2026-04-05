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

DCC 模型上下文协议（Model Context Protocol，MCP）生态系统的基础库。它提供 **Rust 核心引擎 + PyO3 Python 绑定**，交付高性能动作管理、技能发现、传输层、沙箱安全、共享内存、屏幕捕获、USD 支持和遥测 —— 所有这些均 **零运行时 Python 依赖**。

> **注意**：本项目处于积极开发中（v0.12+）。API 可能会演进；版本历史请参阅 CHANGELOG.md。

## 为什么选择 dcc-mcp-core？

| 特性 | 描述 |
|------|------|
| **高性能** | Rust 核心，rmp-serde 零拷贝序列化 & LZ4 压缩 |
| **类型安全** | 完整的 PyO3 绑定 + 全面 `.pyi` 类型存根（约 105 个公共符号） |
| **Skills 系统** | 零代码脚本注册为 MCP 工具（SKILL.md + scripts/） |
| **弹性传输** | IPC + 连接池、熔断器、重试策略 |
| **进程管理** | 启动/监控/自动恢复 DCC 进程 |
| **沙箱安全** | 基于策略的访问控制 + 审计日志 |
| **跨平台** | Windows、macOS、Linux 三平台测试 |

AI 友好文档：[AGENTS.md](AGENTS.md) | [CLAUDE.md](CLAUDE.md) | [`.agents/skills/dcc-mcp-core/SKILL.md`](.agents/skills/dcc-mcp-core/SKILL.md)

## 快速开始

### 安装

```bash
# 从 PyPI 安装（预编译 wheel，支持 Python 3.8+）
pip install dcc-mcp-core

# 或从源码安装（需要 Rust 工具链）
git clone https://github.com/loonghao/dcc-mcp-core.git
cd dcc-mcp-core
pip install -e .
```

### 基本用法

```python
from dcc_mcp_core import (
    ActionRegistry, ActionDispatcher, ActionValidator,
    EventBus, ActionResultModel, success_result, scan_and_load
)

# 1. 创建动作注册表并从环境变量加载 Skills
registry = ActionRegistry()
skills = scan_and_load(dcc_name="maya")
print(f"已加载 {len(skills)} 个技能包")

# 2. 设置带验证的调度器
validator = ActionValidator()
dispatcher = ActionDispatcher(registry, validator)

# 3. 订阅生命周期事件
bus = EventBus()
bus.subscribe("action.after_execute", lambda e: print(f"✓ {e.action_name}: {e.result.success}"))

# 4. 调用动作（自动验证参数）
result = dispatcher.call(
    "maya_geometry__create_sphere",
    radius=2.0,
    position=[1, 0, 0]
)

if result.success:
    print(f"已创建: {result.context.get('object_name')}")
    if result.prompt:
        print(f"建议: {result.prompt}")
else:
    print(f"错误: {result.error}")
```

## 包结构

DCC-MCP-Core 组织为几个子包：

- **actions**：动作管理和执行
  - `base.py`：基础 Action 类定义
  - `manager.py`：用于动作发现和执行的 ActionManager
  - `registry.py`：用于注册和检索动作的 ActionRegistry
  - `middleware.py`：用于横切关注点的中间件
  - `events.py`：用于动作通信的事件系统

- **models**：MCP 生态系统的数据模型
  - `action_result.py`：动作的结构化结果模型

- **skills**：Skills 技能包系统，零代码脚本注册
  - `scanner.py`：SkillScanner 目录扫描，发现 SKILL.md 文件
  - `loader.py`：SKILL.md 解析器和脚本枚举
  - `script_action.py`：ScriptAction 工厂，动态生成 Action 子类

- **utils**：实用函数和辅助工具
  - `module_loader.py`：模块加载工具
  - `filesystem.py`：文件系统操作
  - `decorators.py`：用于错误处理的函数装饰器
  - `dependency_injector.py`：依赖注入工具
  - `template.py`：模板渲染工具
  - `platform.py`：平台特定工具

## 中间件系统

DCC-MCP-Core 包含一个中间件系统，用于在动作执行前后插入自定义逻辑：

```python
from dcc_mcp_core.actions.middleware import LoggingMiddleware, PerformanceMiddleware, MiddlewareChain
from dcc_mcp_core.actions.manager import ActionManager

# 创建中间件链
chain = MiddlewareChain()

# 添加中间件（顺序很重要 - 先添加的先执行）
chain.add(LoggingMiddleware)  # 记录动作执行详情
chain.add(PerformanceMiddleware, threshold=0.5)  # 监控执行时间

# 使用中间件链创建动作管理器
manager = ActionManager("maya", middleware=chain.build())

# 通过中间件链执行动作
result = manager.call_action("create_sphere", radius=2.0)

# 结果中将包含中间件添加的性能数据
print(f"执行时间：{result.context['performance']['execution_time']:.2f}秒")
```

### 内置中间件

- **LoggingMiddleware**：记录动作执行详情和计时
- **PerformanceMiddleware**：监控执行时间并警告慢动作

### 自定义中间件

您可以通过继承 `Middleware` 基类来创建自定义中间件：

```python
from dcc_mcp_core.actions.middleware import Middleware
from dcc_mcp_core.actions.base import Action
from dcc_mcp_core.models import ActionResultModel

class CustomMiddleware(Middleware):
    def process(self, action: Action, **kwargs) -> ActionResultModel:
        # 预处理逻辑
        print(f"执行 {action.name} 之前")

        # 调用链中的下一个中间件（或动作本身）
        result = super().process(action, **kwargs)

        # 后处理逻辑
        print(f"执行 {action.name} 之后：{'成功' if result.success else '失败'}")

        # 您可以根据需要修改结果
        if result.success:
            result.context["custom_data"] = "由中间件添加"

        return result
```

## 核心概念

### ActionResultModel — AI 友好的结构化结果

所有动作结果使用 `ActionResultModel`，专为 AI 设计，带有结构化上下文和下一步建议：

```python
from dcc_mcp_core import ActionResultModel, success_result, error_result

# 工厂函数（推荐）
ok = success_result(
    message="球体已创建",
    prompt="考虑添加材质或调整 UV",
    context={"object_name": "sphere1", "position": [0, 1, 0]}
)

err = error_result(
    message="创建球体失败",
    error="半径必须为正数"
)
```

### ActionRegistry 与调度器 — 动作系统

```python
from dcc_mcp_core import (
    ActionRegistry, ActionDispatcher, ActionValidator,
    EventBus, SemVer
)

registry = ActionRegistry()
dispatcher = ActionDispatcher(registry, ActionValidator())

# 事件驱动架构
bus = EventBus()
bus.subscribe("action.after_execute", lambda e: print(f"✓ {e.action_name}: {e.result.success}"))
```

## Skills 技能包系统 — 零代码 MCP 工具注册

**Skills 系统**是 dcc-mcp-core 最独特的功能：让你将任何脚本**零代码**注册为 MCP 可发现工具。复用 [OpenClaw Skills](https://docs.openclaw.ai/tools) 生态格式。

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
tools: ["Bash"]
tags: ["automation"]
dcc: maya
---
# 我的工具
```

**3. 使用：**

```python
import os; os.environ["DCC_MCP_SKILL_PATHS"] = "/path/to/my-tool"
from dcc_mcp_core import scan_and_load
skills = scan_and_load(dcc_name="maya")
print(f"已加载 {len(skills)} 个技能包")
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

查看 `examples/skills/` 获取 **10 个完整示例**。

## 架构概览 — 11 个 Rust Crate 工作区

| Crate | 职责 | 关键类型 |
|----------------------|-----------|------------------|
| `dcc-mcp-models` | 数据模型 | `ActionResultModel`, `SkillMetadata` |
| `dcc-mcp-actions` | 动作生命周期 | `ActionRegistry`, `EventBus`, `ActionDispatcher`, `ActionPipeline` |
| `dcc-mcp-skills` | 技能发现 | `SkillScanner`, `SkillWatcher`, 依赖解析器 |
| `dcc-mcp-protocols` | MCP 协议 | `ToolDefinition`, `ResourceDefinition`, `DccAdapter` |
| `dcc-mcp-transport` | IPC 通信 | `TransportManager`, `ConnectionPool`, `CircuitBreaker`, `FramedChannel` |
| `dcc-mcp-process` | 进程管理 | `PyDccLauncher`, `ProcessMonitor`, `CrashRecoveryPolicy` |
| `dcc-mcp-sandbox` | 安全沙箱 | `SandboxPolicy`, `InputValidator`, `AuditLog` |
| `dcc-mcp-shm` | 共享内存 | `SharedBuffer`, LZ4 压缩 |
| `dcc-mcp-capture` | 屏幕捕获 | `Capturer`, 跨平台后端 |
| `dcc-mcp-telemetry` | 可观测性 | `TelemetryConfig`, tracing |
| `dcc-mcp-usd` | USD 场景 | `UsdStage`, `UsdPrim` |

```python
ActionResultModel(
    success=True,
    message="成功创建球体",
    prompt="现在您可以修改球体的属性或添加材质",
    error=None,
    context={
        "object_name": "sphere_1.0",
        "position": [0, 0, 0]
    }
)
```

### 字段

- **success**：布尔值，表示动作是否成功
- **message**：人类可读的结果消息
- **prompt**：关于下一步操作的建议
- **error**：当 success 为 False 时的错误消息
- **context**：包含额外上下文数据的字典

### 方法

- **to_dict()**：将模型转换为字典，具有版本无关的兼容性（兼容 Pydantic v1 和 v2）
- **model_dump()** / **dict()**：原生 Pydantic 序列化方法（版本相关）

### 使用示例

```python
# 创建结果模型
result = ActionResultModel(
    success=True,
    message="操作完成",
    prompt="下一步建议",
    context={"key": "value"}
)

# 转换为字典（版本无关）
result_dict = result.to_dict()

# 访问字段
if result.success:
    print(f"成功：{result.message}")
    if result.prompt:
        print(f"下一步：{result.prompt}")
    print(f"上下文数据：{result.context}")
else:
    print(f"错误：{result.error}")
```

## 功能特性

- 基于类的 Action 设计，使用 Pydantic 模型
- 参数验证和类型检查
- 带有上下文和提示的结构化结果格式
- 动态动作发现和加载
- 用于横切关注点的中间件支持
- 用于动作通信的事件系统
- 异步动作执行
- 全面的错误处理
- **Skills 技能包系统**：零代码将脚本（MEL、MaxScript、BAT、Shell、Python）注册为 MCP 工具
- **兼容 OpenClaw**：直接复用 OpenClaw Skills 生态格式（SKILL.md + scripts/）

## Skills 技能包系统

Skills 系统允许你将任何脚本（Python、MEL、MaxScript、BAT、Shell 等）零代码注册为 MCP 可发现的工具。直接复用 [OpenClaw Skills](https://docs.openclaw.ai/tools) 生态格式。

### 快速上手

1. **创建 Skill 目录**，包含 `SKILL.md` 和 `scripts/` 文件夹：

```
maya-geometry/
├── SKILL.md
└── scripts/
    ├── create_sphere.py
    ├── batch_rename.mel
    └── export_fbx.bat
```

2. **编写 SKILL.md**（标准 OpenClaw 格式）：

```yaml
---
name: maya-geometry
description: "Maya 几何体创建和修改工具"
tools: ["Bash", "Read"]
tags: ["maya", "geometry"]
---
# Maya Geometry Skill

使用这些工具在 Maya 中创建和修改几何体。
```

3. **设置环境变量**指向 Skills 目录：

```bash
# Linux/macOS
export DCC_MCP_SKILL_PATHS="/path/to/my-skills"

# Windows
set DCC_MCP_SKILL_PATHS=C:\path\to\my-skills

# 多路径（使用平台路径分隔符）
export DCC_MCP_SKILL_PATHS="/path/skills1:/path/skills2"
```

4. **完成！** 脚本自动被发现并注册为 MCP 工具：

```python
from dcc_mcp_core import create_action_manager

manager = create_action_manager("maya")
# DCC_MCP_SKILL_PATHS 中的 Skills 自动加载

# 调用 Skill Action
result = manager.call_action("maya_geometry__create_sphere", radius=2.0)
```

### 支持的脚本类型

| 扩展名 | 类型 | 执行方式 |
|--------|------|---------|
| `.py` | Python | 通过系统 Python `subprocess` 执行 |
| `.mel` | MEL (Maya) | 通过 context 中的 DCC 适配器执行 |
| `.ms` | MaxScript | 通过 context 中的 DCC 适配器执行 |
| `.bat`, `.cmd` | Batch | `cmd /c` |
| `.sh`, `.bash` | Shell | `bash` |
| `.ps1` | PowerShell | `powershell -File` |
| `.js`, `.jsx` | JavaScript | `node` |

### 工作原理

1. **SkillScanner** 扫描目录寻找 `SKILL.md` 文件
2. **SkillLoader** 解析 YAML frontmatter 并枚举 `scripts/` 目录
3. **ScriptAction 工厂** 为每个脚本动态生成 Action 子类
4. Action 注册到现有的 **ActionRegistry**
5. MCP Server 层可通过 **EventBus** 订阅 `skill.loaded` 事件

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
- **~105 个 Python 公共符号** + 完整 `.pyi` 类型存根
- **兼容 OpenClaw Skills**：直接复用生态格式

## 安装

```bash
# 从 PyPI 安装（预编译 wheel，支持 Python 3.8+）
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
vx just test       # 运行 Python 测试
vx just lint       # 全量 lint（Rust + Python）
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
vx just test-rust       # 所有 Rust 单元测试
vx just test-cov        # 带覆盖率报告
vx just ci              # 完整 CI 流水线
vx just preflight       # 仅 pre-commit 检查
```

### 传输层 — IPC 进程通信

dcc-mcp-core 提供生产级 IPC 传输层：

```python
from dcc_mcp_core import (
    TransportManager, TransportAddress, TransportScheme,
    RoutingStrategy, IpcListener, connect_ipc,
    FramedChannel
)

# 服务端：监听连接
listener = IpcListener.new("/tmp/dcc-mcp-server.sock")
handle = listener.start(handler_fn=my_message_handler)

# 客户端：连接服务器
channel = connect_ipc("/tmp/dcc-mcp-server.sock")
response = channel.call({"method": "ping", "params": {}})

# 高级：连接池 + 弹性
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
stats = monitor.stats(process)

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

## 更多示例

查看 [`examples/skills/`](examples/skills/) 目录获取 **10 个完整技能包示例**，以及 [VitePress 文档站](https://loonghao.github.io/dcc-mcp-core/) 获取各模块完整指南。

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
| `feat!:` | 破坏性变更 | Major (`x.0.0`) |
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
- **[AGENTS.md](AGENTS.md)** — 所 AI 代理综合指南（架构、命令、API 参考、陷阱规避）
- **[CLAUDE.md](CLAUDE.md)** — Claude Code 专用指令
- **[.agents/skills/dcc-mcp-core/SKILL.md](.agents/skills/dcc-mcp-core/SKILL.md)** — 完整 API 技能定义，用于学习与使用此库
- **[python/dcc_mcp_core/__init__.py](python/dcc_mcp_core/__init__.py)** — 完整公共 API 表面（约 105 个符号）
