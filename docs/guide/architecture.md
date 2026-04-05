# Architecture

This document describes the architecture of DCC-MCP-Core, a Rust-powered foundation library for the DCC Model Context Protocol ecosystem.

## Overview

DCC-MCP-Core is structured as a Rust workspace with Python bindings via PyO3. The library provides:

- **Zero third-party runtime dependencies** in the Rust core
- **Optional Python bindings** via PyO3 for DCC integration
- **Modular crate design** for selective dependency usage

## Crate Structure

```
dcc-mcp-core (workspace root)
├── dcc-mcp-models      # Data models and types
├── dcc-mcp-actions     # Action registry system
├── dcc-mcp-skills      # Skills package system
├── dcc-mcp-protocols   # MCP protocol types
├── dcc-mcp-transport   # IPC and network transport
└── dcc-mcp-utils       # Utility functions
```

### Dependency Graph

```
dcc-mcp-models (base types)
       ↓
dcc-mcp-actions ← dcc-mcp-models (actions depend on models)
       ↓
dcc-mcp-skills ← dcc-mcp-actions, dcc-mcp-models
       ↓
dcc-mcp-protocols ← dcc-mcp-models (protocol types use models)
       ↓
dcc-mcp-transport ← dcc-mcp-protocols (transport uses protocol types)
       ↓
dcc-mcp-utils (shared utilities, no internal dependencies)
```

## Crate Responsibilities

### dcc-mcp-models

**Purpose**: Core data models and type definitions.

**Key Types**:
- `ActionResult`: Standardized action execution result
- `SkillMetadata`: Skill package metadata
- `EventData`: Event payload structure
- `UriTemplate`: URI template parsing and matching

**Dependencies**: None (base crate)

### dcc-mcp-actions

**Purpose**: Centralized action registry and execution system.

**Key Components**:
- `ActionRegistry`: Global action registration and lookup
- `Action`: Trait for action implementations
- `ActionManager`: DCC-specific action management

**Key Functions**:
- `register_action()`: Register a named action
- `call_action()`: Execute a registered action
- `list_actions()`: Get all registered actions

**Dependencies**: `dcc-mcp-models`

### dcc-mcp-skills

**Purpose**: Zero-code skill package registration via markdown files.

**Key Components**:
- `SkillScanner`: Scans directories for skill packages
- `SkillRegistry`: Central skill storage
- `SkillResolver`: Resolves skill dependencies

**Skill Package Format**:
```markdown
---
name: my-skill
version: 1.0.0
description: A useful skill
author: Author Name
---

# Skill Documentation

Skill content and instructions...
```

**Dependencies**: `dcc-mcp-actions`, `dcc-mcp-models`

### dcc-mcp-protocols

**Purpose**: MCP (Model Context Protocol) type definitions.

**Key Types**:
- `MCPServerProtocol`: Server-side protocol implementation
- `MCPClientProtocol`: Client-side protocol implementation
- `PromptProtocol`: Prompt handling
- `ResourceProtocol`: Resource management
- `ToolProtocol`: Tool execution

**URI Templates**:
- `{+uri}` - URI with fragments
- `{uri}` - Base URI
- Query parameter extraction

**Dependencies**: `dcc-mcp-models`

### dcc-mcp-transport

**Purpose**: IPC and network communication layer.

**Transport Types**:
- **IPC**: Unix sockets / Windows named pipes
- **TCP**: Network sockets
- **WebSocket**: Browser-compatible
- **HTTP**: REST-style communication

**Key Components**:
- `TransportPool`: Connection pooling
- `TransportConfig`: Configuration management
- `Session`: Connection session tracking
- `WireProtocol`: Binary serialization (MessagePack)

**Ping/Pong Health Checks**:
- `ping()` - Send heartbeat
- `ping_with_timeout()` - Check response within timeout
- Automatic reconnection on timeout

**Dependencies**: `dcc-mcp-protocols`, `tokio`

### dcc-mcp-utils

**Purpose**: Shared utility functions.

**Modules**:
- `filesystem`: File path operations
- `type_wrappers`: Python type interop helpers
- `constants`: Shared constants

**Dependencies**: None

## Python Bindings

Python bindings are generated via PyO3 with the `python-bindings` feature:

```toml
[features]
python-bindings = ["pyo3", "dcc-mcp-models/python-bindings", ...]
```

### Python Package Structure

```
python/dcc_mcp_core/
├── __init__.py      # Public API
├── _core.pyi       # Type stubs
└── _core.*.so      # Compiled Rust extension
```

### Binding Pattern

Each crate exposes a `python_module!` macro that generates the Python module:

```rust
#[pymodule]
fn dcc_mcp_core(_py: Python, m: &PyModule) -> PyResult<()> {
    dcc_mcp_models::python::register_module(m)?;
    dcc_mcp_actions::python::register_module(m)?;
    // ...
    Ok(())
}
```

## Design Decisions

### 1. Zero Runtime Dependencies

The Rust core has no third-party runtime dependencies. This ensures:
- Minimal binary size
- Predictable behavior
- No dependency version conflicts in DCC environments

Optional dependencies (like `pyo3`) are feature-gated.

### 2. PyO3 0.28

Using PyO3 0.28 with:
- `multiple-pymethods` - Multiple #[pymethods] per struct
- `abi3-py38` - Stable ABI for Python 3.8+
- `extension-module` - Allow loading from any Python path

### 3. Rust Edition 2024

Edition 2024 provides:
- Implicit `async fn` in trait definitions
- `async let` bindings
- Lifetime subtyping improvements

### 4. Tokio for Async

Using Tokio for async runtime because:
- Industry standard for Rust async
- Excellent Windows support (named pipes)
- Well-tested with PyO3

### 5. MessagePack Serialization

Using RMP (Rust MessagePack) for wire protocol:
- Compact binary format
- Fast serialization/deserialization
- Language-agnostic

## Memory Model

### ActionRegistry

The `ActionRegistry` uses a `DashMap` for thread-safe action storage:

```rust
struct ActionRegistry {
    actions: DashMap<String, Arc<dyn Action>>,
}
```

### EventBus

The `EventBus` supports both sync and async handlers:

```rust
struct EventBus {
    sync_handlers: DashMap<String, Vec<HandlerFn>>,
    async_handlers: DashMap<String, Vec<AsyncHandlerFn>>,
}
```

### TransportPool

Connection pooling for efficient resource usage:

```rust
struct TransportPool {
    config: TransportConfig,
    connections: DashMap<SessionId, Connection>,
    semaphore: Semaphore,
}
```

## Thread Safety

All internal state uses:
- `parking_lot::Mutex` for short critical sections
- `DashMap` for concurrent hash maps
- `Arc` for shared ownership

No `std::sync::Mutex` - `parking_lot` is faster and doesn't poison on panic.

## Error Handling

Using `thiserror` for error types:

```rust
#[derive(Error, Debug)]
pub enum TransportError {
    #[error("Connection timeout: {0}")]
    Timeout(String),

    #[error("Connection refused: {0}")]
    ConnectionRefused(String),

    #[error("Ping timeout after {0}s")]
    PingTimeout(u64),
}
```

## Testing Strategy

- **Unit tests**: Each crate has inline `#[cfg(test)]` modules
- **Integration tests**: `tests/` directory with Python and Rust tests
- **Property tests**: `proptest` for randomized testing
- **Fuzzing**: `cargo-fuzz` for protocol parsing

## Build Optimization

Release profile configured for maximum performance:

```toml
[profile.release]
opt-level = 3      # Maximum optimization
lto = true         # Link-time optimization
codegen-units = 1  # Single codegen unit for better optimization
strip = true       # Strip symbols from binary
```

## Future Considerations

- **WebAssembly support**: Potential for browser-based DCC communication
- **gRPC transport**: Alternative to custom wire protocol
- **GraphQL subscription support**: For real-time DCC state updates
