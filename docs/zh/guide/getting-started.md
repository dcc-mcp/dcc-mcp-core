# 快速开始

## 安装

### 从 PyPI 安装

```bash
pip install dcc-mcp-core
```

### 从源代码安装（需要 Rust 工具链）

```bash
git clone https://github.com/loonghao/dcc-mcp-core.git
cd dcc-mcp-core
pip install -e .
```

::: tip
从源代码构建需要 Rust 工具链，可从 [rustup.rs](https://rustup.rs/) 安装。
构建由 [maturin](https://www.maturin.rs/) 处理，它会编译 Rust 核心并安装 Python 包。
:::

## 环境要求

- **Python**: >= 3.7（CI 测试 3.7、3.8、3.9、3.10、3.11、3.12、3.13）
- **Rust**: >= 1.85（从源代码构建时需要）
- **许可证**: MIT
- **Python 依赖**: 零 — 所有功能都在编译的 Rust 扩展中

## 快速上手

### Skills-First：`create_skill_server`（v0.12.12+ 推荐）

将脚本暴露为 MCP 工具最快捷的方式。在脚本目录创建 `SKILL.md`，然后一键完成所有配置：

```python
import os
from dcc_mcp_core import create_skill_server, McpHttpConfig

# 设置应用专属 Skill 路径
os.environ["DCC_MCP_MAYA_SKILL_PATHS"] = "/path/to/my-skills"

# 一键：发现 Skills + 启动 MCP HTTP 服务器
server = create_skill_server("maya", McpHttpConfig(port=8765))
handle = server.start()
print(f"Maya MCP 服务器地址：{handle.mcp_url()}")
# AI 客户端（Claude Desktop 等）连接到 http://127.0.0.1:8765/mcp
```

如需更多控制，可直接使用 `SkillCatalog`：

```python
import os
from dcc_mcp_core import SkillCatalog, ToolRegistry

os.environ["DCC_MCP_SKILL_PATHS"] = "/path/to/my-skills"

registry = ToolRegistry()
catalog = SkillCatalog(registry)

discovered = catalog.discover(dcc_name="maya")
print(f"发现了 {discovered} 个 Skill")

# 加载 Skill，并查看注册后的 Action 名称
actions = catalog.load_skill("maya-geometry")
print(actions)
```

参见 [Skills 系统指南](/zh/guide/skills) 了解 `SKILL.md` 的编写方式和更多选项。

### Action 注册表

```python
from dcc_mcp_core import ToolRegistry

registry = ToolRegistry()
registry.register(
    name="create_sphere",
    description="Creates a sphere in the scene",
    category="geometry",
    tags=["geometry", "creation"],
    dcc="maya",
)

action = registry.get_action("create_sphere")
print(action)  # 包含 Action 元数据的字典

maya_actions = registry.list_actions(dcc_name="maya")
```

### Action 结果

```python
from dcc_mcp_core import success_result, error_result

result = success_result("创建了 5 个球体", prompt="接下来使用 modify", count=5)
print(result.success)  # True
print(result.message)  # "创建了 5 个球体"
print(result.context)  # {"count": 5}

err = error_result("失败", "文件未找到", prompt="检查路径")
print(err.success)  # False
```

### 事件总线

```python
from dcc_mcp_core import EventBus

bus = EventBus()
sid = bus.subscribe("scene.changed", lambda: print("场景已更新!"))
bus.publish("scene.changed")
bus.unsubscribe("scene.changed", sid)
```

### MCP HTTP 服务器

一行代码将注册表暴露给 AI 客户端（Claude Desktop 等）：

```python
from dcc_mcp_core import ToolRegistry, McpHttpServer, McpHttpConfig

registry = ToolRegistry()
# ... 注册 Actions 或加载 Skills ...

config = McpHttpConfig(port=8765)
server = McpHttpServer(registry, config)
handle = server.start()

print(f"MCP 服务器运行在 {handle.mcp_url()}")
# handle.shutdown() 停止服务器
```

## 开发环境设置

```bash
git clone https://github.com/loonghao/dcc-mcp-core.git
cd dcc-mcp-core

# 使用 vx 安装（推荐）
vx just install

# 或手动设置
pip install maturin
maturin develop
```

## 运行测试

```bash
vx just test
vx just lint
```

## 下一步

- 了解 [Actions 动作](/zh/guide/actions) — 核心构建块
- 探索 [Events 事件](/zh/guide/events) 的生命周期钩子
- 查看 [Skills 技能包](/zh/guide/skills) 的零代码脚本注册
- 使用 [MCP HTTP 服务器](/zh/api/http) 暴露工具给 AI 客户端
- 查看 [传输层](/zh/guide/transport) 的 DCC 通信
- 了解 [架构设计](/zh/guide/architecture) — 14 个 Rust crate 的工作区结构
