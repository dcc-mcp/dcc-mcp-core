# Getting Started

## Installation

### From PyPI

```bash
pip install dcc-mcp-core
```

### From Source

```bash
git clone https://github.com/loonghao/dcc-mcp-core.git
cd dcc-mcp-core
pip install -e .
```

## Requirements

- **Python**: >= 3.7 (CI tests 3.11, 3.12, 3.13)
- **License**: MIT
- **Dependencies**: Zero for Python 3.8+

## Quick Start

```python
from dcc_mcp_core import create_action_manager

# Create an action manager for a specific DCC
manager = create_action_manager("maya")

# Execute an action
result = manager.call_action("create_sphere", radius=2.0)

# Check the result
print(result.success, result.message, result.context)
```

## Development Setup

```bash
# Clone the repository
git clone https://github.com/loonghao/dcc-mcp-core.git
cd dcc-mcp-core

# Create and activate virtual environment
python -m venv venv
source venv/bin/activate  # On Windows: venv\Scripts\activate

# Install development dependencies
pip install -e .
pip install pytest pytest-cov pytest-mock pyfakefs

# Or use vx (recommended)
vx just install
```

## Running Tests

```bash
# Run tests with coverage
vx just test

# Run specific tests
vx uvx nox -s pytest -- tests/test_action_manager.py -v

# Run linting
vx just lint

# Run linting with auto-fix
vx just lint-fix
```

## Next Steps

- Learn about [Actions](/guide/actions) — the core building block
- Explore the [Action Manager](/guide/action-manager) for lifecycle management
- Check out the [Skills System](/guide/skills) for zero-code script registration
