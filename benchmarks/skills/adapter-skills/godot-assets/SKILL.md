---
name: godot-assets
description: >-
  Infrastructure skill - plan, install, and enable downloaded Godot asset packages.
  Use after an asset-provider skill returns a local ZIP package. Not for searching
  remote stores; use a provider skill such as godot-asset-store.
license: MIT
compatibility: "Godot 4.4+; dcc-mcp-core 0.19+"
allowed-tools: "python"
metadata:
  dcc-mcp:
    dcc: godot
    layer: infrastructure
    stage: bootstrap
    version: "0.1.0"
    search-hint: "Godot asset package addon ZIP install project template enable plugin"
    tags: "godot,asset,addon,package,install,plugin"
    tools: tools.yaml
    depends: "dcc-diagnostics"
---

# Godot Assets

Plan every package before installation. Downloads stay in provider skills; this skill owns safe
ZIP extraction, project placement, filesystem refresh, and explicit plugin enablement.
