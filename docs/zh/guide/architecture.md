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

## 内存模型

### ActionRegistry

`ActionRegistry` 使用 `DashMap` 实现线程安全的动作存储：

```rust
struct ActionRegistry {
    actions: DashMap<String, Arc<dyn Action>>,
}
```

### EventBus

`EventBus` 同时支持同步和异步处理程序：

```rust
struct EventBus {
    sync_handlers: DashMap<String, Vec<HandlerFn>>,
    async_handlers: DashMap<String, Vec<AsyncHandlerFn>>,
}
```

### TransportPool

连接池实现高效资源利用：

```rust
struct TransportPool {
    config: TransportConfig,
    connections: DashMap<SessionId, Connection>,
    semaphore: Semaphore,
}
```

## 线程安全

所有内部状态使用：
- `parking_lot::Mutex` 用于短期临界区
- `DashMap` 用于并发哈希映射
- `Arc` 用于共享所有权

不使用 `std::sync::Mutex` - `parking_lot` 更快且不会在 panic 时中毒。

## 错误处理

使用 `thiserror` 处理错误类型：

```rust
#[derive(Error, Debug)]
pub enum TransportError {
    #[error("连接超时: {0}")]
    Timeout(String),

    #[error("连接被拒绝: {0}")]
    ConnectionRefused(String),

    #[error("Ping 超时 {0}s 后")]
    PingTimeout(u64),
}
```

## 测试策略

- **单元测试**：每个 crate 有内联 `#[cfg(test)]` 模块
- **集成测试**：`tests/` 目录包含 Python 和 Rust 测试
- **属性测试**：`proptest` 用于随机化测试
- **模糊测试**：`cargo-fuzz` 用于协议解析

## 构建优化

Release 配置针对最佳性能：

```toml
[profile.release]
opt-level = 3      # 最大优化
lto = true         # 链接时优化
codegen-units = 1  # 单代码生成单元以获得更好优化
strip = true       # 从二进制文件中剥离符号
```

## 未来考虑

- **WebAssembly 支持**：浏览器端 DCC 通信潜力
- **gRPC 传输**：自定义线协议的替代方案
- **GraphQL 订阅支持**：DCC 状态实时更新
