# What is DCC-MCP-Core?

DCC-MCP-Core is an **action management system** designed for Digital Content Creation (DCC) applications, providing a unified interface that allows AI to interact with various DCC software such as Maya, Blender, Houdini, and more.

Built with a **Rust core** exposed to Python via [PyO3](https://pyo3.rs/), it combines the performance and thread safety of Rust with the accessibility of Python.

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

- **ActionRegistry** — Thread-safe, lock-free action registration and lookup via DashMap
- **EventBus** — Publish/subscribe system for decoupled action lifecycle events
- **Skills System** — Zero-code registration of scripts as MCP tools via SKILL.md
- **MCP Protocol Types** — Type-safe definitions for Tools, Resources, and Prompts
- **Transport Layer** — Connection pooling, service discovery, and session management for DCC communication
- **Type Wrappers** — RPyC-safe wrappers (BooleanWrapper, IntWrapper, FloatWrapper, StringWrapper)
- **Zero Python Dependencies** — Pure Rust core compiled to a native Python extension
- **Thread Safety** — All core types use DashMap for lock-free concurrent access

## Architecture

DCC-MCP-Core is a Rust workspace with 6 sub-crates, compiled into a single Python extension module via maturin:

```
dcc-mcp-core/
├── src/lib.rs                  # PyO3 module entry point (_core)
├── crates/
│   ├── dcc-mcp-actions/        # ActionRegistry, EventBus
│   ├── dcc-mcp-models/         # ActionResultModel, SkillMetadata
│   ├── dcc-mcp-protocols/      # MCP type definitions (Tool, Resource, Prompt)
│   ├── dcc-mcp-skills/         # SKILL.md scanner and loader
│   ├── dcc-mcp-transport/      # Connection pool, service discovery, sessions
│   └── dcc-mcp-utils/          # Filesystem, constants, type wrappers, logging
└── python/
    └── dcc_mcp_core/
        └── __init__.py          # Re-exports from _core extension
```

## Python API Surface

All public APIs are available from the top-level `dcc_mcp_core` package:

```python
import dcc_mcp_core

# 16 classes, 14 functions, 8 constants
# See the API Reference for complete documentation
```

## Related Projects

- [dcc-mcp-rpyc](https://github.com/loonghao/dcc-mcp-rpyc) — RPyC bridge for remote DCC operations
- [dcc-mcp-maya](https://github.com/loonghao/dcc-mcp-maya) — Maya MCP server implementation
