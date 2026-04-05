# 常见问题

关于 DCC-MCP-Core 的常见问题解答。

## 通用问题

### 什么是 DCC-MCP-Core？

DCC-MCP-Core 是一个 Rust 核心库，配合 Python 绑定，提供：
- **动作注册表 (Action Registry)**：集中式系统，用于在 DCC 应用中注册和执行动作（Maya、Blender、Houdini、3ds Max 等）
- **事件总线 (Event Bus)**：发布-订阅事件系统，用于挂载 DCC 生命周期钩子
- **MCP 协议类型**：用于 AI 编码助手 Model Context Protocol 的类型定义
- **传输层**：用于分布式 DCC 集成的 IPC 和网络通信

### 支持哪些 DCC 应用？

当前支持的 DCC 集成：
- **Maya**：完整的动作和事件支持
- **Blender**：完整的动作和事件支持
- **Houdini**：完整的动作和事件支持
- **3ds Max**：完整的动作和事件支持
- **Unreal Engine**：传输层支持
- **通用 Python**：支持任意 Python 3.8+ 环境

### 支持哪些 Python 版本？

Python 3.8、3.9、3.10、3.11、3.12 和 3.13 均已支持并在 CI 中测试。

## 安装

### 如何安装 dcc-mcp-core？

**从 PyPI 安装：**
```bash
pip install dcc-mcp-core
```

**从源码安装：**
```bash
git clone https://github.com/loonghao/dcc-mcp-core.git
cd dcc-mcp-core
pip install -e .
```

### 依赖有哪些？

核心库**无第三方依赖**。所有依赖都是可选的：
- `pyo3 >= 0.23` 用于 Python 绑定
- `pytest`、`pytest-cov`、`pytest-mock`、`pyfakefs` 用于测试

### 如何安装特定 DCC 集成？

```bash
# Maya
pip install dcc-mcp-core[maya]

# Blender
pip install dcc-mcp-core[blender]

# 所有 DCC
pip install dcc-mcp-core[all]
```

## 动作 (Actions)

### 如何注册自定义动作？

```python
from dcc_mcp_core import ActionRegistry, action

# 使用装饰器
registry = ActionRegistry()

@registry.action("my_custom_action")
def my_action(x: int, y: int) -> dict:
    """将两个数字相加并返回结果。"""
    return {"result": x + y}

# 或手动注册
def another_action(name: str) -> dict:
    return {"greeting": f"你好, {name}!"}

registry.register("another_action", another_action)
```

### 如何执行动作？

```python
from dcc_mcp_core import ActionRegistry

registry = ActionRegistry()
result = registry.call("my_action", x=10, y=20)

print(result.success)    # True
print(result.message)    # "Action completed successfully"
print(result.context)    # {"result": 30}
```

### 如何验证动作输入？

```python
from dcc_mcp_core import action

@action(validator=lambda params: params.get("x", 0) > 0)
def positive_only(x: int):
    """只接受正数的动作。"""
    return {"x": x}
```

## 事件 (Events)

### 事件系统是如何工作的？

EventBus 提供发布-订阅模式：

```python
from dcc_mcp_core import EventBus

bus = EventBus()

# 订阅事件
def on_save(file_path: str):
    print(f"正在保存到: {file_path}")

bus.subscribe("dcc.save", on_save)

# 发布事件
bus.publish("dcc.save", file_path="/tmp/scene.usd")
```

### 有哪些可用事件？

标准 DCC 生命周期事件：
- `dcc.startup` - DCC 应用启动
- `dcc.shutdown` - DCC 应用关闭
- `dcc.save` - 保存前
- `dcc.save.complete` - 保存完成后
- `dcc.open` - 打开文件前
- `dcc.open.complete` - 打开文件完成后
- `dcc.undo` - 撤销前
- `dcc.redo` - 重做后

### 可以使用异步事件处理程序吗？

可以，EventBus 支持异步处理程序：

```python
import asyncio
from dcc_mcp_core import EventBus

bus = EventBus()

@bus.on("network.request")
async def handle_request(endpoint: str):
    # 异步操作
    await asyncio.sleep(0.1)
    return {"status": "ok"}
```

## 技能包 (Skills)

### 什么是 Skills 系统？

Skills 系统允许通过带有 YAML frontmatter 的 Markdown 文件进行零代码脚本注册：

```markdown
---
name: my-skill
version: 1.0.0
description: 一个有用的技能
---

# 我的技能

这个技能做些有用的事。
```

### 如何扫描技能包？

```python
from dcc_mcp_core.skills import SkillScanner

scanner = SkillScanner()
skills = scanner.scan(["/path/to/skills", "/another/path"])

for skill in skills:
    print(f"{skill.name} v{skill.version}: {skill.description}")
```

## 传输层 (Transport Layer)

### 有哪些传输选项可用？

- **IPC（进程间通信）**：通过 Unix 套接字或命名管道进行快速本地通信
- **TCP**：用于分布式系统的网络通信
- **WebSocket**：基于浏览器的连接
- **HTTP**：REST 风格通信

### 如何创建传输池？

```python
from dcc_mcp_core.transport import TransportPool, TransportConfig

config = TransportConfig(
    max_connections=10,
    timeout=30.0,
)

pool = TransportPool(config)
```

## 故障排除

### 我的动作注册不工作，应该检查什么？

1. 确保动作函数有文档字符串
2. 检查注册和调用时的参数名称是否匹配
3. 验证 ActionRegistry 实例在注册和调用时是同一个
4. 启用调试日志以查看注册消息

### 如何启用调试日志？

```python
import logging
logging.basicConfig(level=logging.DEBUG)

from dcc_mcp_core import ActionRegistry
# 现在所有 ActionRegistry 操作都会打印调试信息
```

### 如何报告 bug 或请求功能？

请在 [GitHub](https://github.com/loonghao/dcc-mcp-core/issues) 上打开 issue，包含：
- DCC 应用及版本
- Python 版本
- 最小复现代码
- 预期 vs 实际行为

## 贡献

### 如何为项目做贡献？

请参阅 [CONTRIBUTING.md](https://github.com/loonghao/dcc-mcp-core/blob/main/CONTRIBUTING.md) 指南：
1. 开发环境设置
2. 编码规范
3. 测试要求
4. Pull Request 流程

### 有社区聊天吗？

加入 [GitHub Discussions](https://github.com/loonghao/dcc-mcp-core/discussions) 的讨论。
