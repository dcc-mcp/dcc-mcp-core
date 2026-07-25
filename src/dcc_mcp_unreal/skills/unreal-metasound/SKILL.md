---
name: unreal-metasound
description: >-
  Domain skill — Unreal Engine MetaSound audio graph authoring: create
  MetaSound Source assets, add inputs and graph nodes, connect nodes, set
  defaults, build assets, list nodes, and validate graphs. Use for
  procedural audio design and MetaSound graph creation in Unreal Engine
  5.4+. Not for asset management — use unreal-assets for that. Not for
  Blueprint logic — use unreal-blueprints for gameplay scripting.
license: MIT
compatibility: Unreal Engine 5.4+, Python 3.9+
allowed-tools: Bash Read Write
metadata:
  dcc-mcp:
    dcc: unreal
    version: "1.0.0"
    layer: domain
    stage: authoring
    search-hint: "metasound, audio graph, oscillator, filter, envelope, mixer, wave player, synthesizer, sound design, procedural audio, MetaSound Source"
    tags: "unreal, metasound, audio, graph, authoring, domain"
    tools: tools.yaml
---

# Unreal MetaSound Tools

Authoring tools for Unreal Engine MetaSound audio graphs. Build
procedural audio sources by composing oscillator, filter, envelope,
mixer, and other DSP nodes.

**Target:** Unreal Engine 5.4+ (MetaSound stable API).
**Domain boundary:** audio graph authoring only — no asset management,
no Blueprint logic, no level/actor manipulation.

## Tools

### `unreal_metasound__create_metasound_source`
Create a new MetaSound Source asset under `/Game/`. Returns the asset
path and initial graph handle.

### `unreal_metasound__add_metasound_input`
Add an input parameter (Float, Bool, Int, String, WaveTable, Object) to
an existing MetaSound graph. Returns the input identifier and type.

### `unreal_metasound__add_metasound_node`
Add a DSP node to a MetaSound graph. Node types are validated against a
whitelist: Oscillator, Filter, Envelope, Mixer, WavePlayer, Delay,
Reverb, PitchShift, DynamicsProcessor, Flanger, Chorus.

### `unreal_metasound__connect_metasound_nodes`
Connect two nodes in a MetaSound graph via a source pin → target pin
connection descriptor.

### `unreal_metasound__set_metasound_parameter_default`
Set the default value of a MetaSound input parameter.

### `unreal_metasound__build_metasound`
Compile / build a MetaSound asset. Returns build status and diagnostics.

### `unreal_metasound__list_metasound_nodes`
List every node (name + type) currently in a MetaSound graph. Read-only.

### `unreal_metasound__validate_metasound_graph`
Validate a MetaSound graph for structural issues: no cycles, all inputs
connected, all nodes valid. Returns `compatible: false` + reason when
the Unreal version is below 5.4. Read-only.

## Prerequisites

- Unreal Engine 5.4 or later with MetaSound plugin enabled
- Python 3.9+ with `unreal` module accessible (embedded Python or
  remote execution bridge)

## Connection Model

Connections use a structured descriptor:

```json
{
  "from_node": "Oscillator_1",
  "from_pin": "Audio",
  "to_node": "Filter_1",
  "to_pin": "Input"
}
```

## Safety

- All asset paths are constrained to `/Game/` — absolute or filesystem
  paths outside Content are rejected.
- No `exec()`, `eval()`, `subprocess`, or arbitrary Python execution.
- `import unreal` is always a function-local lazy import so metadata
  parsing (tools.yaml, SKILL.md) never requires a running Unreal
  instance.
