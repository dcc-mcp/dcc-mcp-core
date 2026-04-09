# 常见问题（FAQ）

关于 DCC-MCP-Core 的常见问题解答。

## 基础问题

### DCC-MCP-Core 是什么？

DCC-MCP-Core 是一个基础 Rust 库（含 Python 绑定），提供：

- **ActionRegistry** — 线程安全的 Action 注册与查找
- **SkillCatalog** — 渐进式 Skill 发现与加载；脚本通过 SKILL.md 自动注册为 MCP 工具
- **EventBus** — DCC 生命周期 Hook 的发布/订阅事件系统
- **MCP 协议类型** — Model Context Protocol 的类型定义（Tools、Resources、Prompts）
- **传输层** — 分布式 DCC 集成的 IPC 与网络通信
- **MCP HTTP 服务器** — 将 DCC 工具暴露给 AI 客户端的流式 HTTP 服务器

### 支持哪些 DCC 应用？

dcc-mcp-core 是 DCC 无关的 — 核心库提供基础设施，DCC 特定集成由独立项目提供：

- **Maya** — 通过 [dcc-mcp-maya](https://github.com/loonghao/dcc-mcp-maya)
- **Blender、Houdini、3ds Max、Unreal** — 使用本库的社区/第三方集成

核心库适用于任何 Python 3.7+ 环境。

### 支持哪些 Python 版本？

Python 3.7–3.13 在 CI 中全部测试。使用 `abi3-py38` 构建 wheel 以最大化兼容性。

### 是否有 Python 运行时依赖？

**没有。** 库没有任何 Python 运行时依赖，所有内容都编译进 Rust 核心。

## 安装

### 如何安装 dcc-mcp-core？

**从 PyPI：**
```bash
pip install dcc-mcp-core
```

**从源码编译（需要 Rust 1.85+ 和 maturin）：**
```bash
git clone https://github.com/loonghao/dcc-mcp-core.git
cd dcc-mcp-core
pip install maturin
maturin develop
```

## Actions

### 如何注册 Action？

```python
from dcc_mcp_core import ActionRegistry, ActionDispatcher
import json

reg = ActionRegistry()

# 注册 Action 元数据（可选带 JSON Schema）
reg.register(
    name="create_sphere",
    description="创建多边形球体",
    category="geometry",
    tags=["create", "mesh"],
    dcc="maya",
    version="1.0.0",
    input_schema=json.dumps({
        "type": "object",
        "required": ["radius"],
        "properties": {"radius": {"type": "number", "minimum": 0.0}},
    }),
)

# 附加 Python 处理器
dispatcher = ActionDispatcher(reg)
dispatcher.register_handler("create_sphere", lambda params: {"name": "sphere1"})
result = dispatcher.dispatch("create_sphere", '{"radius": 1.0}')
print(result["output"])  # {"name": "sphere1"}
```

### 如何返回结构化结果？

```python
from dcc_mcp_core import success_result, error_result, from_exception

# 成功
result = success_result("球体已创建", context={"name": "sphere1"})
print(result.success)   # True
print(result.context)   # {"name": "sphere1"}

# 错误
result = error_result("创建球体失败", error="没有活动场景")
print(result.success)   # False

# 从异常创建
try:
    raise ValueError("半径必须 > 0")
except Exception:
    result = from_exception("无效半径")
```

### 如何验证 Action 输入？

```python
from dcc_mcp_core import ActionValidator

validator = ActionValidator.from_schema_json('{"type":"object","required":["radius"],"properties":{"radius":{"type":"number"}}}')
ok, errors = validator.validate('{"radius": 1.0}')
assert ok

ok, errors = validator.validate('{}')
assert not ok
print(errors)  # ['missing required field: radius']
```

## 事件系统

### 事件系统如何工作？

```python
from dcc_mcp_core import EventBus

bus = EventBus()

# 订阅 — 返回订阅 ID
def on_save(file_path: str):
    print(f"正在保存到：{file_path}")

sub_id = bus.subscribe("dcc.save", on_save)

# 发布
bus.publish("dcc.save", file_path="/tmp/scene.usd")

# 取消订阅
bus.unsubscribe("dcc.save", sub_id)
```

::: warning 异步处理器
EventBus 原生不支持 `async def` 回调。如需异步逻辑，请在同步处理器中调度到你的事件循环。
:::

## Skills

### Skills 系统是什么？

Skills 系统允许零代码脚本注册。将脚本放入带有 `SKILL.md` 文件的目录，它们就会被自动发现并注册为 MCP 工具：

```markdown
---
name: maya-geometry
description: "几何体创建工具"
version: "1.0.0"
dcc: maya
tags: ["geometry"]
tools:
  - name: create_sphere
    description: "创建球体"
    source_file: scripts/create_sphere.py
---
```

### 如何发现并加载 Skill？

```python
from dcc_mcp_core import ActionRegistry, SkillCatalog
import os

os.environ["DCC_MCP_SKILL_PATHS"] = "/path/to/skills"

registry = ActionRegistry()
catalog = SkillCatalog(registry)

# 发现 Skill
count = catalog.discover(dcc_name="maya")

# 加载 Skill（将工具注册到 ActionRegistry）
actions = catalog.load_skill("maya-geometry")
print(actions)  # ['maya_geometry__create_sphere']
```

### Skill 工具的 Action 命名规则是什么？

Action 名称遵循 `{skill名称（下划线）}__{工具名称}` 格式，例如：
- Skill `maya-geometry`，工具 `create_sphere` → Action `maya_geometry__create_sphere`

### 如何不加载就扫描 Skill？

```python
from dcc_mcp_core import scan_and_load_lenient

skills, skipped = scan_and_load_lenient(extra_paths=["/my/skills"])
for skill in skills:
    print(f"{skill.name} ({len(skill.tools)} 个工具)")
```

## 传输层

### 支持哪些传输选项？

- **TCP** — 网络通信（`TransportAddress.tcp(host, port)`）
- **命名管道** — Windows 上的低延迟本地通信（`TransportAddress.named_pipe(name)`）
- **Unix Domain Socket** — Linux/macOS 上的低延迟本地通信（`TransportAddress.unix_socket(path)`）

使用 `TransportAddress.default_local(dcc_type, pid)` 自动选择当前平台的最优 IPC 传输。

### 如何注册 DCC 服务并连接到它？

**DCC 端（服务器）：**
```python
import os
from dcc_mcp_core import TransportManager, IpcListener, TransportAddress

mgr = TransportManager("/tmp/dcc-mcp")
instance_id, listener = mgr.bind_and_register("maya", version="2025")
channel = listener.accept()  # 等待 Agent 连接
```

**Agent 端（客户端）：**
```python
from dcc_mcp_core import TransportManager, connect_ipc

mgr = TransportManager("/tmp/dcc-mcp")
entry = mgr.find_best_service("maya")
channel = connect_ipc(entry.effective_address())
rtt = channel.ping()
```

## MCP HTTP 服务器

### 如何通过 HTTP 为 AI 客户端暴露 DCC 工具？

```python
from dcc_mcp_core import ActionRegistry, McpHttpServer, McpHttpConfig

registry = ActionRegistry()
registry.register("get_scene_info", description="获取场景信息", category="scene", dcc="maya")

server = McpHttpServer(registry, McpHttpConfig(port=8765))
handle = server.start()
print(handle.mcp_url())  # http://127.0.0.1:8765/mcp
# 将 AI 客户端连接到此 URL
handle.shutdown()
```

## 故障排查

### Action 注册不生效怎么办？

1. 确认注册和查找使用的是同一个 `ActionRegistry` 实例
2. 调用 `reg.list_actions()` 验证 Action 是否已注册
3. 使用 `reg.get_action("my_action")` 检查存储的元数据
4. 若使用 `ActionDispatcher`，验证 `dispatcher.handler_count()` > 0

### 如何启用调试日志？

导入前设置 `DCC_MCP_LOG` 环境变量：
```bash
export DCC_MCP_LOG=debug
```

或通过 `TelemetryConfig` 配置：
```python
from dcc_mcp_core import TelemetryConfig

cfg = TelemetryConfig("my-service").with_stdout_exporter()
cfg.init()
```

### 如何报告 Bug 或请求功能？

请在 [GitHub](https://github.com/loonghao/dcc-mcp-core/issues) 上提 Issue，并包含：
- DCC 应用及版本
- Python 版本（`python --version`）
- dcc-mcp-core 版本（`python -c "import dcc_mcp_core; print(dcc_mcp_core.__version__)"`）
- 最小可复现代码
- 预期行为与实际行为

## 贡献

### 如何贡献代码？

参阅 [CONTRIBUTING.md](https://github.com/loonghao/dcc-mcp-core/blob/main/CONTRIBUTING.md)。关键步骤：

1. 安装 Rust 1.85+ 和 Python 3.8+
2. 克隆仓库
3. 运行 `vx just dev` 以开发模式构建安装
4. 运行 `vx just test` 执行测试套件

### 是否有社区讨论渠道？

在 [GitHub Discussions](https://github.com/loonghao/dcc-mcp-core/discussions) 参与讨论。
