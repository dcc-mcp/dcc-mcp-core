---
name: legacy-form-skill
description: Pre-0.15 legacy SKILL.md fixture (issue #356) — exercises the backward-compatible dual-read path. Same semantics as new-form-skill but with all dcc-mcp-core extensions at top level.
license: MIT
dcc: maya
version: "1.2.3"
tags: [modeling, polygon, bevel]
search-hint: "bevel edges mesh polygon modeling"
tools:
  - name: bevel
    description: Apply a bevel to the selected edges.
    read_only: false
    destructive: true
    idempotent: false
    source_file: scripts/bevel.py
  - name: measure
groups:
  - name: advanced
    description: Advanced modeling tools not needed for most workflows.
    default-active: false
    tools: [bevel]
---

# Legacy-form skill

This fixture declares all dcc-mcp-core extensions at the YAML top level,
the way skills were authored before v0.15. The loader still accepts this
form but emits a deprecation warning and `is_spec_compliant()` returns
`False`.
