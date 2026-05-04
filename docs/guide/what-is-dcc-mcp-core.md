# What is DCC-MCP-Core?

DCC-MCP-Core is a **foundational Rust library with Python bindings** for the DCC (Digital Content Creation) Model Context Protocol (MCP) ecosystem. It enables AI assistants to interact with DCC software (Maya, Blender, Houdini, 3ds Max, etc.) through a unified, high-performance interface.

Built with a **Rust core** exposed to Python via [PyO3](https://pyo3.rs/) and compiled by [maturin](https://github.com/PyO3/maturin), it combines the performance and thread safety of Rust with the accessibility of Python.

## Core Workflow

```mermaid
flowchart LR
    AI([AI Assistant]):::aiNode
    MCP{{MCP Server}}:::serverNode
    Core{{DCC-MCP-Core}}:::coreNode
    DCC[/DCC Software/]:::dccNode

    AI -->|1. Request| MCP
    MCP -->|2. Discover Actions| Core
    Core -->|3. Execute in DCC| DCC
    DCC -->|4. Result| Core
    Core -->|5. Structured Result| MCP
    MCP -->|6. Response| AI

    classDef aiNode fill:#f9d,stroke:#f06,stroke-width:2px,color:#333
    classDef serverNode fill:#bbf,stroke:#66f,stroke-width:2px,color:#333
    classDef coreNode fill:#fbb,stroke:#f66,stroke-width:2px,color:#333
    classDef dccNode fill:#bfb,stroke:#6b6,stroke-width:2px,color:#333
```

## Key Features

- **ToolRegistry** — Thread-safe action registration, search, and versioning
- **SkillCatalog** — Progressive skill discovery and loading; scripts auto-registered as MCP tools via SKILL.md (Skills-First architecture since v0.12.10)
- **EventBus** — Publish/subscribe system for DCC lifecycle events
- **MCP HTTP Server** — Streamable HTTP server (2025-03-26 spec) for serving MCP tools to AI clients
- **MCP Protocol Types** — Type-safe definitions for Tools, Resources, Prompts, and Annotations
- **Transport Layer** — Connection pooling, service discovery, and session management for DCC communication (TCP, Named Pipes, Unix Sockets)
- **Shared Memory** — Zero-copy scene data transfer between DCC and agent processes
- **Process Management** — DCC process launch, monitoring, and crash recovery
- **Telemetry** — Tracing and metrics infrastructure via OpenTelemetry
- **Sandbox** — Security policy, input validation, and audit logging for AI actions
- **Capture** — DCC viewport screenshot capture
- **USD Bridge** — Scene exchange via OpenUSD stage representation
- **Type Wrappers** — RPyC-safe wrappers (BooleanWrapper, IntWrapper, FloatWrapper, StringWrapper)
- **Zero Python Dependencies** — Pure Rust core compiled to a native Python extension

## Architecture

DCC-MCP-Core is a Rust workspace with **30 workspace members** (29 functional crates + `workspace-hack`), compiled into a single Python extension module `dcc_mcp_core._core` via maturin:

```
dcc-mcp-core/
├── src/lib.rs                  # PyO3 module entry point (_core)
├── crates/
│   ├── dcc-mcp-models/         # ToolResult, SkillMetadata, DccName, shared errors
│   ├── dcc-mcp-actions/        # ToolRegistry, EventBus, Dispatcher, Validator
│   ├── dcc-mcp-skills/         # SkillScanner, SkillCatalog, SkillWatcher, Resolver
│   ├── dcc-mcp-protocols/      # MCP Tool/Resource/Prompt/DccAdapter models
│   ├── dcc-mcp-jsonrpc/        # MCP 2025-03-26 JSON-RPC wire types
│   ├── dcc-mcp-job/            # Async jobs + optional persistence
│   ├── dcc-mcp-skill-rest/     # Per-DCC /v1/* REST skill API
│   ├── dcc-mcp-gateway/        # Multi-DCC gateway + capability index
│   ├── dcc-mcp-http/           # Embedded MCP Streamable HTTP server core
│   ├── dcc-mcp-server/         # Binary entry point, gateway runner
│   ├── dcc-mcp-logging/        # Rolling file logging
│   ├── dcc-mcp-paths/          # Platform path helpers
│   ├── dcc-mcp-pybridge*/      # PyO3 helper crates
│   ├── dcc-mcp-transport/      # IPC transport and channel adapters
│   ├── dcc-mcp-process/        # Launch, monitor, crash recovery
│   ├── dcc-mcp-telemetry/      # Tool metrics and recorders
│   ├── dcc-mcp-sandbox/        # SandboxPolicy, validation, audit log
│   ├── dcc-mcp-shm/            # Shared memory buffers
│   ├── dcc-mcp-capture/        # Screen/window capture
│   ├── dcc-mcp-usd/            # USD scene description bridge
│   ├── dcc-mcp-workflow/       # WorkflowCatalog and YAML workflows
│   ├── dcc-mcp-scheduler/      # ScheduleSpec, TriggerSpec, scheduler service
│   ├── dcc-mcp-artefact/       # FileRef and content-addressed handoff
│   ├── dcc-mcp-host/           # Host execution bridge contracts
│   └── dcc-mcp-tunnel-*/       # Remote MCP tunnel protocol, relay, and agent
└── python/
    └── dcc_mcp_core/
        ├── __init__.py          # Top-level public re-exports
        ├── skill.py             # Pure-Python skill script helpers
        ├── result_envelope.py   # Typed ToolResult helpers
        └── py.typed             # PEP 561 marker
        # _core.pyi is generated after a stub-gen/dev build, not checked in
```

## Python API Surface

All public APIs are available from the top-level `dcc_mcp_core` package. Use `llms.txt` for the compact AI-facing index and `llms-full.txt` for the complete index before falling back to source inspection:

```python
from dcc_mcp_core import (
    # Actions
    ToolRegistry, ToolDispatcher, ToolPipeline, ToolValidator,
    ToolRecorder, ToolMetrics, EventBus,
    ToolResult, success_result, error_result,

    # Skills — Skills-First architecture
    SkillCatalog, SkillSummary, SkillMetadata, ToolDeclaration,
    SkillScanner, SkillWatcher, scan_and_load,

    # MCP HTTP Server
    McpHttpServer, McpHttpConfig,

    # Transport
    IpcChannelAdapter, GracefulIpcChannelAdapter, SocketServerAdapter, DccLinkFrame,

    # Protocols
    ToolDefinition, ToolAnnotations, ResourceDefinition, PromptDefinition,

    # Shared Memory
    PySharedSceneBuffer, PySharedBuffer, PyBufferPool,

    # Process
    PyDccLauncher, PyProcessWatcher, PyCrashRecoveryPolicy,

    # Telemetry
    TelemetryConfig, is_telemetry_initialized,

    # Sandbox
    SandboxPolicy, SandboxContext, InputValidator, AuditLog,

    # Capture
    Capturer, CaptureFrame,

    # USD
    UsdStage, UsdPrim, VtValue, SdfPath,
)
```

See the [API Reference](/api/actions) for complete documentation of every symbol.

## Version & Python Support

- **Current version**: 0.14.27 <!-- x-release-please-version -->
- **Python**: 3.7–3.13 (abi3-py38 wheel, tested in CI across all versions)
- **Rust**: Edition 2024, MSRV 1.85
- **Build**: maturin + PyO3; zero runtime Python dependencies

## Related Projects

- [dcc-mcp-maya](https://github.com/loonghao/dcc-mcp-maya) — Maya MCP server implementation
- [dcc-mcp-unreal](https://github.com/loonghao/dcc-mcp-unreal) — Unreal Engine adapter (in development)
- [dcc-mcp-photoshop](https://github.com/loonghao/dcc-mcp-photoshop) — Photoshop UXP WebSocket bridge (in development)
- [dcc-mcp-zbrush](https://github.com/loonghao/dcc-mcp-zbrush) — ZBrush HTTP REST bridge (in development)
- [dcc-mcp-rpyc](https://github.com/loonghao/dcc-mcp-rpyc) — RPyC bridge for remote DCC operations
