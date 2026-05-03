---
name: maya-prompts-demo
description: >-
  Fixture skill exercising the MCP prompts primitive (issue #731 gateway
  aggregation coverage). Ships two template prompts via a sibling
  ``prompts.yaml`` so gateway ``prompts/list`` has something to merge
  from the maya-side backend.
license: MIT
compatibility: Python 3.7+
metadata:
  dcc-mcp.dcc: maya
  dcc-mcp.version: "1.0.0"
  dcc-mcp.layer: example
  dcc-mcp.search-hint: "bake animation, render frame, prompts fixture"
  dcc-mcp.prompts: prompts.yaml
---

# Maya prompts demo

Fixture-only skill for `test_gateway_prompts_aggregation.py` and
`test_e2e_gateway_prompts_sse.py`. Publishes two prompts so aggregation
tests can confirm routing and prefixing across multiple DCC backends.
