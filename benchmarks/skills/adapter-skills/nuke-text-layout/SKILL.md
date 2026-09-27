---
name: nuke-text-layout
description: >-
  Host skill - create or update a bounded Nuke Text2 label with deterministic
  font scale, node position, text box, alignment, and verified readback.
license: MIT
compatibility: "Nuke 15+ Python API; dcc-mcp-core 0.20+"
allowed-tools: Python
metadata:
  dcc-mcp:
    dcc: nuke
    version: "0.0.0"
    layer: domain
    stage: scene
    search-hint: "nuke Text2 text label title font size layout position box alignment"
    tags: "nuke, compositing, Text2, labels, layout"
    tools: tools.yaml
---

# Nuke Text Layout

Use `upsert_text2_label` for one named `Text2` node. It is idempotent: an
existing `Text2` is updated, a missing node is created, and a same-name node of
any other class is rejected. The tool accepts bounded Unicode plain text
without brackets, backslashes, or control characters, plus a requested
pixel size, graph position, text box, and fixed alignment enums. It does not
accept a node class, Python, Tcl, scripts, callbacks, expressions, or UI input.
Every required Text2 knob must be static under Nuke's aggregate knob-state
probes (`isAnimated`, `hasExpression`, all animation curves, and key count).
The boolean probes, finite curve list and non-negative integer key count must
have their documented result shapes. An animated, keyed, expression-bearing,
unsupported, or unobservable knob state fails before an existing node is
changed; the tool never flattens multi-frame or multi-view animation to a
current-frame value.

Nuke 15's effective Text2 sizing behavior is `global_font_scale`, not the inert
`font_size` knob. The tool maps `font_size_px / 64.0` to
`global_font_scale`, then returns the actual scale, derived pixel size, graph
position, box, and alignment. A mismatched readback rolls back an update or
deletes a partially created node; it never reports success without a match.
Scale, every box coordinate, and both graph-position coordinates must read back
as finite non-boolean numeric values. The box must be an exact four-element
list or tuple; unsupported containers and malformed lengths fail closed.

The registered route is synchronous, main-thread-affine, and has a 30-second
deadline hint. Save the Nuke script separately only after layout assertions
pass.
