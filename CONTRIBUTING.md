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
   vx just prek-all   # Run all pre-commit hooks
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
- **Linter**: `ruff check` + `isort`
- **Target**: Python 3.7+ (but CI tests 3.11+)
- **Quotes**: Double quotes (`"`)
- **Docstrings**: Google-style

### Import Organization

All files must use section headers for imports:

```python
# Import future modules
from __future__ import annotations

# Import built-in modules
import os
from pathlib import Path

# Import third-party modules
from pydantic import BaseModel

# Import local modules
from dcc_mcp_core.models import ActionResultModel
```

### Type Hints

All public APIs must have type annotations:

```python
def call_action(self, name: str, **kwargs: Any) -> ActionResultModel:
    """Execute an action by name.

    Args:
        name: The action name to execute.
        **kwargs: Parameters to pass to the action.

    Returns:
        Structured result containing success status, message, and context.

    Raises:
        ActionNotFoundError: If the action name is not registered.
    """
```

### Testing

- Test files: `tests/test_<module>.py`
- Test functions: `test_<description>`
- Use `pyfakefs` for filesystem mocking
- Use `pytest-asyncio` for async tests
- Aim for high coverage on new code

## Project Architecture

```
dcc_mcp_core/
├── __init__.py              # Public API exports
├── models.py                # Pydantic data models (ActionResultModel)
├── actions/                 # Action system
│   ├── base.py              # Action base class with InputModel/OutputModel
│   ├── manager.py           # ActionManager: discover, load, execute actions
│   ├── registry.py          # ActionRegistry: register and retrieve actions
│   ├── middleware.py         # Middleware chain for cross-cutting concerns
│   ├── events.py            # EventBus for action lifecycle events
│   ├── adapter.py           # Adapters for legacy function-based actions
│   └── generator.py         # Action template generation
├── skills/                  # Skills system (zero-code script registration)
│   ├── scanner.py           # SkillScanner: discover SKILL.md files
│   ├── loader.py            # SkillLoader: parse YAML frontmatter
│   └── script_action.py     # ScriptAction factory
├── utils/                   # Shared utilities
│   ├── filesystem.py        # File discovery and path operations
│   ├── module_loader.py     # Dynamic Python module loading
│   ├── decorators.py        # Error handling decorators
│   ├── dependency_injector.py # DI container for action contexts
│   ├── template.py          # Jinja2 template rendering
│   ├── platform.py          # Platform detection utilities
│   ├── type_wrappers.py     # RPyC-safe type wrappers
│   └── result_factory.py    # Helper functions for ActionResultModel
└── protocols/               # Protocol/interface definitions
    └── __init__.py
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
| `vx just test` | Run tests with coverage |
| `vx just test-v` | Run tests with verbose output |
| `vx just test-file <path>` | Run a specific test file |
| `vx just lint` | Run linter checks |
| `vx just lint-fix` | Auto-fix lint issues |
| `vx just prek` | Run pre-commit on staged files |
| `vx just prek-all` | Run pre-commit on all files |
| `vx just build` | Build the package |
| `vx just clean` | Clean build artifacts |

## Questions?

Open an [issue](https://github.com/loonghao/dcc-mcp-core/issues) or start a [discussion](https://github.com/loonghao/dcc-mcp-core/discussions).
