---
name: usd-tools
description: >-
  Infrastructure skill — low-level OpenUSD scene inspection and validation:
  read layer stacks, traverse prims, validate USD schemas. Use when working
  directly with raw USD files (usda, usdc, usdz) or verifying USD compliance.
  Not for Maya-specific USD export — use maya-pipeline__export_usd for that.
  Not for full DCC pipeline workflows — use a domain pipeline skill instead.
license: Apache-2.0
compatibility: Requires usdcat and usdchecker from the OpenUSD distribution
allowed-tools: Bash Read
metadata:
  dcc-mcp.dcc: python
  dcc-mcp.version: "1.0.0"
  dcc-mcp.layer: infrastructure
  dcc-mcp.search-hint: "USD stage, prim, schema validation, layer stack, usda, usdc, usdz, usdchecker, usdcat, raw USD file"
  dcc-mcp.tags: "usd, openusd, scene inspection, validation, infrastructure"
  dcc-mcp.tools: tools.yaml
  openclaw:
    requires:
      bins:
        - usdcat
        - usdchecker
    emoji: "🎬"
    homepage: https://openusd.org
---

# OpenUSD Tools

Inspect and validate Universal Scene Description (USD) files.

## Tools

### `usd_tools__inspect`
Print the contents of a `.usd`, `.usda`, `.usdc`, or `.usdz` file.

### `usd_tools__validate`
Run `usdchecker` compliance validation and return the report.

## Prerequisites

Install the OpenUSD Python bindings:
```bash
pip install usd-core
```
Or install the full distribution from https://openusd.org.
