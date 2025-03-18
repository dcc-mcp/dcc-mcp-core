# dcc-mcp-core

Foundational library for the DCC Model Context Protocol (MCP) ecosystem. It provides common utilities, base classes, and shared functionality that are used across all other DCC-MCP packages.

## Features

- Parameter processing and validation
- Standardized logging system
- Common exception hierarchy
- Utility functions for DCC integration
- Version compatibility checking

## Requirements

- Python 3.7+
- Compatible with Windows, macOS, and Linux
- Designed to work within DCC software Python environments

## Installation

```bash
pip install dcc-mcp-core
```

## Usage

```python
from dcc_mcp_core import logging, parameters, exceptions

# Configure logging
logger = logging.get_logger("my_module")
logger.info("Starting operation")

# Process parameters
params = parameters.validate({"value": 10}, {"value": {"type": int, "required": True}})

# Handle exceptions
try:
    # Your code here
    pass
except exceptions.MCPError as e:
    logger.error(f"Error occurred: {e}")
```

## License

MIT
