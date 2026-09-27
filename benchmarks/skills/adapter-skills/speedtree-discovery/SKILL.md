---
name: speedtree-discovery
description: >-
  Inspect an installed SpeedTree runtime, discover official samples and export
  presets, inspect preset options, fingerprint supported assets, plan an
  export, and preflight Unreal integration. Read-only operations do not require
  a C++ hook.
license: MIT
compatibility: "SpeedTree Modeler/Compiler installation; dcc-mcp-core 0.20+"
allowed-tools: "python"
metadata:
  dcc-mcp:
    dcc: speedtree
    layer: domain
    version: "0.1.0"
    search-hint: "SpeedTree capabilities coverage wind LOD modeling limitations inspect runtime discover samples presets asset export plan Unreal preflight"
    tags: "speedtree,modeler,compiler,spm,srt,export,unreal,inspection"
    tools: tools.yaml

---

# SpeedTree discovery

Start with `inspect_capabilities` to distinguish implemented file operations
from unavailable Modeler editing. It needs no GUI binding and does not probe
the license or current document. `inspect_runtime` separately verifies an exact
PID/HWND; it does not inspect generator state. These tools never inject code,
modify the SpeedTree process, or create output directories. Use the packaged
`speedtree-export` skill for official CLI export.
