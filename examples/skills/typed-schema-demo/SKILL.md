---
name: typed-schema-demo
description: >-
  Example skill — demonstrates zero-dependency JSON Schema derivation from
  Python dataclasses and type annotations (issue #242). Use as a reference
  when authoring typed handlers that should publish inputSchema /
  outputSchema without hand-writing JSON. Not intended for production use.
license: MIT
compatibility: Python 3.10+
metadata:
  dcc-mcp.dcc: python
  dcc-mcp.version: "1.0.0"
  dcc-mcp.layer: example
  dcc-mcp.search-hint: "structured schema, dataclass, json schema, inputSchema, outputSchema, typed handler, pydantic-free"
  dcc-mcp.tags: "example, schema, structured"
---

# Typed Schema Demo (issue #242)

This skill demonstrates the `dcc_mcp_core.schema` helpers landed for
issue #242: authors write a typed handler and
`tool_spec_from_callable` derives both `inputSchema` and `outputSchema`
from the annotations, with no dependency on `pydantic`, `jsonschema`, or
`attrs`.

## What to look at

- `scripts/demo.py` — one handler using a dataclass input and a dataclass
  output. The derived schemas are structurally compatible with pydantic's
  `model_json_schema()` so callers can swap in pydantic later without
  migrating agents or cached schemas.

## How to wire it into a server

The demo module builds a `ToolSpec` that is ready for
`dcc_mcp_core._tool_registration.register_tools(server, [spec])`. Inside
an adapter (e.g. Maya/Blender), register it during bootstrap:

```python
from dcc_mcp_core._tool_registration import register_tools
from typed_schema_demo.scripts.demo import spec

register_tools(server, [spec], dcc_name="python")
```

When the negotiated MCP session is `2025-06-18`, the gateway publishes
`outputSchema` alongside `inputSchema` so clients can validate the
`structuredContent` payload our handler returns.
