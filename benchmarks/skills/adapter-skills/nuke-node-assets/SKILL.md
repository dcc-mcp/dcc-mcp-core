---
name: nuke-node-assets
description: >-
  Package reusable Nuke node graphs as versioned Gizmos, instantiate saved
  Gizmos, and inspect their public knob interface and graph health.
license: MIT
compatibility: "Nuke 13+ Python API; dcc-mcp-core 0.19+"
allowed-tools: Python
metadata:
  dcc-mcp:
    dcc: nuke
    version: "1.0.0"
    layer: domain
    stage: compositing
    search-hint: "nuke gizmo group reusable node asset package expose knobs instantiate inspect validate version"
    tags: "nuke,compositing,gizmo,group,asset,reuse"
    tools: tools.yaml
---

# Nuke Node Assets

Use `gizmo_create_from_group` for portable production assets. It publishes an
existing Group under the isolated root configured by
`DCC_MCP_NUKE_PLUGIN_ROOT`, with a stable dotted id, semantic version, typed
bounded knobs, and ordered input contract. Executable callbacks are rejected.
Use `gizmo_instantiate` by id/version and `gizmo_validate` for hash, dependency,
clean-load, render, and channel-preservation evidence.

The earlier path-based `package_gizmo`, `instantiate_gizmo`, and
`inspect_gizmo` tools remain for compatibility. They do not provide the
registered-asset security contract and should not be used for untrusted assets.
`inspect_gizmo` accepts both legacy and registered-asset manifest knob layouts.
`package_gizmo` refuses existing and concurrently-created paths unless
`overwrite=true` is explicit.

Package a new version to a new filename. Instance-preserving graph upgrades are
not part of this skill yet.
