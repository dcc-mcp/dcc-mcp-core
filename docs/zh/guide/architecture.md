# 架构设计

本文档描述 DCC-MCP-Core 的架构设计。DCC-MCP-Core 是一个 Rust 驱动的 DCC Model Context Protocol 生态系统基础库。

## 概述

DCC-MCP-Core 采用 Rust workspace 结构，通过 PyO3 提供 Python 绑定。核心特点：

- **零运行时第三方依赖** - Rust 核心无第三方运行时依赖
- **可选 Python 绑定** - 通过 PyO3 实现 DCC 集成
- **模块化 crate 设计** - 按需选择性依赖

## Crate 结构

```
dcc-mcp-core (workspace 根目录)
├── dcc-mcp-models      # 数据模型和类型定义
├── dcc-mcp-actions     # 动作注册系统
├── dcc-mcp-skills      # 技能包系统
├── dcc-mcp-protocols   # MCP 协议类型
├── dcc-mcp-transport   # IPC 和网络传输
└── dcc-mcp-utils       # 工具函数
```

### 依赖关系图

```
dcc-mcp-models (基础类型)
       ↓
dcc-mcp-actions ← dcc-mcp-models (动作依赖模型)
       ↓
dcc-mcp-skills ← dcc-mcp-actions, dcc-mcp-models
       ↓
dcc-mcp-protocols ← dcc-mcp-models (协议类型使用模型)
       ↓
dcc-mcp-transport ← dcc-mcp-protocols (传输层使用协议类型)
       ↓
dcc-mcp-utils (共享工具，无内部依赖)
```

## 各 Crate 职责

### dcc-mcp-models

**职责**：核心数据模型和类型定义。

**关键类型**：
- `ActionResult`：标准化的动作执行结果
- `SkillMetadata`：技能包元数据
- `EventData`：事件负载结构
- `UriTemplate`：URI 模板解析和匹配

**依赖**：无（基础 crate）

### dcc-mcp-actions

**职责**：集中式动作注册和执行系统。

**关键组件**：
- `ActionRegistry`：全局动作注册和查找
- `Action`：动作实现的 trait
- `ActionManager`：DCC 特定的动作管理

**关键函数**：
- `register_action()`：注册命名动作
- `call_action()`：执行已注册的动作
- `list_actions()`：获取所有已注册动作

**依赖**：`dcc-mcp-models`

### dcc-mcp-skills

**职责**：通过 Markdown 文件实现零代码技能包注册。

**关键组件**：
- `SkillScanner`：扫描目录中的技能包
- `SkillRegistry`：集中式技能存储
- `SkillResolver`：解析技能依赖关系

**技能包格式**：
```markdown
---
name: my-skill
version: 1.0.0
description: 一个有用的技能
author: 作者名称
---

# 技能文档

技能内容和说明...
```

**依赖**：`dcc-mcp-actions`、`dcc-mcp-models`

### dcc-mcp-protocols

**职责**：MCP（Model Context Protocol）类型定义。

**关键类型**：
- `MCPServerProtocol`：服务端协议实现
- `MCPClientProtocol`：客户端协议实现
- `PromptProtocol`：提示词处理
- `ResourceProtocol`：资源管理
- `ToolProtocol`：工具执行

**URI 模板**：
- `{+uri}` - 带片段的 URI
- `{uri}` - 基础 URI
- 查询参数提取

**依赖**：`dcc-mcp-models`

### dcc-mcp-transport

**职责**：IPC 和网络通信层。

**传输类型**：
- **IPC**：Unix 套接字 / Windows 命名管道
- **TCP**：网络套接字
- **WebSocket**：浏览器兼容
- **HTTP**：REST 风格通信

**关键组件**：
- `TransportPool`：连接池
- `TransportConfig`：配置管理
- `Session`：连接会话追踪
- `WireProtocol`：二进制序列化（MessagePack）

**Ping/Pong 健康检查**：
- `ping()` - 发送心跳
- `ping_with_timeout()` - 超时检查响应
- 超时自动重连

**依赖**：`dcc-mcp-protocols`、`tokio`

### dcc-mcp-utils

**职责**：共享工具函数。

**模块**：
- `filesystem`：文件路径操作
- `type_wrappers`：Python 类型互操作辅助
- `constants`：共享常量

**依赖**：无

## Python 绑定

Python 绑定通过 PyO3 的 `python-bindings` feature 生成：

```toml
[features]
python-bindings = ["pyo3", "dcc-mcp-models/python-bindings", ...]
```

### Python 包结构

```
python/dcc_mcp_core/
├── __init__.py      # 公开 API
├── _core.pyi       # 类型存根
└── _core.*.so      # 编译的 Rust 扩展
```

### 绑定模式

每个 crate 暴露一个 `python_module!` 宏来生成 Python 模块：

```rust
#[pymodule]
fn dcc_mcp_core(_py: Python, m: &PyModule) -> PyResult<()> {
    dcc_mcp_models::python::register_module(m)?;
    dcc_mcp_actions::python::register_module(m)?;
    // ...
    Ok(())
}
```

## 设计决策

### 1. 零运行时依赖

Rust 核心没有第三方运行时依赖。这确保了：
- 最小的二进制大小
- 可预测的行为
- DCC 环境中无依赖版本冲突

可选依赖（如 `pyo3`）通过 feature 门控。

### 2. PyO3 0.28

使用 PyO3 0.28，特性：
- `multiple-pymethods` - 每个 struct 多个 #[pymethods]
- `abi3-py38` - Python 3.8+ 稳定 ABI
- `extension-module` - 允许从任意 Python 路径加载

### 3. Rust Edition 2024

Edition 2024 提供：
- 隐式 `async fn` 在 trait 定义中
- `async let` 绑定
- 生命周期子类型改进

### 4. Tokio 异步运行时

使用 Tokio 因为：
- Rust 异步的事实标准
- 出色的 Windows 支持（命名管道）
- 与 PyO3 配合良好

### 5. MessagePack 序列化

使用 RMP（Rust MessagePack）作为线协议：
- 紧凑的二进制格式
- 快速序列化/反序列化
- 语言无关

## Skills-First 架构

推荐通过 `create_skill_manager` 以 **Skills-First** 模式将 DCC 工具暴露到 MCP。一次调用即可串联完整的技术栈：

```
create_skill_manager("maya")
        │
        ├─ ActionRegistry   （线程安全 Action 注册表）
        ├─ ActionDispatcher （将调用路由到 Python 处理函数）
        ├─ SkillCatalog     （发现 + 加载 SKILL.md 技能包）
        │       └─ 扫描 DCC_MCP_MAYA_SKILL_PATHS + DCC_MCP_SKILL_PATHS
        └─ McpHttpServer    （返回可立即启动的 HTTP 服务器）
```

```python
import os
os.environ["DCC_MCP_MAYA_SKILL_PATHS"] = "/studio/maya-skills"

from dcc_mcp_core import create_skill_manager, McpHttpConfig

server = create_skill_manager("maya", McpHttpConfig(port=8765))
handle = server.start()
print(f"Maya MCP server: {handle.mcp_url()}")
# 完成后调用 handle.shutdown()
```

**技能路径解析顺序**（先找到的优先）：
1. 应用专属环境变量：`DCC_MCP_{APP}_SKILL_PATHS`（如 `DCC_MCP_MAYA_SKILL_PATHS`）
2. 全局环境变量：`DCC_MCP_SKILL_PATHS`
3. 平台数据目录：`~/.local/share/dcc-mcp/skills/{app}/`
4. `extra_paths` 参数传入的额外路径

::: tip 手动组装
如果需要自定义中间件或更精细的控制，可手动组装：
`ActionRegistry` → `ActionDispatcher` → `SkillCatalog` → `McpHttpServer`。
:::

## 线程安全

所有内部状态使用：
- `parking_lot::Mutex` 用于短期临界区
- `parking_lot::RwLock` 用于读写模式
- `Arc` 用于共享所有权

不使用 `std::sync::Mutex` — `parking_lot` 更快且不会在 panic 时中毒。

## 错误处理

使用 `thiserror` 处理错误类型：

```rust
#[derive(Error, Debug)]
pub enum TransportError {
    #[error("连接超时: {0}")]
    Timeout(String),

    #[error("连接被拒绝: {0}")]
    ConnectionRefused(String),
}
```

## 测试策略

- **单元测试**：每个 crate 有内联 `#[cfg(test)]` 模块
- **集成测试**：`tests/` 目录包含 Python 和 Rust 测试
- **覆盖率追踪**：`cargo-llvm-cov` + `pytest --cov`

## 构建命令

| 命令 | 工具 | 用途 |
|------|------|------|
| `cargo check` | cargo | 快速语法/类型检查 |
| `cargo clippy` | clippy | 使用 `-D warnings` 静态分析 |
| `cargo fmt --check` | rustfmt | 格式检查 |
| `maturin develop` | maturin | 以开发模式安装 wheel |
| `cargo test --workspace` | cargo | 运行所有 Rust 测试 |
| `pytest tests/` | pytest | 运行 Python 集成测试 |
