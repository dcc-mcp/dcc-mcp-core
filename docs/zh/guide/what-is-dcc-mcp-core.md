# 什么是 DCC-MCP-Core？

DCC-MCP-Core 是一个为数字内容创建（DCC）应用程序设计的**动作管理系统**，提供统一的接口，使 AI 能够与各种 DCC 软件（如 Maya、Blender、Houdini 等）进行交互。

基于 **Rust 核心**，通过 [PyO3](https://pyo3.rs/) 暴露给 Python，将 Rust 的性能和线程安全与 Python 的易用性相结合。

## 核心工作流程

```mermaid
flowchart LR
    AI([AI 助手]):::aiNode
    MCP{{MCP 服务器}}:::serverNode
    Core{{DCC-MCP-Core}}:::coreNode
    DCC[/DCC 软件/]:::dccNode

    AI -->|1. 请求| MCP
    MCP -->|2. 发现 Actions| Core
    Core -->|3. 在 DCC 中执行| DCC
    DCC -->|4. 结果| Core
    Core -->|5. 结构化结果| MCP
    MCP -->|6. 响应| AI

    classDef aiNode fill:#f9d,stroke:#f06,stroke-width:2px,color:#333
    classDef serverNode fill:#bbf,stroke:#66f,stroke-width:2px,color:#333
    classDef coreNode fill:#fbb,stroke:#f66,stroke-width:2px,color:#333
    classDef dccNode fill:#bfb,stroke:#6b6,stroke-width:2px,color:#333
```

## 核心特性

- **ActionRegistry** — 基于 DashMap 的线程安全、无锁 Action 注册与查找
- **EventBus** — 发布/订阅系统，用于解耦的 Action 生命周期事件
- **Skills 技能包** — 零代码将脚本注册为 MCP 工具（通过 SKILL.md）
- **MCP 协议类型** — Tools、Resources 和 Prompts 的类型安全定义
- **传输层** — 连接池、服务发现、会话管理，用于 DCC 通信
- **类型包装器** — RPyC 安全包装器（BooleanWrapper、IntWrapper、FloatWrapper、StringWrapper）
- **零 Python 依赖** — 纯 Rust 核心编译为原生 Python 扩展
- **线程安全** — 所有核心类型使用 DashMap 实现无锁并发访问

## 架构

DCC-MCP-Core 是一个 Rust workspace，包含 6 个子 crate，通过 maturin 编译为单个 Python 扩展模块：

```
dcc-mcp-core/
├── src/lib.rs                  # PyO3 模块入口 (_core)
├── crates/
│   ├── dcc-mcp-actions/        # ActionRegistry、EventBus
│   ├── dcc-mcp-models/         # ActionResultModel、SkillMetadata
│   ├── dcc-mcp-protocols/      # MCP 类型定义 (Tool, Resource, Prompt)
│   ├── dcc-mcp-skills/         # SKILL.md 扫描器和加载器
│   ├── dcc-mcp-transport/      # 连接池、服务发现、会话管理
│   └── dcc-mcp-utils/          # 文件系统、常量、类型包装器、日志
└── python/
    └── dcc_mcp_core/
        └── __init__.py          # 从 _core 扩展重新导出
```

## Python API 概览

所有公共 API 均可从顶层 `dcc_mcp_core` 包导入：

```python
import dcc_mcp_core

# 16 个类、14 个函数、8 个常量
# 详见 API 参考获取完整文档
```

## 相关项目

- [dcc-mcp-rpyc](https://github.com/loonghao/dcc-mcp-rpyc) — 用于远程 DCC 操作的 RPyC 桥接
- [dcc-mcp-maya](https://github.com/loonghao/dcc-mcp-maya) — Maya MCP 服务器实现
