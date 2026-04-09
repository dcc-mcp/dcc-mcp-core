---
name: usd-tools
description: "OpenUSD scene inspection and validation tools — read layer stacks, traverse prims, validate USD files. Use when working with USD assets, pipelines, or scene description."
license: Apache-2.0
compatibility: Requires usdcat and usdchecker from the OpenUSD distribution
allowed-tools: Bash Read
metadata:
  category: pipeline
  openclaw:
    requires:
      bins:
        - usdcat
        - usdchecker
    emoji: "🎬"
    homepage: https://openusd.org
tags: [usd, openusd, pipeline, scene, validation]
dcc: python
version: "1.0.0"
tools:
  - name: inspect
    description: Print the contents of a USD file in human-readable form
    input_schema:
      type: object
      required: [file]
      properties:
        file: {type: string, description: Path to the USD file}
        flatten: {type: boolean, description: Flatten all layers, default false}
    read_only: true
    idempotent: true
    source_file: scripts/inspect.py

  - name: validate
    description: Run USD compliance checks on a file
    input_schema:
      type: object
      required: [file]
      properties:
        file: {type: string, description: Path to the USD file to validate}
    read_only: true
    idempotent: true
    source_file: scripts/validate.py
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
