# Architecture

DCC-MCP-Core is a **Rust-powered DCC automation framework** with Python bindings via PyO3, designed for AI-assisted workflows. It solves MCP's context explosion and provides zero-code skill registration.

## High-Level Design

### Three-Layer Stack

```
┌──────────────────────────────┐
│ AI Agent (Claude, GPT, etc.) │ ← talks MCP via HTTP
└──────────────┬───────────────┘
               │ MCP Streamable HTTP
               ▼
┌──────────────────────────────┐
│ Gateway Server (Rust core)   │ ← coordinates tool discovery
│ - Version-aware election     │   and session routing
│ - Session isolation          │
│ - Tool scoping               │
└──────────┬──────────────────┘
           │ IPC (fast, zero-copy)
           │
     ┌─────┴──────┬────────┬────────┐
     ▼            ▼        ▼        ▼
  ┌──────┐   ┌──────┐  ┌──────┐  ┌──────┐
  │ Maya │   │Blend │  │Houd  │  │Photo │
  │ (v25)│   │ (3.9)│  │(21)  │  │(2025)│
  └──────┘   └──────┘  └──────┘  └──────┘
   Instances with embedded Skills system
```

### Core Principles

1. **Session Isolation** — Each AI session pinned to one DCC instance (prevents context explosion)
2. **Version-Aware Election** — Newest DCC automatically becomes gateway (no manual failover)
3. **Zero-Code Skills** — SKILL.md + scripts/ = instant MCP tools (no Python glue)
4. **Structured Results** — Every tool returns `{success, message, context, next_steps}` (AI-friendly)
5. **Progressive Discovery** — Tools scoped by DCC/scope/product (71% context reduction)

## The Library

DCC-MCP-Core is a Rust workspace with Python bindings via PyO3. It provides:

- **Zero third-party runtime dependencies** in the Rust core
- **Optional Python bindings** via PyO3 for DCC integration
- **14 modular crates** for selective dependency usage

## Crate Structure

```
dcc-mcp-core (workspace root)
├── dcc-mcp-models       # ActionResultModel, SkillMetadata, DCC types
├── dcc-mcp-actions      # ActionRegistry, EventBus, ActionDispatcher, Pipeline
├── dcc-mcp-skills       # SkillScanner, SkillCatalog, SkillWatcher, Resolver
├── dcc-mcp-protocols    # MCP types: ToolDefinition, ResourceDefinition, Prompt, DccAdapter, BridgeKind
├── dcc-mcp-transport    # IPC, ConnectionPool, FileRegistry, FramedChannel
├── dcc-mcp-process      # PyDccLauncher, ProcessMonitor, ProcessWatcher, CrashRecovery
├── dcc-mcp-telemetry    # Tracing/recording: ActionRecorder, TelemetryConfig
├── dcc-mcp-sandbox      # Security: SandboxPolicy, SandboxContext, AuditLog
├── dcc-mcp-shm          # Shared memory: PySharedBuffer, PyBufferPool
├── dcc-mcp-capture      # Screen capture: Capturer, CaptureFrame
├── dcc-mcp-usd          # USD scene description: UsdStage, SdfPath, VtValue
├── dcc-mcp-http         # MCP HTTP server: McpHttpServer, McpHttpConfig, Gateway
├── dcc-mcp-server       # Binary entry point: dcc-mcp-server, gateway runner
└── dcc-mcp-utils       # Filesystem, type wrappers, constants
```

### Dependency Graph

```
dcc-mcp-models (base types)
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

## Crate Responsibilities

### dcc-mcp-models

**Purpose**: Core data models and type definitions shared across all crates.

**Key Types**:
- `ActionResultModel` — Unified result type for action executions
- `SkillMetadata` — Parsed skill package metadata
- `SceneInfo`, `SceneStatistics` — DCC scene information
- `DccInfo`, `DccCapabilities`, `DccError` — DCC adapter types
- `ScriptResult`, `CaptureResult` — Operation results

**Dependencies**: None (base crate)

### dcc-mcp-actions

**Purpose**: Centralized action registry, validation, dispatch, and pipeline system.

**Key Components**:
- `ActionRegistry` — Thread-safe registry: register/get/search/list/unregister actions
- `ActionDispatcher` — Typed dispatch with validation to registered Python callables
- `ActionValidator` — JSON Schema-based parameter validation
- `ActionPipeline` — Middleware pipeline (logging, timing, audit, rate limiting)
- `EventBus` — Pub/sub event system for DCC lifecycle events
- `VersionedRegistry` — Multi-version action registry with SemVer constraint resolution

**Key Traits**: None — actions are plain Python callables registered via `ActionDispatcher.register_handler()`

**Dependencies**: `dcc-mcp-models`

### dcc-mcp-skills

**Purpose**: Zero-code skill package discovery, loading, and hot-reload via filesystem watching.

**Key Components**:
- `SkillScanner` — mtime-cached directory scanner for SKILL.md packages
- `SkillCatalog` — Progressive skill loading with on-demand discovery (register actions on `load_skill`)
- `SkillWatcher` — Platform-native filesystem watcher (inotify/FSEvents/ReadDirectoryChangesW)
- `SkillMetadata` — Parsed metadata from SKILL.md frontmatter
- Dependency resolution: `resolve_dependencies`, `expand_transitive_dependencies`, `validate_dependencies`

**Skill Package Format**: `SKILL.md` with YAML frontmatter (`name`, `version`, `description`, `tools`, `dcc`, `tags`, `depends`)

**Dependencies**: `dcc-mcp-actions`, `dcc-mcp-models`

### dcc-mcp-protocols

**Purpose**: MCP (Model Context Protocol) type definitions per 2025-03-26 spec.

**Key Types**:
- `ToolDefinition`, `ToolAnnotations` — MCP tool schema with behavior hints
- `ResourceDefinition`, `ResourceTemplateDefinition`, `ResourceAnnotations` — MCP resource schema
- `PromptDefinition`, `PromptArgument` — MCP prompt schema
- `DccAdapter` — DCC adapter capability descriptor
- `BridgeKind` — Bridge type enum (Http, WebSocket, NamedPipe, Custom) for non-Python DCCs

**Dependencies**: `dcc-mcp-models`

### dcc-mcp-transport

**Purpose**: IPC and network transport layer with service discovery, sessions, and connection pooling.

**Transport Types**:
- **IPC**: Unix sockets (Linux/macOS) / Windows named pipes — sub-millisecond, PID-unique
- **TCP**: Network sockets — cross-machine or fallback

**Key Components**:
- `TransportManager` — High-level manager: service registry, session pool, routing
- `IpcListener` / `ListenerHandle` — Server-side IPC listener with connection tracking
- `FramedChannel` — Full-duplex framed channel with background reader loop
- `TransportAddress` — Protocol-agnostic endpoint (TCP, named pipe, unix socket)
- `CircuitBreaker` — Failure detection and fast-drop

**Wire Protocol**: MessagePack with 4-byte big-endian length prefix

**Dependencies**: `dcc-mcp-protocols`, `tokio`

### dcc-mcp-process

**Purpose**: Cross-platform DCC process lifecycle management and crash recovery.

**Key Components**:
- `PyDccLauncher` — Async spawn/terminate/kill of DCC processes
- `PyProcessMonitor` — CPU/memory monitoring via `sysinfo`
- `PyProcessWatcher` — Background event-polling watcher with heartbeat/status tracking
- `PyCrashRecoveryPolicy` — Exponential/fixed backoff restart policy

**Dependencies**: `tokio`, `sysinfo`

### dcc-mcp-telemetry

**Purpose**: Distributed tracing and metrics collection.

**Key Components**:
- `ActionRecorder` / `RecordingGuard` — RAII timing guard for action executions
- `ActionMetrics` — Read-only snapshot of per-action metrics (count, success rate, p95/p99 latency)
- `TelemetryConfig` — Builder for global telemetry provider (stdout/JSON exporter)

**Dependencies**: `tracing`, `metrics`

### dcc-mcp-sandbox

**Purpose**: Security policy enforcement, audit logging, and input validation.

**Key Components**:
- `SandboxPolicy` — API whitelist, path allowlist, execution constraints (timeout, max actions, read-only)
- `SandboxContext` — Per-session execution context bundling policy + audit log
- `AuditLog` / `AuditEntry` — Structured audit trail per action invocation
- `InputValidator` — Schema-based validation with injection-guard pattern matching

**Dependencies**: None

### dcc-mcp-shm

**Purpose**: Zero-copy shared memory buffers for high-frequency DCC ↔ Agent data exchange.

**Key Components**:
- `PySharedBuffer` — Named memory-mapped file buffer with cross-process handoff
- `PyBufferPool` — Fixed-capacity pool of reusable buffers (amortises mmap overhead at 30 fps)
- `PySharedSceneBuffer` — High-level wrapper with inline vs. chunked storage (>256 MiB split)

**Compression**: LZ4 optional on write; auto-decompress on read

**Dependencies**: `lz4`

### dcc-mcp-capture

**Purpose**: GPU framebuffer screenshot and viewport capture for DCC applications.

**Backends**:
- **Windows**: DXGI Desktop Duplication API — GPU direct access, <16ms per frame
- **Linux**: X11 XShmGetImage
- **Fallback**: Mock synthetic backend (CI / headless)

**Key Components**:
- `Capturer` — Auto-backend-selection entry point (`new_auto()` / `new_mock()`)
- `CaptureFrame` — Captured image data with PNG/JPEG/raw BGRA encoding

**Dependencies**: Platform-specific (windows-capture, x11grab, etc.)

### dcc-mcp-usd

**Purpose**: USD scene description data model and serialization (pure Rust, no OpenUSD C++ dependency).

**Key Components**:
- `UsdStage` — Main stage container with prim management and metadata
- `UsdPrim` — Prim with attribute get/set and API schema checking
- `SdfPath` — Scene graph path with absolute/relative resolution
- `VtValue` — Variant value container (bool, int, float, string, vec3f, asset, token)

**Serialization**: USDA (human-readable) and JSON (compact, for IPC)

**Bridge Functions**: `scene_info_json_to_stage`, `stage_to_scene_info_json`, `units_to_mpu`, `mpu_to_units`

**Dependencies**: `pxr-usd` (thin wrapper, no C++ runtime)

### dcc-mcp-http

**Purpose**: MCP Streamable HTTP server (2025-03-26 spec) for HTTP-based MCP clients, with optional gateway competition.

**Key Components**:
- `McpHttpServer` — Background-thread HTTP server (axum/Tokio)
- `McpHttpConfig` — Server configuration (port, CORS, request timeout, gateway fields)
- `ServerHandle` — Server handle with URL retrieval, `is_gateway` flag, and graceful shutdown
- `GatewayRunner` — First-wins port competition orchestrator
- `GatewayConfig` — Gateway configuration (port, stale timeout, heartbeat interval)
- `GatewayHandle` — Handle indicating whether this process won the gateway port
- `GatewayState` — Shared gateway state (registry, stale timeout, HTTP client for proxying)

**McpHttpConfig Gateway Fields**:
- `gateway_port` — Port to compete for (0 = disabled, default 0)
- `registry_dir` — Shared FileRegistry directory
- `stale_timeout_secs` — Seconds without heartbeat before instance is stale
- `heartbeat_secs` — Heartbeat interval in seconds
- `dcc_type` / `dcc_version` / `scene` — Instance metadata for gateway routing

**SSE Support**: `GET /mcp` long-lived SSE stream for server-push events

**Dependencies**: `axum`, `tokio`, `reqwest`, `socket2`, `dcc-mcp-transport`, `dcc-mcp-protocols`, `dcc-mcp-actions`, `dcc-mcp-skills`

### dcc-mcp-server

**Purpose**: Binary entry point (`dcc-mcp-server` CLI) that assembles and runs the full MCP server with gateway support.

**Key Components**:
- `main.rs` — CLI entry point using `GatewayRunner` and `McpHttpServer` library APIs

**Dependencies**: `dcc-mcp-http`

### dcc-mcp-utils

**Purpose**: Shared utility functions and constants.

**Modules**:
- `filesystem` — Platform-specific directories via `dirs` crate
- `type_wrappers` — RPyC-safe wrappers (BooleanWrapper, IntWrapper, FloatWrapper, StringWrapper)
- `constants` — App metadata and environment variable names

**Dependencies**: `dirs`

## Skills-First Architecture

The recommended entry-point for exposing DCC tools over MCP is the **Skills-First** pattern using `create_skill_manager`. A single call wires together the full stack:

```
create_skill_manager("maya")
        │
        ├─ ActionRegistry  (thread-safe action store)
        ├─ ActionDispatcher (routes calls to Python handlers)
        ├─ SkillCatalog    (discovers + loads SKILL.md packages)
        │       └─ scans DCC_MCP_MAYA_SKILL_PATHS + DCC_MCP_SKILL_PATHS
        └─ McpHttpServer   (returns ready-to-start HTTP server)
```

```python
import os
os.environ["DCC_MCP_MAYA_SKILL_PATHS"] = "/studio/maya-skills"

from dcc_mcp_core import create_skill_manager, McpHttpConfig

server = create_skill_manager("maya", McpHttpConfig(port=8765))
handle = server.start()
print(f"Maya MCP server: {handle.mcp_url()}")
# handle.shutdown() when done
```

**Skill path resolution order** (first found wins):
1. `DCC_MCP_{APP}_SKILL_PATHS` — per-app env var (e.g. `DCC_MCP_MAYA_SKILL_PATHS`)
2. `DCC_MCP_SKILL_PATHS` — global fallback
3. Platform data dir: `~/.local/share/dcc-mcp/skills/{app}/`
4. `extra_paths` argument

::: tip Manual Assembly
If you need custom middleware or fine-grained control, assemble the stack manually:
`ActionRegistry` → `ActionDispatcher` → `SkillCatalog` → `McpHttpServer`.
:::

## Python Bindings

All 14 crates are compiled into a single PyO3 native extension (`dcc_mcp_core._core`) via `maturin`.

```toml
# pyproject.toml
[project]
requires-python = ">=3.7"
dependencies = []  # Zero runtime dependencies
```

### Python Package Structure

```
python/dcc_mcp_core/
├── __init__.py     # Public API (re-exports ~140 symbols from _core)
├── _core.pyi       # Type stubs (auto-generated from Rust)
└── py.typed        # PEP 561 marker
```

## Design Decisions

### 1. Zero Runtime Python Dependencies

The native extension bundles all Rust code — no `pip install` for PyO3, tokio, etc. This ensures:
- No version conflicts with DCC's embedded Python
- Predictable behavior across Maya/Blender/Houdini/3ds Max
- Minimal import latency

### 2. PyO3 0.22+ / Maturin

Using PyO3 with:
- `multiple-pymethods` — Multiple `#[pymethods]` per struct
- `abi3-py38` — Stable ABI for Python 3.8+ (CI tests 3.7–3.13)
- `extension-module` — Allow loading from any Python path

### 3. Rust Edition 2024, MSRV 1.85

### 4. Tokio for Async Runtime

Industry standard with excellent Windows named-pipes support.

### 5. MessagePack Wire Protocol

Compact binary format with 4-byte big-endian length prefix — language agnostic.

### 6. `parking_lot` Mutex

Faster than `std::sync::Mutex` and doesn't poison on panic.

## Thread Safety

All internal state uses:
- `parking_lot::Mutex` for short critical sections
- `parking_lot::RwLock` for reader-writer patterns
- No `std::sync::Mutex` or `RwLock`

## Error Handling

Using `thiserror` for error types with `#[from]` for automatic conversion.

## Testing Strategy

- **Unit tests**: Each crate has inline `#[cfg(test)]` modules
- **Integration tests**: `tests/` directory with Python + Rust tests (via `cargo test` and `pytest`)
- **Coverage tracking**: `cargo-llvm-cov` + `pytest --cov`

## Build Commands

| Command | Tool | Purpose |
|---------|------|---------|
| `cargo check` | cargo | Fast syntax/type check |
| `cargo clippy` | clippy | Lint with `-D warnings` (CI strict) |
| `cargo fmt --check` | rustfmt | Format check |
| `maturin develop` | maturin | Install wheel in dev mode |
| `cargo test --workspace` | cargo | Run all Rust tests |
| `pytest tests/` | pytest | Run Python integration tests |
