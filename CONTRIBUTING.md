# Contributing to dcc-mcp-core

Thank you for your interest in contributing! This guide will help you get started.

## Quick Start

```bash
# Clone and setup
git clone https://github.com/loonghao/dcc-mcp-core.git
cd dcc-mcp-core

# Install dependencies (requires vx: https://github.com/loonghao/vx)
vx just install

# Verify setup
vx just test
```

## Development Workflow

1. **Create a branch** from `main`:
   ```bash
   git checkout -b feat/my-feature
   ```

2. **Make changes** following the [coding standards](#coding-standards)

3. **Run checks** before committing:
   ```bash
   vx just lint       # Check code style
   vx just lint-fix   # Auto-fix code style issues
   vx just test       # Run tests with coverage
   vx just preflight  # Run pre-flight checks (Rust check + clippy + fmt + test)
   ```

4. **Commit** using [Conventional Commits](https://www.conventionalcommits.org/):
   ```bash
   git commit -m "feat: add new middleware type"
   git commit -m "fix: resolve action loading race condition"
   git commit -m "docs: update Skills system documentation"
   ```

5. **Push and open a Pull Request** against `main`

## Coding Standards

### Python Style

- **Formatter**: `ruff format` (line length: 120)
- **Linter**: `ruff check` (includes import sorting via `I` rules)
- **Target**: Python 3.7+ (CI tests 3.7–3.13)
- **Quotes**: Double quotes (`"`)
- **Docstrings**: Google-style

### Import Organization

All files must use section headers for imports:

```python
# Import future modules
from __future__ import annotations

# Import built-in modules
from pathlib import Path

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core import ToolResult
```

### Type Hints

All public APIs must have type annotations:

```python
def register_action(self, name: str, **kwargs: Any) -> None:
    """Register an action by name.

    Args:
        name: The action name to register.
        **kwargs: Action metadata fields (description, dcc, tags, etc.).
    """
```

### Testing

- Test files: `tests/test_<module>.py`
- Test functions: `test_<description>`
- Use `tmp_path` fixture for temporary directories
- Use `monkeypatch` for environment variable mocking
- Aim for high coverage on new code

## Project Architecture

```
crates/                      # Rust workspace crates (core logic)
├── dcc-mcp-models/          # ToolResult, SkillMetadata
├── dcc-mcp-actions/         # ToolRegistry, ToolDispatcher, EventBus, ToolPipeline
├── dcc-mcp-skills/          # SkillScanner, SkillCatalog, SkillWatcher, dependency resolver
├── dcc-mcp-protocols/       # MCP protocol types (Tool, Resource, Prompt, DccAdapter, BridgeKind)
├── dcc-mcp-transport/       # IPC, ConnectionPool, FramedChannel, CircuitBreaker, FileRegistry
├── dcc-mcp-process/         # PyDccLauncher, ProcessMonitor, CrashRecovery
├── dcc-mcp-telemetry/       # TelemetryConfig, ToolRecorder, ToolMetrics
├── dcc-mcp-sandbox/         # SandboxPolicy, InputValidator, AuditLog
├── dcc-mcp-shm/             # PyBufferPool, PySharedBuffer, LZ4 compression
├── dcc-mcp-capture/         # Capturer, cross-platform backends
├── dcc-mcp-usd/             # UsdStage, UsdPrim, scene info bridge
├── dcc-mcp-http/            # McpHttpServer, McpHttpConfig, Gateway (first-wins competition)
├── dcc-mcp-server/          # dcc-mcp-server binary, gateway runner
└── dcc-mcp-utils/           # Filesystem, constants, type wrappers, JSON
src/
└── lib.rs                   # PyO3 module entry point (_core)
python/
└── dcc_mcp_core/
    ├── __init__.py           # Public API re-exports (~177 symbols) from _core
    ├── _core.pyi             # Type stubs
    ├── skill.py              # Pure-Python skill script helpers (no _core dependency)
    └── py.typed              # PEP 561 marker
tests/                       # Python integration tests
examples/
└── skills/                  # Example SKILL.md packages (11 examples)
```

## Release Process

This project uses [Release Please](https://github.com/googleapis/release-please) for automated releases:

- Push conventional commits to `main` via PR
- Release Please creates/updates a release PR with version bump + CHANGELOG
- Merging the release PR triggers PyPI publish + GitHub Release creation

**You do NOT need to manually bump versions or edit CHANGELOG.md.**

## Available Commands

| Command | Description |
|---------|-------------|
| `vx just install` | Install project dependencies |
| `vx just dev` | Build + install dev wheel (maturin develop) |
| `vx just test` | Run Python tests |
| `vx just test-rust` | Run Rust unit tests |
| `vx just test-cov` | Run tests with coverage report |
| `vx just lint` | Run linter checks (Rust + Python) |
| `vx just lint-fix` | Auto-fix lint issues |
| `vx just preflight` | Pre-flight check (Rust check + clippy + fmt + test) |
| `vx just ci` | Full CI pipeline (Rust + Python) |
| `vx just build` | Build the package |
| `vx just clean` | Clean build artifacts |

## Questions?

Open an [issue](https://github.com/loonghao/dcc-mcp-core/issues) or start a [discussion](https://github.com/loonghao/dcc-mcp-core/discussions).
