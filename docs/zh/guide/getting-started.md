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

### Action 注册表

```python
from dcc_mcp_core import ActionRegistry

registry = ActionRegistry()
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
- 查看 [传输层](/zh/guide/transport) 的 DCC 通信
