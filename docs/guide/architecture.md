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
- **30 workspace members** (29 functional crates + `workspace-hack`) for selective dependency usage; root `Cargo.toml` is the source of truth

## Crate Structure

```
dcc-mcp-core (workspace root)
├── dcc-mcp-naming        # SEP-986 tool-name / action-id validators
├── dcc-mcp-models        # ToolResult, SkillMetadata, DccName, shared errors
├── dcc-mcp-actions       # ToolRegistry, EventBus, ToolDispatcher, validation
├── dcc-mcp-skills        # SkillScanner, SkillCatalog, SkillWatcher, resolver
├── dcc-mcp-protocols     # MCP-facing Tool/Resource/Prompt/DccAdapter models
├── dcc-mcp-jsonrpc       # MCP 2025-03-26 JSON-RPC wire types
├── dcc-mcp-job           # Async job tracking + optional persistence
├── dcc-mcp-skill-rest    # Per-DCC /v1/* REST skill API
├── dcc-mcp-gateway       # Multi-DCC gateway + dynamic capability index
├── dcc-mcp-http          # Embedded MCP Streamable HTTP server core
├── dcc-mcp-server        # Binary entry point and gateway runner
├── dcc-mcp-logging       # Rolling file logging
├── dcc-mcp-paths         # Platform path helpers
├── dcc-mcp-pybridge      # PyO3 helper macros / JSON / YAML bridge
├── dcc-mcp-pybridge-derive # derive macros for PyO3 helpers
├── dcc-mcp-transport     # IPC transport, frames, channel adapters
├── dcc-mcp-process       # Launch, monitor, watcher, crash recovery
├── dcc-mcp-telemetry     # Tool metrics and recorders
├── dcc-mcp-sandbox       # SandboxPolicy, validation, audit log
├── dcc-mcp-shm           # Shared memory buffers
├── dcc-mcp-capture       # Screen/window capture
├── dcc-mcp-usd           # USD scene description bridge
├── dcc-mcp-workflow      # WorkflowCatalog and YAML workflows
├── dcc-mcp-scheduler     # ScheduleSpec, TriggerSpec, scheduler service
├── dcc-mcp-artefact      # FileRef and content-addressed handoff
├── dcc-mcp-host          # Host execution bridge / adapter-facing contracts
├── dcc-mcp-tunnel-protocol # Remote MCP tunnel protocol + auth
├── dcc-mcp-tunnel-relay  # RelayServer
├── dcc-mcp-tunnel-agent  # Local tunnel sidecar
└── workspace-hack        # Workspace dependency deduplication
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
- `ToolResult` — Unified result type for tool executions
- `SkillMetadata` — Parsed skill package metadata
- `SceneInfo`, `SceneStatistics` — DCC scene information
- `DccInfo`, `DccCapabilities`, `DccError` — DCC adapter types
- `ScriptResult`, `CaptureResult` — Operation results

**Dependencies**: None (base crate)

**Maintainer layout**:
- `skill_metadata/mod.rs` now focuses on the public struct surface, while runtime helpers, serde parsing helpers, and Python bindings live in focused sibling modules.
- `skill_metadata/tool_declaration.rs` keeps the declaration model and serde rules, while the PyO3 accessor surface lives in a dedicated sibling module.
- This keeps spec-facing fields easy to scan without mixing frontmatter parsing, ClawHub helpers, and PyO3 accessors in one block.

### dcc-mcp-actions

**Purpose**: Centralized action registry, validation, dispatch, and pipeline system.

**Key Components**:
- `ToolRegistry` — Thread-safe registry: register/get/search/list/unregister tools
- `ToolDispatcher` — Typed dispatch with validation to registered Python callables
- `ToolValidator` — JSON Schema-based parameter validation
- `ToolPipeline` — Middleware pipeline (logging, timing, audit, rate limiting)
- `EventBus` — Pub/sub event system for DCC lifecycle events
- `VersionedRegistry` — Multi-version action registry with SemVer constraint resolution

**Key Traits**: None — actions are plain Python callables registered via `ToolDispatcher.register_handler()`

**Dependencies**: `dcc-mcp-models`

**Maintainer layout**:
- `registry/mod.rs` keeps the core registry behavior, while `ActionMeta` and the Python binding shim live in focused sibling modules.
- `chain.rs` is a thin facade: step/result types, placeholder interpolation, the `ActionChain` fluent builder and executor, and unit tests each live in dedicated sibling modules (`chain_types.rs`, `chain_interpolate.rs`, `chain_exec.rs`, `chain_tests.rs`).
- This separates Rust-side lookup/update semantics from PyO3 translation code and makes metadata evolution easier to review.

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

**Maintainer layout**:
- `catalog/catalog.rs` now focuses on query/read APIs, while discovery/bootstrap and load/unload lifecycle paths live in dedicated implementation files.
- `loader/mod.rs` stays centered on single-skill `SKILL.md` parsing, while batch scan/load orchestration and filesystem enumeration helpers live in sibling modules.
- `validator.rs` is a thin facade now; report types, validation rules, Python bindings, and unit tests each live in focused siblings.
- `watcher.rs` is a thin facade around the `SkillWatcher` public surface; shared `WatcherInner` state and the `WatcherError` type live in `watcher_inner.rs`, the `should_reload` / `is_skill_related` FS filters live in `watcher_filter.rs`, the PyO3 wrapper lives in `watcher_python.rs`, and unit tests live in `watcher_tests.rs`.
- This keeps search/ranking work separate from registry mutation and script-handler registration, which lowers the cognitive load for future refactors.

### dcc-mcp-protocols

**Purpose**: MCP (Model Context Protocol) type definitions per 2025-03-26 spec.

**Key Types**:
- `ToolDefinition`, `ToolAnnotations` — MCP tool schema with behavior hints
- `ResourceDefinition`, `ResourceTemplateDefinition`, `ResourceAnnotations` — MCP resource schema
- `PromptDefinition`, `PromptArgument` — MCP prompt schema
- `DccAdapter` — DCC adapter capability descriptor
- `BridgeKind` — Bridge type enum (Http, WebSocket, NamedPipe, Custom) for non-Python DCCs

**Dependencies**: `dcc-mcp-models`

**Maintainer layout**:
- `types.rs` is now a thin re-export surface; tool/resource/prompt models live in focused internal modules.
- `mock/config.rs` keeps the public `MockConfig` API while defaults, builder methods, and DCC presets live in separate implementation files.
- `mock/adapter.rs` keeps shared state and helpers while trait implementations are split by capability (`connection`, `scene_manager`, `transform`, `hierarchy`, etc.).

### dcc-mcp-transport

**Purpose**: IPC and network transport layer with service discovery, sessions, and connection pooling.

**Transport Types**:
- **IPC**: Unix sockets (Linux/macOS) / Windows named pipes — sub-millisecond, PID-unique
- **TCP**: Network sockets — cross-machine or fallback

**Key Components**:
- `IpcChannelAdapter` — Client/server IPC adapter using DccLink frames over ipckit
- `SocketServerAdapter` — Multi-client TCP/UDS listener for server-side IPC
- `DccLinkFrame` — Binary frame type (msg_type, seq, body) for DccLink wire protocol
- `TransportAddress` — Protocol-agnostic endpoint (TCP, named pipe, unix socket)

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

**Maintainer layout (dcc-mcp-shm)**:
- `src/buffer.rs` is trimmed from 720 to 553 lines by moving the `#[cfg(test)] mod tests { ... }` block (61 integration-style tests across `test_create`, `test_open`, `test_gc`, `test_descriptor` submodules) into a sibling `buffer_tests.rs`. Mounted via `#[cfg(test)] #[path = "buffer_tests.rs"] mod tests;`. Production types (`SharedBuffer`, `BufferDescriptor`, `gc_orphans`) and all private helpers stay co-located to retain access to `SharedBuffer::inner` / `read_header` etc.

**Maintainer layout (dcc-mcp-actions pipeline)**:
- `src/pipeline/python.rs` becomes a 67-line facade that mounts four siblings via `#[path]`: `python_helpers` (`value_to_py`, `PyCallableHook`), `python_middleware` (`PyLoggingMiddleware`, `PyTimingMiddleware`, `PyAuditMiddleware`, `PyRateLimitMiddleware` — inner fields `pub(super)` so the pipeline can construct them), `python_shared` (`Shared{Timing,Audit,RateLimit}Middleware` `Arc` newtypes implementing `ActionMiddleware`), `python_pipeline` (`PyActionPipeline` — the Python-facing `ToolPipeline`). Every Python class is re-exported so `pipeline::python::{PyActionPipeline, PyLoggingMiddleware, …}` keeps working. The `Shared*` newtypes are re-exported under `#[cfg(test)]` so the existing `python_tests.rs` unit tests can reference `super::python::Shared*` unchanged.

**Maintainer layout (dcc-mcp-transport)**:
- `src/discovery/file_registry.rs` keeps the `FileRegistry` struct and every `impl FileRegistry` method in place (private fields would otherwise require workarounds); the 298-line `#[cfg(test)] mod tests` block is extracted into `file_registry_tests.rs` and mounted via `#[cfg(test)] #[path = "file_registry_tests.rs"] mod tests;`. File drops from 759 to 463 lines with no behaviour change.

**Maintainer layout (dcc-mcp-usd)**:
- `src/types.rs` is a thin facade over the six core USD data types. `SdfPath` lives in `types_sdf_path.rs`, `VtValue` in `types_vt_value.rs`, `UsdAttribute` in `types_attribute.rs`, `UsdPrim` (+ the `default_true` serde helper) in `types_prim.rs`, `UsdLayer` (+ the `default_y_axis` / `default_mpu` serde helpers) in `types_layer.rs`, and `UsdStageMetrics` in `types_metrics.rs`. Unit tests live in `types_tests.rs`. The facade re-exports every type so `dcc_mcp_usd::types::{SdfPath, VtValue, UsdAttribute, UsdPrim, UsdLayer, UsdStageMetrics}` keeps working unchanged.

**Maintainer layout (dcc-mcp-skills resolver)**:
- `src/resolver.rs` keeps the production dependency-resolution logic (`resolve_dependencies`, `resolve_skill_order`, `topological_sort`, cycle detection) in place; the 321-line `#[cfg(test)] mod tests` block — covering happy paths, missing deps, cycles, diamond graphs, and edge cases — is extracted into a sibling `resolver_tests.rs` and mounted via `#[cfg(test)] #[path = "resolver_tests.rs"] mod tests;`. File drops from 685 to 365 lines.

**Maintainer layout (dcc-mcp-artefact)**:
- `src/lib.rs` retains `FileRef`, `ArtefactStore` trait, `FilesystemArtefactStore`, `InMemoryArtefactStore`, and all `put_*` / `resolve` helpers. The `#[cfg(test)] mod tests` block (round-trip JSON, idempotency, TTL, filesystem persistence) lives in `lib_tests.rs` and is mounted via `#[cfg(test)] #[path = "lib_tests.rs"] mod tests;`.

**Maintainer layout (dcc-mcp-scheduler)**:
- `src/service.rs` keeps `SchedulerService`, `SchedulerHandle`, `SchedulerConfig`, and cron/webhook trigger dispatch in place. The `#[cfg(test)] mod tests` block (cron-spec coverage, `max_concurrent` gating, HMAC verification) is extracted into `service_tests.rs` and mounted via `#[cfg(test)] #[path = "service_tests.rs"] mod tests;`.

**Maintainer layout (dcc-mcp-capture python)**:
- `src/python.rs` retains every `#[pyclass]` / `#[pymethods]` binding (`PyCapturer`, `PyCaptureFrame`, `PyWindowFinder`, `PyWindowInfo`, `PyCaptureTarget`). The test module — exercising the Mock backend and capability detection — is extracted into `python_tests.rs` and mounted via `#[cfg(test)] #[path = "python_tests.rs"] mod tests;`.

**Maintainer layout**:
- `src/python.rs` is a thin facade over the PyO3 bindings: the shared Tokio runtime handle, `ProcessError → PyErr` adaptor, and `ProcessStatus → &'static str` serialiser live in `python_helpers.rs`; each Python-facing class lives in its own focused sibling — `python_monitor.rs` (`PyProcessMonitor`), `python_launcher.rs` (`PyDccLauncher`), `python_crash_policy.rs` (`PyCrashRecoveryPolicy` + the `parse_status` helper), `python_watcher.rs` (`PyProcessWatcher` + the internal `PyWatcherEvent` event type), `python_standalone_dispatcher.rs` (`PyStandaloneDispatcher`), and `python_pumped_dispatcher.rs` (`PyPumpedDispatcher` + the `parse_affinity` / `outcome_to_dict` helpers). The facade re-exports every `Py*` class and keeps `register_classes` as the single registration entry point.

### dcc-mcp-telemetry

**Purpose**: Distributed tracing and metrics collection.

**Key Components**:
- `ToolRecorder` / `RecordingGuard` — RAII timing guard for tool executions
- `ToolMetrics` — Read-only snapshot of per-tool metrics (count, success rate, p95/p99 latency)
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
- `McpServerHandle` — Server handle with URL retrieval, `is_gateway` flag, and graceful shutdown
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

**Connection-Scoped Cache** (issue #438): Per-session `tools/list` snapshot stored on `McpSession`. On cache hit, avoids redundant registry scan, bare-name resolution, and `McpTool` construction. Invalidated when the `AppState::registry_generation` counter is bumped (skill load/unload, group activation/deactivation). Configurable via `McpHttpConfig.enable_tool_cache` (default `True`).

**Dependencies**: `axum`, `tokio`, `reqwest`, `socket2`, `dcc-mcp-transport`, `dcc-mcp-protocols`, `dcc-mcp-actions`, `dcc-mcp-skills`

**Maintainer layout**:
- `src/tests/gateway.rs` is a shared fixture module; gateway tests are split into focused submodules for REST, MCP methods, batch handling, session headers, subscriptions, runner competition, and pagination.
- Legacy unreferenced `segment_*` test fragments were removed so the crate test tree mirrors real runtime responsibilities.
- `src/handlers/tools_call.rs` is now a thin facade; request resolution, async job dispatch, sync execution, and result shaping live in focused helper modules.
- `src/gateway/handlers.rs` is a routing facade; SSE, REST, MCP batch/request handling, notification forwarding, and instance proxying are split into dedicated files.
- `src/server.rs` keeps the public server types and startup orchestration, while background tasks, gateway bootstrap, and listener spawn strategies live in dedicated implementation modules.
- `src/job.rs` is a thin facade over the in-process async job tracker: `Job` / `JobStatus` / `JobProgress` / `JobEvent` data live in `job_types.rs`, the `JobManager` registry (transitions, persistence, subscriptions, GC) lives in `job_manager.rs`, and unit tests live in `job_tests.rs`.
- `src/resources.rs` keeps the `ResourceRegistry` (producer wiring, subscription state, `notify_updated` fan-out); the `ResourceProducer` trait + content/error types live in `resources_types.rs`, the built-in producers (`scene://`, `capture://`, `audit://`, `artefact://`) live in `resources_producers.rs`, and unit tests live in `resources_tests.rs`.
- `src/prompts.rs` keeps the `PromptRegistry` (lazy cache, skill-set invalidation, `list` / `get` surface); the YAML spec types + `PromptError` live in `prompts_spec.rs`, the `{{name}}` template engine lives in `prompts_template.rs`, the sibling-file / glob loader plus the workflow-derived prompt generator live in `prompts_loader.rs`, and unit tests live in `prompts_tests.rs`.
- `src/protocol.rs` is a thin facade keeping protocol-version negotiation + session/method constants; every MCP message type is split by primitive: JSON-RPC envelope + error codes in `protocol_jsonrpc.rs`, lifecycle (`initialize` / capabilities / roots / logging / elicitation) in `protocol_lifecycle.rs`, tools (`tools/list` / `tools/call` / annotations / content) in `protocol_tools.rs`, resources (`resources/list|read|subscribe`) in `protocol_resources.rs`, prompts (`prompts/list|get`) in `protocol_prompts.rs`, and SSE formatter + cursor helpers in `protocol_sse.rs`.
- `src/handler.rs` is a thin facade over the top-level axum handlers: shared `AppState` + cancellation/elicitation TTL constants live in `handler_state.rs`, the three HTTP verbs (`POST` / `GET` / `DELETE /mcp`) live in `handler_routes.rs`, notification and response-message routing (`notifications/cancelled`, `roots/list_changed`, elicitation correlation) live in `handler_notifications.rs`, and the JSON-RPC method router plus `initialize` / `tools/list` implementations live in `handler_dispatch.rs`. Per-method request handlers (`tools/call`, `resources/*`, `prompts/*`, `elicitation/create`, …) continue to live under `src/handlers/` and are re-exported through the facade so existing call sites keep using `crate::handler::*`.
- `src/gateway/namespace.rs` is a thin facade over the tool-name namespace helpers: the canonical name lists (`GATEWAY_LOCAL_TOOLS`, `CORE_TOOL_NAMES`), SEP-986 separator constants, and the `is_local_tool` / `is_core_tool` / `instance_short` / `is_instance_prefix` predicates live in `namespace_constants.rs`; the encoder / decoder pair (`extract_bare_tool_name`, `skill_tool_name`, `decode_skill_tool_name`, `encode_tool_name`, `decode_tool_name`, `assert_gateway_tool_name`) lives in `namespace_encode.rs`; the #307 bare-name resolver (`BareNameInput`, `resolve_bare_names`) and the one-shot deprecation warn helpers (`warn_legacy_prefixed_once`, process-local dedupe state) live in `namespace_bare.rs`; unit tests live in `namespace_tests.rs`. The facade re-exports every public symbol so downstream modules and tests keep using `crate::gateway::namespace::*` unchanged.

### dcc-mcp-server

**Purpose**: Binary entry point (`dcc-mcp-server` CLI) that assembles and runs the full MCP server with gateway support.

**Key Components**:
- `main.rs` — CLI entry point using `GatewayRunner` and `McpHttpServer` library APIs

**Dependencies**: `dcc-mcp-http`

### dcc-mcp-logging

**Purpose**: File logging with rotation and retention pruning.

**Modules**:
- `file_logging` — Rolling-file `tracing` subscriber with size/daily rotation
- `file_logging_config` — `FileLoggingConfig`, `RotationPolicy`
- `file_logging_writer` — `RollingFileWriter`, rotation state machine

**Dependencies**: `dirs`, `tracing`, `tracing-subscriber`, `tracing-appender`, `parking_lot`, `time`

### dcc-mcp-paths

**Purpose**: Platform-specific path helpers.

**Modules**:
- `filesystem` — Platform-specific directories via `dirs` crate

**Dependencies**: `dirs`

### dcc-mcp-pybridge

**Purpose**: PyO3 helpers — `repr_pairs!` / `to_dict_pairs!` macros.

**Modules**:
- `py_json` — `py_json()` / `py_yaml()` serialization helpers
- `pybridge_derive` — `#[derive(ReprPairs)]` derive macro (in `dcc-mcp-pybridge-derive`)

**Dependencies**: `pyo3`

## Skills-First Architecture

The recommended entry-point for exposing DCC tools over MCP is the **Skills-First** pattern using `create_skill_server`. A single call wires together the full stack:

```
create_skill_server("maya")
        │
        ├─ ToolRegistry  (thread-safe tool store)
        ├─ ToolDispatcher (routes calls to Python handlers)
        ├─ SkillCatalog    (discovers + loads SKILL.md packages)
        │       └─ scans DCC_MCP_MAYA_SKILL_PATHS + DCC_MCP_SKILL_PATHS
        └─ McpHttpServer   (returns ready-to-start HTTP server)
```

```python
import os
os.environ["DCC_MCP_MAYA_SKILL_PATHS"] = "/studio/maya-skills"

from dcc_mcp_core import create_skill_server, McpHttpConfig

server = create_skill_server("maya", McpHttpConfig(port=8765))
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
`ToolRegistry` → `ToolDispatcher` → `SkillCatalog` → `McpHttpServer`.
:::

## Python Bindings

The workspace builds a single PyO3 native extension (`dcc_mcp_core._core`) via `maturin`; optional feature crates are included according to `pyproject.toml` / the root `justfile`.

```toml
# pyproject.toml
[project]
requires-python = ">=3.7"
dependencies = []  # Zero runtime dependencies
```

### Python Package Structure

```
python/dcc_mcp_core/
├── __init__.py     # Public top-level re-exports from _core + pure-Python helpers
├── *.py            # Pure-Python helpers (server base, skill helpers, envelopes, constants)
└── py.typed        # PEP 561 marker

# Generated after a stub-gen/dev build, not checked in as source of truth:
# python/dcc_mcp_core/_core.pyi
```

## Design Decisions

### 1. Zero Runtime Python Dependencies

The native extension bundles all Rust code — no `pip install` for PyO3, tokio, etc. This ensures:
- No version conflicts with DCC's embedded Python
- Predictable behavior across Maya/Blender/Houdini/3ds Max
- Minimal import latency

### 2. PyO3 0.28+ / Maturin

Using PyO3 with:
- `multiple-pymethods` — Multiple `#[pymethods]` per struct
- `abi3-py38` — Stable ABI for Python 3.8+ (CI tests 3.7–3.13)
- `extension-module` — Allow loading from any Python path

### 3. Rust Edition 2024, MSRV 1.95

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
- **Preferred Rust test shape**: keep helper fixtures in a thin root module and split large suites by behavior domain instead of appending more scenarios to a monolithic file

## Build Commands

| Command | Tool | Purpose |
|---------|------|---------|
| `cargo check` | cargo | Fast syntax/type check |
| `cargo clippy` | clippy | Lint with `-D warnings` (CI strict) |
| `cargo fmt --check` | rustfmt | Format check |
| `maturin develop` | maturin | Install wheel in dev mode |
| `cargo test --workspace` | cargo | Run all Rust tests |
| `pytest tests/` | pytest | Run Python integration tests |
