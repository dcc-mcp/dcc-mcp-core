# 什么是 DCC-MCP-Core？

DCC-MCP-Core 是 DCC（数字内容创建）Model Context Protocol (MCP) 生态系统的**基础 Rust 库（含 Python 绑定）**。它使 AI 助手能够通过统一的高性能接口与 DCC 软件（Maya、Blender、Houdini、3ds Max 等）进行交互。

基于 **Rust 核心**，通过 [PyO3](https://pyo3.rs/) 和 [maturin](https://github.com/PyO3/maturin) 编译为 Python 扩展模块，将 Rust 的性能与线程安全和 Python 的易用性相结合。

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

- **ToolRegistry** — 线程安全的 Action 注册、搜索和版本管理
- **SkillCatalog** — 渐进式 Skill 发现与加载；脚本通过 SKILL.md 自动注册为 MCP 工具（v0.12.10 起支持 Skills-First 架构）
- **EventBus** — DCC 生命周期事件的发布/订阅系统
- **MCP HTTP 服务器** — 符合 2025-03-26 规范的流式 HTTP 服务器，将 DCC 工具暴露给 AI 客户端
- **MCP 协议类型** — Tools、Resources、Prompts 和 Annotations 的类型安全定义
- **传输层** — DCC 通信的连接池、服务发现和会话管理（TCP、命名管道、Unix Socket）
- **共享内存** — DCC 与 Agent 进程之间的零拷贝场景数据传输
- **进程管理** — DCC 进程启动、监控和崩溃恢复
- **遥测** — 基于 OpenTelemetry 的追踪与指标基础设施
- **沙箱** — AI 操作的安全策略、输入验证和审计日志
- **截图捕获** — DCC 视口截图捕获
- **USD 桥接** — 通过 OpenUSD Stage 进行场景交换
- **类型包装器** — RPyC 安全包装器（BooleanWrapper、IntWrapper、FloatWrapper、StringWrapper）
- **零 Python 运行时依赖** — 纯 Rust 核心编译为原生 Python 扩展

## 架构

DCC-MCP-Core 是一个包含 **14 个子 crate** 的 Rust workspace，通过 maturin 编译为单一 Python 扩展模块 `dcc_mcp_core._core`：

```
dcc-mcp-core/
├── src/lib.rs                  # PyO3 模块入口点 (_core)
├── crates/
│   ├── dcc-mcp-models/         # ToolResult, SkillMetadata, ToolDeclaration
│   ├── dcc-mcp-actions/        # ToolRegistry, EventBus, Pipeline, Dispatcher, Validator
│   ├── dcc-mcp-skills/         # SkillScanner, SkillCatalog, SkillWatcher, Resolver
│   ├── dcc-mcp-protocols/      # MCP 类型：ToolDefinition, ResourceDefinition, Prompt, DccAdapter
│   ├── dcc-mcp-transport/      # IPC (ipckit), DccLinkFrame, IpcChannelAdapter, SocketServerAdapter
│   ├── dcc-mcp-process/        # PyDccLauncher, ProcessMonitor, CrashRecovery
│   ├── dcc-mcp-telemetry/      # ToolRecorder, ToolMetrics, TelemetryConfig
│   ├── dcc-mcp-sandbox/        # SandboxPolicy, SandboxContext, AuditLog, InputValidator
│   ├── dcc-mcp-shm/            # PySharedBuffer, PyBufferPool, PySharedSceneBuffer
│   ├── dcc-mcp-capture/        # Capturer, CaptureFrame
│   ├── dcc-mcp-usd/            # UsdStage, UsdPrim, VtValue, SdfPath
│   ├── dcc-mcp-http/           # McpHttpServer, McpHttpConfig, McpServerHandle, Gateway
│   ├── dcc-mcp-server/         # dcc-mcp-server CLI, Gateway runner
│   └── dcc-mcp-utils/          # 文件系统, 常量, 类型包装器, JSON 工具
└── python/
    └── dcc_mcp_core/
        ├── __init__.py          # 从 _core 重导出约 140 个公开符号
        ├── skill.py             # 纯 Python Skill 脚本辅助（无 _core 依赖）
        └── _core.pyi            # 所有公开 API 的类型存根
```

## Python API 概览

所有公开 API 均可从顶层包 `dcc_mcp_core` 访问，包含约 140 个公开符号，跨越 14 个领域：

```python
from dcc_mcp_core import (
    # Actions
    ToolRegistry, ToolDispatcher, ToolPipeline, ToolValidator,
    ToolRecorder, ToolMetrics, EventBus,
    ToolResult, success_result, error_result,

    # Skills — Skills-First 架构
    SkillCatalog, SkillSummary, SkillMetadata, ToolDeclaration,
    SkillScanner, SkillWatcher, scan_and_load,

    # MCP HTTP 服务器
    McpHttpServer, McpHttpConfig,

    # 传输层
    IpcChannelAdapter, GracefulIpcChannelAdapter, SocketServerAdapter, DccLinkFrame,

    # 协议类型
    ToolDefinition, ToolAnnotations, ResourceDefinition, PromptDefinition,

    # 共享内存
    PySharedSceneBuffer, PySharedBuffer, PyBufferPool,

    # 进程管理
    PyDccLauncher, PyProcessWatcher, PyCrashRecoveryPolicy,

    # 遥测
    TelemetryConfig, is_telemetry_initialized,

    # 沙箱
    SandboxPolicy, SandboxContext, InputValidator, AuditLog,

    # 截图捕获
    Capturer, CaptureFrame,

    # USD
    UsdStage, UsdPrim, VtValue, SdfPath,
)
```

完整的符号说明请参阅 [API 参考](/zh/api/actions)。

## 版本与 Python 支持

- **当前版本**：0.14.11 <!-- x-release-please-version -->
- **Python**：3.7–3.13（abi3-py38 wheel，CI 全版本测试）
- **Rust**：Edition 2024，MSRV 1.85
- **构建**：maturin + PyO3；零运行时 Python 依赖

## 相关项目

- [dcc-mcp-rpyc](https://github.com/loonghao/dcc-mcp-rpyc) — 远程 DCC 操作的 RPyC 桥接
- [dcc-mcp-maya](https://github.com/loonghao/dcc-mcp-maya) — Maya MCP 服务器实现
