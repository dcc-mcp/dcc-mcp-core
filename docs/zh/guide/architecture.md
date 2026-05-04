# 架构设计

DCC-MCP-Core 是一个 Rust workspace，通过 PyO3 提供 Python 绑定。核心特点：

- **零运行时第三方依赖** — Rust 核心无第三方运行时依赖
- **可选 Python 绑定** — 通过 PyO3 实现 DCC 集成
- **30 个 workspace 成员**（29 个功能 crate + `workspace-hack`）— 以根目录 `Cargo.toml` 为准，按需选择性依赖

## Crate 结构

```
dcc-mcp-core (workspace 根目录)
├── dcc-mcp-models       # ToolResult, SkillMetadata, DCC 类型
├── dcc-mcp-actions      # ToolRegistry, EventBus, ToolDispatcher, Pipeline
├── dcc-mcp-skills       # SkillScanner, SkillCatalog, SkillWatcher, Resolver
├── dcc-mcp-protocols    # MCP 类型: ToolDefinition, ResourceDefinition, Prompt, BridgeKind
├── dcc-mcp-jsonrpc      # MCP 2025-03-26 JSON-RPC 线协议类型
├── dcc-mcp-job          # 异步 job 追踪和可选持久化
├── dcc-mcp-skill-rest   # Per-DCC /v1/* REST Skill API
├── dcc-mcp-gateway      # Multi-DCC 网关和动态 capability index
├── dcc-mcp-transport    # IPC (ipckit), DccLinkFrame, IpcChannelAdapter, SocketServerAdapter
├── dcc-mcp-process      # PyDccLauncher, ProcessMonitor, ProcessWatcher, CrashRecovery
├── dcc-mcp-telemetry    # Tracing/recording: ToolRecorder, TelemetryConfig
├── dcc-mcp-sandbox      # Security: SandboxPolicy, SandboxContext, AuditLog
├── dcc-mcp-shm          # Shared memory: PySharedBuffer, PyBufferPool
├── dcc-mcp-capture      # Screen capture: Capturer, CaptureFrame
├── dcc-mcp-usd          # USD scene description: UsdStage, SdfPath, VtValue
├── dcc-mcp-http         # Embedded MCP Streamable HTTP server core
├── dcc-mcp-server       # 二进制入口点: dcc-mcp-server, gateway runner
├── dcc-mcp-logging      # 滚动文件日志
├── dcc-mcp-paths        # 平台路径辅助
├── dcc-mcp-pybridge*    # PyO3 helper crates
├── dcc-mcp-host         # Host execution bridge 契约
└── dcc-mcp-tunnel-*     # Remote MCP tunnel protocol、relay、agent
```

### 依赖关系图

```
dcc-mcp-models (基础类型)
       ↓
dcc-mcp-actions ← dcc-mcp-models
       ↓
dcc-mcp-skills ← dcc-mcp-actions, dcc-mcp-models
       ↓
dcc-mcp-protocols ← dcc-mcp-models
       ↓
dcc-mcp-transport ← dcc-mcp-protocols
       ↓
dcc-mcp-http ← dcc-mcp-transport, dcc-mcp-protocols, dcc-mcp-actions, dcc-mcp-skills
       ↓
dcc-mcp-server ← dcc-mcp-http
```

## 各 Crate 职责

### dcc-mcp-models

**职责**：核心数据模型和类型定义，所有 crate 共享。

**关键类型**：
- `ToolResult` — 统一的动作执行结果类型
- `SkillMetadata` — 解析后的技能包元数据
- `SceneInfo`、`SceneStatistics` — DCC 场景信息
- `DccInfo`、`DccCapabilities`、`DccError` — DCC 适配器类型
- `ScriptResult`、`CaptureResult` — 操作结果

**依赖**：无（基础 crate）

### dcc-mcp-actions

**职责**：集中式动作注册、验证、调度和中间件管线系统。

**关键组件**：
- `ToolRegistry` — 线程安全注册表：register/get/search/list/unregister actions
- `ToolDispatcher` — 带验证的调度，路由到已注册的 Python 可调用对象
- `ToolValidator` — 基于 JSON Schema 的参数验证
- `ToolPipeline` — 中间件管线（日志、计时、审计、限流）
- `EventBus` — DCC 生命周期事件的发布/订阅系统
- `VersionedRegistry` — 多版本动作注册表，支持 SemVer 约束解析

**关键特征**：动作是普通的 Python 可调用对象，通过 `ToolDispatcher.register_handler()` 注册

**依赖**：`dcc-mcp-models`

### dcc-mcp-skills

**职责**：零代码技能包发现、加载和文件系统热重载。

**关键组件**：
- `SkillScanner` — 基于 mtime 缓存的目录扫描器，发现 SKILL.md 包
- `SkillCatalog` — 渐进式技能发现与加载管理（推荐 API）
- `SkillWatcher` — 平台原生文件系统监听器（inotify/FSEvents/ReadDirectoryChangesW）
- `SkillMetadata` — 从 SKILL.md frontmatter 解析的元数据
- 依赖解析：`resolve_dependencies`、`expand_transitive_dependencies`、`validate_dependencies`

**技能包格式**：`SKILL.md` 含 YAML frontmatter（`name`、`version`、`description`、`tools`、`dcc`、`tags`、`depends`、`search-hint`）

**依赖**：`dcc-mcp-actions`、`dcc-mcp-models`

### dcc-mcp-protocols

**职责**：MCP（Model Context Protocol）类型定义，遵循 2025-03-26 规范。

**关键类型**：
- `ToolDefinition`、`ToolAnnotations` — MCP 工具模式及行为提示
- `ResourceDefinition`、`ResourceTemplateDefinition`、`ResourceAnnotations` — MCP 资源模式
- `PromptDefinition`、`PromptArgument` — MCP 提示词模式
- `DccAdapter` — DCC 适配器能力描述符
- `BridgeKind` — 桥接类型枚举（Http、WebSocket、NamedPipe、Custom）

**依赖**：`dcc-mcp-models`

### dcc-mcp-transport

**职责**：IPC 和网络传输层，基于 ipckit 提供帧级通信。

**传输类型**：
- **IPC**：Unix sockets (Linux/macOS) / Windows 命名管道 — 亚毫秒延迟，PID 唯一
- **TCP**：网络套接字 — 跨机器或降级使用

**关键组件**：
- `IpcChannelAdapter` — 基于 ipckit 的客户端/服务端 IPC 适配器，使用 DccLink 帧
- `SocketServerAdapter` — 多客户端 TCP/UDS 监听器，用于服务端 IPC
- `DccLinkFrame` — DccLink 线协议二进制帧类型（msg_type, seq, body）
- `TransportAddress` — 协议无关端点（TCP、命名管道、Unix Socket）
- `FileRegistry` — 基于文件的服务发现（Gateway 使用）

**线协议**：MessagePack，4 字节大端长度前缀

**依赖**：`dcc-mcp-protocols`、`tokio`

### dcc-mcp-process

**职责**：跨平台 DCC 进程生命周期管理和崩溃恢复。

**关键组件**：
- `PyDccLauncher` — 异步 spawn/terminate/kill DCC 进程
- `PyProcessMonitor` — 通过 `sysinfo` 进行 CPU/内存监控
- `PyProcessWatcher` — 后台事件轮询监听器，含心跳/状态追踪
- `PyCrashRecoveryPolicy` — 指数/固定退避重启策略

**依赖**：`tokio`、`sysinfo`

### dcc-mcp-telemetry

**职责**：分布式追踪和指标收集。

**关键组件**：
- `ToolRecorder` / `RecordingGuard` — RAII 计时守卫，用于动作执行
- `ToolMetrics` — 每个动作指标的只读快照（计数、成功率、P95/P99 延迟）
- `TelemetryConfig` — 全局遥测 provider 构建器（stdout/JSON 导出器）

**依赖**：`tracing`、`metrics`

### dcc-mcp-sandbox

**职责**：安全策略执行、审计日志和输入验证。

**关键组件**：
- `SandboxPolicy` — API 白名单、路径允许列表、执行约束（超时、最大动作数、只读）
- `SandboxContext` — 每会话执行上下文，捆绑策略 + 审计日志
- `AuditLog` / `AuditEntry` — 每次动作调用的结构化审计追踪
- `InputValidator` — 基于 Schema 的验证，含注入防护模式匹配

**依赖**：无

### dcc-mcp-shm

**职责**：零拷贝共享内存缓冲区，用于高频 DCC ↔ Agent 数据交换。

**关键组件**：
- `PySharedBuffer` — 命名内存映射文件缓冲区，支持跨进程传递
- `PyBufferPool` — 固定容量的可复用缓冲池（在 30fps 下摊销 mmap 开销）
- `PySharedSceneBuffer` — 高级包装器，含内联 vs 分块存储（>256 MiB 分割）

**压缩**：写入时可选 LZ4；读取时自动解压

**依赖**：`lz4`

### dcc-mcp-capture

**职责**：GPU 帧缓冲截图和 DCC 应用视口捕获。

**后端**：
- **Windows**：DXGI Desktop Duplication API — GPU 直接访问，<16ms 每帧
- **Linux**：X11 XShmGetImage
- **降级**：Mock 合成后端（CI / headless）

**关键组件**：
- `Capturer` — 自动后端选择入口点（`new_auto()` / `new_mock()`）
- `CaptureFrame` — 捕获的图像数据，含 PNG/JPEG/raw BGRA 编码

**依赖**：平台特定（windows-capture、x11grab 等）

### dcc-mcp-usd

**职责**：USD 场景描述数据模型和序列化（纯 Rust，无 OpenUSD C++ 依赖）。

**关键组件**：
- `UsdStage` — 主 Stage 容器，含 prim 管理和元数据
- `UsdPrim` — Prim，含属性 get/set 和 API Schema 检查
- `SdfPath` — 场景图路径，含绝对/相对解析
- `VtValue` — 变体值容器（bool、int、float、string、vec3f、asset、token）

**序列化**：USDA（可读）和 JSON（紧凑，用于 IPC）

**桥接函数**：`scene_info_json_to_stage`、`stage_to_scene_info_json`、`units_to_mpu`、`mpu_to_units`

**依赖**：`pxr-usd`（薄包装，无 C++ 运行时）

### dcc-mcp-http

**职责**：MCP Streamable HTTP 服务器（2025-03-26 规范），面向 HTTP 客户端。

**关键组件**：
- `McpHttpServer` — 后台线程 HTTP 服务器（axum/Tokio）
- `McpHttpConfig` — 服务器配置（端口、CORS、请求超时、Gateway 字段）
- `McpServerHandle` — 服务器句柄，含 URL 获取和优雅关机；`is_gateway` 标记是否赢得网关竞争
- `GatewayRunner` / `GatewayConfig` / `GatewayHandle` — 首个获胜端口竞争、实例注册、心跳、代理
- `GatewayState` — Gateway 运行时状态（实例注册表、路由）

**Gateway 架构**：
当 `McpHttpConfig.gateway_port > 0` 时，首个绑定该端口的进程成为 Gateway，提供：
- `/instances` — 所有活跃实例的 JSON 列表
- `/mcp` — Gateway 自身的 MCP 端点（6 个发现元工具）
- `/mcp/{instance_id}` — 透明代理到特定实例
- `/mcp/dcc/{dcc_type}` — 代理到指定 DCC 类型的最佳实例

**依赖**：`axum`、`tokio`、`dcc-mcp-transport`、`dcc-mcp-protocols`、`dcc-mcp-actions`、`dcc-mcp-skills`、`reqwest`、`socket2`

### dcc-mcp-server

**职责**：独立的二进制入口点，提供完整的 MCP 服务器。

**关键组件**：
- `dcc-mcp-server` CLI — 解析命令行参数，启动 Gateway + MCP HTTP 服务器
- 使用 `GatewayRunner` 库 API 进行端口竞争和实例注册

**依赖**：`dcc-mcp-http`

### 基础设施 crate

旧的 `dcc-mcp-utils` 已拆分并删除。新增基础设施能力应放到有明确职责的 crate：`dcc-mcp-logging`（文件日志）、`dcc-mcp-paths`（平台路径）、`dcc-mcp-pybridge`（PyO3/JSON/YAML 桥接），或对应领域 crate，避免重新引入通用 `utils`。



## Skills-First 架构

推荐通过 `create_skill_server` 以 **Skills-First** 模式将 DCC 工具暴露到 MCP。一次调用即可串联完整的技术栈：

```
create_skill_server("maya")
        │
        ├─ ToolRegistry   （线程安全 Action 注册表）
        ├─ ToolDispatcher （将调用路由到 Python 处理函数）
        ├─ SkillCatalog     （发现 + 加载 SKILL.md 技能包）
        │       └─ 扫描 DCC_MCP_MAYA_SKILL_PATHS + DCC_MCP_SKILL_PATHS
        └─ McpHttpServer    （返回可立即启动的 HTTP 服务器）
```

```python
import os
os.environ["DCC_MCP_MAYA_SKILL_PATHS"] = "/studio/maya-skills"

from dcc_mcp_core import create_skill_server, McpHttpConfig

server = create_skill_server("maya", McpHttpConfig(port=8765))
handle = server.start()
print(f"Maya MCP server: {handle.mcp_url()}")
# 完成后调用 handle.shutdown()
```

**技能路径解析顺序**（先找到的优先）：
1. `DCC_MCP_{APP}_SKILL_PATHS` — 应用专属环境变量（如 `DCC_MCP_MAYA_SKILL_PATHS`）
2. `DCC_MCP_SKILL_PATHS` — 全局降级
3. 平台数据目录：`~/.local/share/dcc-mcp/skills/{app}/`
4. `extra_paths` 参数

::: tip 手动组装
如果需要自定义中间件或更精细的控制，可手动组装：
`ToolRegistry` → `ToolDispatcher` → `SkillCatalog` → `McpHttpServer`。
:::

## Python 绑定

workspace 通过 `maturin` 构建为单一 PyO3 原生扩展（`dcc_mcp_core._core`）；启用哪些可选功能以 `pyproject.toml` 和根目录 `justfile` 为准。

```toml
# pyproject.toml
[project]
requires-python = ">=3.7"
dependencies = []  # 零运行时依赖
```

### Python 包结构

```
python/dcc_mcp_core/
├── __init__.py     # 顶层公开重导出
├── skill.py        # 纯 Python Skill 脚本辅助（无 _core 依赖）
├── result_envelope.py # Typed ToolResult helpers
└── py.typed        # PEP 561 标记

# _core.pyi 是 stub-gen/dev 构建后的生成产物，不是手写源码
```

## 设计决策

### 1. 零运行时 Python 依赖

原生扩展捆绑所有 Rust 代码 — 无需 `pip install` PyO3、tokio 等。这确保了：
- 与 DCC 内嵌 Python 无版本冲突
- 在 Maya/Blender/Houdini/3ds Max 中行为可预测
- 最小导入延迟

### 2. PyO3 0.28+ / Maturin

使用 PyO3，特性：
- `multiple-pymethods` — 每个 struct 多个 `#[pymethods]`
- `abi3-py38` — Python 3.8+ 稳定 ABI（CI 测试 3.7–3.13）
- `extension-module` — 允许从任意 Python 路径加载

### 3. Rust Edition 2024, MSRV 1.95

### 4. Tokio 异步运行时

Rust 异步的事实标准，Windows 命名管道支持出色。

### 5. MessagePack 线协议

紧凑二进制格式，4 字节大端长度前缀 — 语言无关。

### 6. `parking_lot` Mutex

比 `std::sync::Mutex` 更快，且不会在 panic 时中毒。

## 线程安全

所有内部状态使用：
- `parking_lot::Mutex` 用于短期临界区
- `parking_lot::RwLock` 用于读写模式
- 不使用 `std::sync::Mutex` 或 `RwLock`

## 错误处理

使用 `thiserror` 处理错误类型，通过 `#[from]` 实现自动转换。

## 测试策略

- **单元测试**：每个 crate 有内联 `#[cfg(test)]` 模块
- **集成测试**：`tests/` 目录包含 Python + Rust 测试（通过 `cargo test` 和 `pytest`）
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
