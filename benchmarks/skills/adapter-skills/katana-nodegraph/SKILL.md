---
name: katana-nodegraph
description: >-
  Host skill - inspect and safely author a Katana node graph with bounded typed
  node, parameter, port, timeline, and project-save operations. Not for raw
  Python, expressions, shell commands, or render scripts.
license: MIT
compatibility: "Katana NodegraphAPI; dcc-mcp-core 0.19+"
allowed-tools: Python
metadata:
  dcc-mcp:
    dcc: katana
    version: "0.5.1"  # x-release-please-version
    layer: domain
    stage: scene
    search-hint: "katana node graph create connect parameter timeline save project"
    tags: "katana,lookdev,nodegraph,lighting"
    tools: tools.yaml
---

# Katana Node Graph

Use read-only status and inspection tools before changing a project. Create
nodes and ports explicitly, connect only existing named ports, and restrict
parameter changes to JSON scalar values. Use `confirm=true` for node or port
deletion. Before saving, configure `DCC_MCP_KATANA_ALLOWED_ROOTS`; overwriting
an existing `.katana` file is always opt-in.

If a non-idempotent call times out after host execution starts, inspect the
node graph before retrying because the final Katana outcome is unknown.
