---
name: blender-prompts-demo
description: >-
  Fixture skill exercising the MCP prompts primitive (issue #731 gateway
  aggregation coverage). Ships one template prompt whose name is
  deliberately disjoint from the maya-side fixture so the aggregated
  list contains a clear union.
license: MIT
compatibility: Python 3.7+
metadata:
  dcc-mcp.dcc: blender
  dcc-mcp.version: "1.0.0"
  dcc-mcp.layer: example
  dcc-mcp.search-hint: "export gltf, prompts fixture"
  dcc-mcp.prompts: prompts.yaml
---

# Blender prompts demo

Fixture-only skill for `test_gateway_prompts_aggregation.py`. Publishes
one prompt (`export_gltf`) disjoint from the maya fixture's prompts so
the merged ``prompts/list`` round-trip can be asserted set-wise.
