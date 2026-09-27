---
name: aftereffects-project
description: >-
  Host skill - inspect, edit, save, and render After Effects projects through
  typed adobepy facades. Use when working with AEP projects and motion intros.
license: MIT
compatibility: "After Effects CEP/ExtendScript; dcc-mcp-core >=0.20.14,<1.0.0"
allowed-tools: Python
metadata:
  dcc-mcp:
    dcc: aftereffects
    version: "0.2.0"
    layer: domain
    stage: scene
    search-hint: "after effects project composition inspect save render aep motion intro template stock video music"
    tags: "adobe, aftereffects, compositing, animation"
    tools: tools.yaml
---

# After Effects Project

## Installation and readiness

Use the adapter-owned install runbook for packaged installs, adobepy
provisioning, generated CEP bridges, upgrades, receipts, and uninstall. For an
Internal deployment with an approved prebuilt CEP bridge:

1. Run `dcc-mcp-cli doctor` to inspect the local CLI and gateway.
2. Run `dcc-mcp-cli install --dcc-type aftereffects` with the approved bridge
   root in `--plugin-source` and the Adobe CEP extension root in
   `--adobe-debug-root`. Internal profiles may provide
   `DCC_MCP_PLUGIN_SOURCE` and `DCC_MCP_ADOBE_DEBUG_ROOT`.
3. Restart After Effects if it has cached the extension.
4. Run `dcc-mcp-cli list` and
   `dcc-mcp-cli wait-ready --dcc-type aftereffects` before loading this skill.

The bridge root must contain its manifest and be selected by an approved
catalog or Internal descriptor. This skill does not create links, copy bridge
files, or treat a filesystem link as proof of a loaded CEP session. Confirm
readiness through the CLI and adapter status.
