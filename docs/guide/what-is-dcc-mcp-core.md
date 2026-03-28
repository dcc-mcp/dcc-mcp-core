# What is DCC-MCP-Core?

DCC-MCP-Core is an **action management system** designed for Digital Content Creation (DCC) applications, providing a unified interface that allows AI to interact with various DCC software such as Maya, Blender, Houdini, and more.

## Core Workflow

```mermaid
flowchart LR
    AI([AI Assistant]):::aiNode
    MCP{{MCP Server}}:::serverNode
    DCCMCP{{DCC-MCP}}:::serverNode
    Actions[(DCC Actions)]:::actionsNode
    DCC[/DCC Software/]:::dccNode

    AI -->|1. Send Request| MCP
    MCP -->|2. Forward Request| DCCMCP
    DCCMCP -->|3. Discover & Load| Actions
    Actions -->|4. Return Info| DCCMCP
    DCCMCP -->|5. Structured Data| MCP
    MCP -->|6. Call Function| DCCMCP
    DCCMCP -->|7. Execute| DCC
    DCC -->|8. Operation Result| DCCMCP
    DCCMCP -->|9. Structured Result| MCP
    MCP -->|10. Return Result| AI

    classDef aiNode fill:#f9d,stroke:#f06,stroke-width:2px,color:#333
    classDef serverNode fill:#bbf,stroke:#66f,stroke-width:2px,color:#333
    classDef dccNode fill:#bfb,stroke:#6b6,stroke-width:2px,color:#333
    classDef actionsNode fill:#fbb,stroke:#f66,stroke-width:2px,color:#333
```

## Key Features

- **Class-Based Actions** — Define operations using Pydantic models with strong type checking and input validation
- **ActionManager** — Lifecycle coordinator for action discovery, loading, and execution
- **Middleware System** — Chain-of-responsibility pattern for logging, performance monitoring, and more
- **Event System** — Publish/subscribe EventBus for action lifecycle events
- **Skills System** — Zero-code registration of scripts as MCP tools via SKILL.md
- **MCP Protocol Layer** — Full protocol abstractions for Tools, Resources, and Prompts
- **Zero Dependencies** — Python 3.8+ with no third-party Python dependencies
- **Rust Core** — Performance-critical modules written in Rust via PyO3

## Project Structure

```
dcc_mcp_core/
├── __init__.py              # Public API exports
├── models.py                # ActionResultModel, SkillMetadata
├── actions/
│   ├── base.py              # Action base class
│   ├── manager.py           # ActionManager
│   ├── registry.py          # ActionRegistry (singleton)
│   ├── middleware.py         # Middleware system
│   ├── events.py            # EventBus
│   ├── function_adapter.py  # Action-to-function adapters
│   └── generator.py         # Action template generation
├── skills/
│   ├── scanner.py           # SkillScanner
│   ├── loader.py            # SKILL.md parser
│   └── script_action.py     # ScriptAction factory
├── protocols/
│   ├── types.py             # MCP type definitions
│   ├── base.py              # Resource, Prompt ABCs
│   ├── server.py            # MCPServerProtocol
│   └── adapter.py           # MCPAdapter
└── utils/
    ├── filesystem.py         # Platform dirs, env paths
    ├── module_loader.py      # Dynamic module loading
    ├── decorators.py         # error_handler, with_context
    ├── dependency_injector.py
    ├── template.py           # Jinja2 rendering
    ├── type_wrappers.py      # RPyC-safe type wrappers
    └── result_factory.py     # Factory functions
```

## Related Projects

- [dcc-mcp-rpyc](https://github.com/loonghao/dcc-mcp-rpyc) — RPyC bridge for remote DCC operations
- [dcc-mcp-maya](https://github.com/loonghao/dcc-mcp-maya) — Maya MCP server implementation
