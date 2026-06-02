# Architecture

DCC-MCP-Core is a **Rust-powered DCC automation framework** with Python bindings via PyO3, designed for AI-assisted workflows. The current architecture is gateway-first: MCP clients, CLI users, ClawHub/OpenClaw skills, CI, and custom HTTP clients all converge on a small discovery/dispatch control plane instead of receiving every backend tool in `tools/list`.

## High-Level Design

### Current Gateway-First Stack

```
+--------------------------------------------------------------------------------+
| Agent / operator surfaces                                                       |
| - MCP clients: search -> describe -> load_skill? -> call                       |
| - CLI users: dcc-mcp-cli list/search/describe/call                              |
| - ClawHub/OpenClaw skills: dcc-cli-gateway                                      |
| - CI and custom clients: REST /v1/*                                             |
+----------------------------------------+---------------------------------------+
                                         |
                       MCP Streamable HTTP + REST /v1/*
                                         |
+----------------------------------------v---------------------------------------+
| Elected gateway (Rust HTTP control plane)                                       |
| - Minimal MCP tools/list: four canonical workflow primitives only               |
| - Dynamic capability search, schema describe, single/batch call routing         |
| - Instance registry, TCP liveness probes, version-aware election, failover      |
| - Admin UI, OpenAPI, audit logs, traces, metrics, jobs, workflows, artefacts    |
+----------------------------------------+---------------------------------------+
                                         |
                    Gateway-routed calls to owning per-DCC server
                                         |
        +-------------------------------+-------------------------------+
        |                               |                               |
+-------v--------+              +-------v--------+              +-------v--------+
| Maya adapter   |              | Blender adapter|              | Custom host    |
| MCP + REST     |              | MCP + REST     |              | MCP + REST     |
| Skills catalog |              | Skills catalog |              | Skills catalog |
+-------+--------+              +-------+--------+              +-------+--------+
        |                               |                               |
  Host bridge / UI-thread pump    Host bridge / add-on           Host RPC / IPC
```

### Core Principles

1. **Minimal MCP surface** — `tools/list` exposes `search`, `describe`, `load_skill`, and `call`; backend tools are never fanned out directly.
2. **Version-aware election** — The gateway is elected and can fail over without hard-coding a single DCC host.
3. **Zero-code skills** — `SKILL.md` + sibling YAML/scripts become MCP tools and REST-callable capabilities.
4. **Structured results** — Every tool returns AI-friendly success/error, message, context, and follow-up hints.
5. **Progressive discovery** — Capabilities are scoped by DCC, instance, scene, product, and skill state.

## The Library

DCC-MCP-Core is a Rust workspace with Python bindings via PyO3. It provides:

- **Zero third-party runtime dependencies** in the Rust core
- **Optional Python bindings** via PyO3 for DCC integration
- **42 workspace packages** (41 functional packages + `workspace-hack`) for selective dependency usage; root `Cargo.toml` is the source of truth

## Crate Structure

```
dcc-mcp-core (workspace root)
├── dcc-mcp-naming        # client-safe tool-name / action-id validators
├── dcc-mcp-models        # ToolResult, SkillMetadata, DccName, shared errors
├── dcc-mcp-actions       # ToolRegistry, EventBus, ToolDispatcher, validation
├── dcc-mcp-skills        # SkillScanner, SkillCatalog, SkillWatcher, resolver
├── dcc-mcp-protocols     # MCP-facing Tool/Resource/Prompt/DccAdapter models
├── dcc-mcp-jsonrpc       # MCP 2025-03-26 JSON-RPC builders
├── dcc-mcp-wire          # Canonical MCP/REST call envelopes, validation, normalization
├── dcc-mcp-app-ui        # DCC-agnostic app_ui observation/action contracts
├── dcc-mcp-job           # Async job tracking + optional persistence
├── dcc-mcp-skill-rest    # Per-DCC /v1/* REST skill API
├── dcc-mcp-gateway-core  # Pure gateway domain/search/ranking types
├── dcc-mcp-gateway-search # Reusable capability search/query/ranking engine
├── dcc-mcp-gateway       # Multi-DCC gateway app + dynamic wrappers
├── dcc-mcp-http-types    # Pure HTTP wire/config/value types, McpHttpConfig
├── dcc-mcp-http-server   # Reusable HTTP runtime support
├── dcc-mcp-http-py       # PyO3 binding boundary for HTTP APIs
├── dcc-mcp-http          # Embedded MCP HTTP facade + compatibility re-exports
├── dcc-mcp-cli           # Client control-plane CLI for gateway REST
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
├── dcc-mcp-catalog       # Public adapter catalog search/describe
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
dcc-mcp-wire ← dcc-mcp-jsonrpc, serde_json (canonical call envelopes + normalization)
       ↓
dcc-mcp-transport ← dcc-mcp-protocols
       ↓
dcc-mcp-gateway-core ← pure gateway domain/search/ranking types
       ↓
dcc-mcp-gateway-search ← reusable search/query/ranking engine
       ↓
dcc-mcp-gateway ← dcc-mcp-gateway-core, dcc-mcp-gateway-search, dcc-mcp-wire, dcc-mcp-transport
       ↓
dcc-mcp-http-types ← pure HTTP wire/config/value types
       ↓
dcc-mcp-http-server ← dcc-mcp-http-types, dcc-mcp-jsonrpc, dcc-mcp-job, dcc-mcp-host
       ↓
dcc-mcp-http ← dcc-mcp-http-types, dcc-mcp-http-server, dcc-mcp-gateway, dcc-mcp-skill-rest
       ↓
dcc-mcp-server ← dcc-mcp-http

dcc-mcp-cli ← dcc-mcp-catalog + gateway REST contract

dcc-mcp-app-ui (independent pure app_ui observation/action/wait/policy/audit contracts)
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

**Purpose**: Centralized tool registry, validation, dispatch, and pipeline system.

**Key Components**:
- `ToolRegistry` — Thread-safe registry: register/get/search/list/unregister tools
- `ToolDispatcher` — Typed dispatch with validation to registered Python callables
- `ToolValidator` — JSON Schema-based parameter validation
- `ToolPipeline` — Middleware pipeline (logging, timing, audit, rate limiting)
- `EventBus` — Pub/sub event system for DCC lifecycle events
- `VersionedRegistry` — Multi-version tool registry with SemVer constraint resolution

**Key Traits**: None — actions are plain Python callables registered via `ToolDispatcher.register_handler()`

**Dependencies**: `dcc-mcp-models`

**Maintainer layout**:
- `registry/mod.rs` keeps the core registry behavior, while `ToolMeta` and the Python binding shim live in focused sibling modules.
- `chain.rs` is a thin facade: step/result types, placeholder interpolation, the `ActionChain` fluent builder and executor, and unit tests each live in dedicated sibling modules (`chain_types.rs`, `chain_interpolate.rs`, `chain_exec.rs`, `chain_tests.rs`).
- This separates Rust-side lookup/update semantics from PyO3 translation code and makes metadata evolution easier to review.

### dcc-mcp-skills

**Purpose**: Zero-code skill package discovery, loading, and hot-reload via filesystem watching.

**Key Components**:
- `SkillScanner` — mtime-cached directory scanner for SKILL.md packages
- `SkillCatalog` — Progressive skill loading with on-demand discovery (register actions on `load_skill`)
- `SkillWatcher` — Platform-native filesystem watcher (inotify/FSEvents/ReadDirectoryChangesW)
- `SkillMetadata` — Parsed metadata from agentskills.io `SKILL.md` plus `metadata.dcc-mcp.*` sibling files
- Dependency resolution: `resolve_dependencies`, `expand_transitive_dependencies`, `validate_dependencies`

**Skill Package Format**: agentskills.io `SKILL.md` frontmatter (`name`, `description`, optional `license` / `compatibility` / `allowed-tools`) plus `metadata.dcc-mcp.*` pointers to sibling files such as `tools.yaml`, `groups.yaml`, workflows, prompts, resources, and external dependency declarations. Legacy top-level dcc-mcp extension keys are rejected by the strict loader.

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
- `types.rs` is now a thin re-export surface; tool/resource/prompt models live in focused internal modules (`types_tools.rs`, `types_resources.rs`, `types_prompts.rs`).
- `DccSceneManager` is now the compatibility composite of focused ISP traits: `DccSceneQuery` (read-only scene inspection), `DccFileIO` (file lifecycle), and `DccSelection` (selection management). New adapters should implement only the focused traits they support; code that still needs the full surface can keep bounding on `DccSceneManager`.
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
- `src/pipeline/python.rs` becomes a 67-line facade that mounts four siblings via `#[path]`: `python_helpers` (`value_to_py`, `PyCallableHook`), `python_middleware` (`PyLoggingMiddleware`, `PyTimingMiddleware`, `PyAuditMiddleware`, `PyRateLimitMiddleware` — inner fields `pub(super)` so the pipeline can construct them), `python_shared` (`Shared{Timing,Audit,RateLimit}Middleware` `Arc` newtypes implementing `ActionMiddleware`), `python_pipeline` (`PyToolPipeline` — the Python-facing `ToolPipeline`). Every Python class is re-exported so `pipeline::python::{PyToolPipeline, PyLoggingMiddleware, …}` keeps working. The `Shared*` newtypes are re-exported under `#[cfg(test)]` so the existing `python_tests.rs` unit tests can reference `super::python::Shared*` unchanged.

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

### dcc-mcp-wire

**Purpose**: Canonical MCP/gateway wire contract for `tools/call` and REST `/v1/call` envelopes. Put shared serialization, validation, and normalization here instead of duplicating ad-hoc helpers in JSON-RPC, gateway, host adapters, or Python wrappers.

**Key Components**:
- `decode_call_tool`, `decode_rest_call`, `encode_call_tool_result` — shared envelope conversion for MCP and REST paths.
- `normalize_arguments` — accepts missing / `null` / empty string as `{}`, preserves objects, parses object-shaped JSON strings, and rejects arrays / numbers / booleans / non-object strings.
- `normalize_meta` — optional sidecar normalization for MCP `_meta` / REST `meta` objects.
- `WireError::kind()` — stable error taxonomy consumed by gateway and REST responses.

**Python host wrappers**: `dcc_mcp_core.host.normalize_tool_arguments()` and `normalize_tool_meta()` expose the same normalization contract for adapters and connectors.

### dcc-mcp-gateway-core

**Purpose**: Pure gateway domain layer for capability records, slug helpers, search queries/pages/hits, and ranking/scoring. It has no HTTP, async runtime, file-registry, or `dcc-mcp-gateway` dependency.

**Key Components**:
- `PendingCall` — gateway-to-backend cancellation correlation primitive.
- `CapabilityRecord` — compact per-tool search/dispatch record.
- `SearchQuery`, `SearchHit`, `SearchPage`, `SearchMode` — token-budgeted capability search contract.
- `ExactScorer`, `FuzzyScorer`, `SubstringScorer`, `StrategyScorer` — pluggable ranking strategies.

**Dependencies**: `serde`, `uuid`, `nucleo-matcher` only where the pure ranking strategy needs it.

### dcc-mcp-gateway-search

**Purpose**: Reusable search/query/ranking engine for the gateway capability index. Keep tokenization, fuzzy/exact matching, pagination, and record projection here so `dcc-mcp-gateway` stays focused on HTTP/MCP orchestration and registry refresh.

**Dependencies**: Pure search dependencies only; no axum, reqwest, registry, or admin UI coupling.

### dcc-mcp-gateway

**Purpose**: Multi-DCC gateway application/infrastructure: registry probing, dynamic MCP wrappers, `/v1/*` REST facade, routing, diagnostics, and admin surface.

**Key Components**:
- `CapabilityIndex` + refresh tasks — build records from live per-DCC instances and evict stale ones.
- `search`, `describe`, `load_skill`, `call` — fixed gateway MCP workflow tools over the dynamic capability index; `/v1/*` routes are the pure HTTP twin.
- Gateway REST facade — `POST /v1/search`, `/v1/describe`, `/v1/call`, `/v1/call_batch`, plus diagnostics/resources/prompts aggregation.
- Admin/dashboard support — read-only `/admin/api/*` inspection for instances, tools, calls, traces, stats, workers, logs, and health.

**Dependencies**: `dcc-mcp-gateway-core`, `dcc-mcp-gateway-search`, `dcc-mcp-wire`, `dcc-mcp-transport`, `dcc-mcp-skill-rest`, `reqwest`, `tokio`.

### dcc-mcp-app-ui

**Purpose**: DCC-agnostic application UI observation/action contract values. Keep these schemas independent from HTTP, Python bindings, OS accessibility, Qt, or webview automation so adapters can share the same contract without inheriting a backend.

**Key Types**:
- `UiBounds`, `UiControlNode`, `UiSnapshot` — bounded UI tree snapshots.
- `UiFindRequest`, `UiActionRequest`, `UiActionKind`, `UiActionResult` — find-and-act envelopes for scoped controls.
- `UiWaitCondition`, `UiWaitResult` — in-tool polling contract.
- `AppUiPolicy`, `AppUiAuditRecord` — action policy and audit records.

**Compatibility**: The Rust types are not re-exported through `dcc-mcp-http` because this surface has not shipped yet. The Python-facing contract names remain stable in `dcc_mcp_core.adapter_contracts`, and CLI/server behavior is unchanged.

**Dependencies**: `serde`, `serde_json`.

### dcc-mcp-http-types

**Purpose**: Pure HTTP wire/config/value types moved out of `dcc-mcp-http` for issue #852. It has no axum, tower, tokio runtime, reqwest, or PyO3 dependency.

**Key Types**:
- `HttpError` / `HttpResult` — shared HTTP error taxonomy.
- `JobConfig`, `WorkflowConfig`, `TelemetryConfig`, `FeatureFlags`, `InstanceConfig` — server configuration value objects.
- `PromptSpec`, `PromptsSpec`, `ProducerContent`, `ResourceError`, `OutputEntry`, `SessionLogMessage` — prompt/resource/output/session wire values.
- `TruncationEnvelope`, `SseChunkFrame` — response-size and SSE chunking helpers.

**Compatibility**: `dcc-mcp-http` re-exports these types under historical paths while callers migrate.

### dcc-mcp-http-server

**Purpose**: Reusable runtime support for the embedded MCP HTTP server without axum or PyO3.

**Key Components**:
- `build_core_tools` — constructs the fixed core MCP tool descriptors.
- `DccExecutorHandle`, `DeferredExecutor` — host/main-thread execution bridge.
- `McpSession`, `SessionManager`, `ToolListSnapshot` — session state and connection-scoped `tools/list` cache.
- `InFlightRequests`, `CancelToken`, `ProgressReporter` — cancellation/progress routing.
- `JobNotifier`, `WorkflowUpdate`, `WorkspaceRoots` — job/workflow notifications and root resolution.

**Dependencies**: `dcc-mcp-http-types`, `dcc-mcp-jsonrpc`, `dcc-mcp-job`, `dcc-mcp-host`, `dcc-mcp-workflow`.

### dcc-mcp-http

**Purpose**: MCP Streamable HTTP facade (2025-03-26 spec) for HTTP-based MCP clients. It owns axum routing, server startup, prompt/resource registries, gateway bootstrap, and compatibility re-exports from the extracted crates.

**Key Components**:
- `McpHttpServer` — background-thread HTTP server (axum/Tokio).
- `McpHttpConfig` — re-export of the `dcc-mcp-http-types::config` aggregate for compatibility; new Rust code can import it from `dcc-mcp-http-types` directly.
- `McpServerHandle` — URL retrieval, `is_gateway` flag, and graceful shutdown.
- `ResourceRegistry` and `PromptRegistry` — MCP `resources/*` and `prompts/*` implementation.
- Gateway bootstrap — delegates dynamic gateway behavior to `dcc-mcp-gateway`.

**SSE Support**: `GET /mcp` long-lived SSE stream for server-push events.

**Connection-Scoped Cache** (issue #438): Per-session `tools/list` snapshot stored on `McpSession`. On cache hit, avoids redundant registry scan, bare-name resolution, and `McpTool` construction. Invalidated when the `AppState::registry_generation` counter is bumped (skill load/unload, group activation/deactivation). Configurable via `McpHttpConfig.enable_tool_cache` (default `True`).

**Dependencies**: `axum`, `tokio`, `reqwest`, `socket2`, `dcc-mcp-http-types`, `dcc-mcp-http-server`, `dcc-mcp-gateway`, `dcc-mcp-skill-rest`, `dcc-mcp-transport`, `dcc-mcp-protocols`, `dcc-mcp-actions`, `dcc-mcp-skills`.


### dcc-mcp-server

**Purpose**: Binary composition entry point (`dcc-mcp-server` CLI). The implicit root mode, explicit `auto`, and default `serve` modes run a per-DCC MCP server, ensure the machine-wide gateway daemon, register as a backend, and keep a daemon guardian while the backend is alive. `serve --no-auto-gateway` runs a per-DCC server without touching the shared gateway, `auto --legacy-gateway-election` restores the old embedded first-wins path, and `gateway` runs the machine-wide gateway daemon without inline DCC execution.

### dcc-mcp-cli

**Purpose**: Client-side control-plane CLI for users, CI, and shell-capable skills. It talks to the gateway REST surface for `health`, `list`, instance-scoped `search`, `load-skill`, `describe`, slug or direct backend `call`, `wait-ready`, guarded `stop-instance`, and adapter install planning; it does not host skills or replace per-DCC servers.

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
