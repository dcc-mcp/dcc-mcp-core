---
name: nuke-script
description: >-
  Host skill - open, inspect, and explicitly save Nuke scripts. Use when
  restoring a known .nk file, checking bounded node topology and knob values,
  Read channels, sampled channel statistics or multipart parts, or saving a
  known path. Not for raw Python execution.
license: MIT
compatibility: "Nuke Python API; dcc-mcp-core 0.19+"
allowed-tools: Python
metadata:
  dcc-mcp:
    dcc: nuke
    version: "0.0.0"
    layer: domain
    stage: scene
    search-hint: "nuke script root selected nodes topology knob values read channels AOV statistics min max mean zero NaN Inf multipart parts open inspect save nk"
    tags: "nuke, compositing, nodes, script"
    tools: tools.yaml
---

# Nuke Script

Use `nuke_script__open_script` to restore an existing absolute `.nk` path. The
tool verifies the active script path, Root frame range, and loaded node count
before reporting success. Opening replaces the current graph, so save any
unsaved work first or explicitly set `discard_unsaved_changes` to `true`.

Use `nuke_script__sample_channel_statistics` for bounded diagnostics. Its
min/max/mean and `all_zero` values describe uniform filtered tile samples; set
the ROI and sample limit explicitly when small or sparse features matter.

Use `nuke_script__list_nodes` before and after graph edits to compare exact
node classes, connected input indices, and bounded non-executable knob values.
