# Architecture

DCC-MCP-Core is a Rust workspace with Python bindings via PyO3. The library provides:

- **Zero third-party runtime dependencies** in the Rust core
- **Optional Python bindings** via PyO3 for DCC integration
- **13 modular crates** for selective dependency usage

## Crate Structure

```
dcc-mcp-core (workspace root)
├── dcc-mcp-models       # ActionResultModel, SkillMetadata, DCC types
├── dcc-mcp-actions      # ActionRegistry, EventBus, ActionDispatcher, Pipeline
├── dcc-mcp-skills       # SkillScanner, SkillLoader, SkillWatcher, Resolver
├── dcc-mcp-protocols    # MCP types: ToolDefinition, ResourceDefinition, Prompt
├── dcc-mcp-transport    # IPC, ConnectionPool, SessionManager, FramedChannel
├── dcc-mcp-process      # PyDccLauncher, ProcessMonitor, ProcessWatcher, CrashRecovery
├── dcc-mcp-telemetry    # Tracing/recording: ActionRecorder, TelemetryConfig
├── dcc-mcp-sandbox      # Security: SandboxPolicy, SandboxContext, AuditLog
├── dcc-mcp-shm          # Shared memory: PySharedBuffer, PyBufferPool
├── dcc-mcp-capture      # Screen capture: Capturer, CaptureFrame
├── dcc-mcp-usd          # USD scene description: UsdStage, SdfPath, VtValue
├── dcc-mcp-http         # MCP HTTP server: McpHttpServer, McpHttpConfig
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
dcc-mcp-http ← dcc-mcp-transport, dcc-mcp-protocols
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

**Purpose**: MCP Streamable HTTP server (2025-03-26 spec) for HTTP-based MCP clients.

**Key Components**:
- `McpHttpServer` — Background-thread HTTP server (axum/Tokio)
- `McpHttpConfig` — Server configuration (port, CORS, request timeout)
- `ServerHandle` — Server handle for URL retrieval and graceful shutdown

**Dependencies**: `axum`, `tokio`

### dcc-mcp-utils

**Purpose**: Shared utility functions and constants.

**Modules**:
- `filesystem` — Platform-specific directories via `dirs` crate
- `type_wrappers` — RPyC-safe wrappers (BooleanWrapper, IntWrapper, FloatWrapper, StringWrapper)
- `constants` — App metadata and environment variable names

**Dependencies**: `dirs`

## Python Bindings

All 13 crates are compiled into a single PyO3 native extension (`dcc_mcp_core._core`) via `maturin`.

```toml
# pyproject.toml
[project]
requires-python = ">=3.7"
dependencies = []  # Zero runtime dependencies
```

### Python Package Structure

```
python/dcc_mcp_core/
├── __init__.py     # Public API (re-exports ~130 symbols from _core)
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
