---
name: painter-material
description: >-
  Inspect and author the channel-level material surface of a Substance 3D Painter
  layer stack. Use when reading which channels a stack carries, enabling or
  disabling the channels a layer contributes, or setting uniform channel values.
  Not for creating layers, importing textures, or painting strokes.
license: MIT
compatibility: "Substance 3D Painter Python API; dcc-mcp-core 0.20.15+"
allowed-tools: Python
metadata:
  dcc-mcp:
    dcc: substance3d_painter
    version: "1.0.0"
    layer: domain
    stage: pipeline
    search-hint: "substance painter material channel basecolor metallic roughness normal height enable disable uniform value"
    tags: "substance, painter, material, texture, channel, basecolor, metallic, roughness, shader"
    tools: tools.yaml
---

# Painter material

Painter's material surface is a set of typed channels carried by a texture-set
stack, plus the per-layer choice of which of those channels a layer actually
contributes to. This skill works directly on that model:

- `inspect_material_channels` reads the stack's channel inventory - label,
  storage format, bit depth, and whether the channel is colour data. Use it to
  discover the exact channel names a stack supports before addressing one.
- `set_layer_channels` replaces the set of channels a layer contributes. Painter
  calls this the layer's active channels, and it is the switch that decides
  whether a layer writes colour, roughness, height, and so on.
- `set_layer_channel_value` writes one uniform value into one channel of an
  existing layer. This is the primitive behind a flat fill; it does not paint.
  The target channel must already be one the layer contributes; use
  `set_layer_channels` to activate it first, otherwise this tool fails with
  `HOST_READBACK_MISMATCH` and a prompt naming the channel to activate.

Channel names are resolved against `textureset.ChannelType` on the running host
rather than being hard-coded, because the available channels depend on the
project's shader. An unknown name fails with the supported list attached.

`inspect_material_channels` is read-only. The two mutating tools do not delete
anything and are idempotent: re-applying the same channel set or the same value
converges to the same state.

**Verification contract.** Channel activation is confirmed by reading the layer's
active channels back from Painter, so it is reported as verified. A uniform
channel *value* has no host readback in Painter's Python API, so
`set_layer_channel_value` confirms only that the target channel is active
afterwards and reports `verified=False` for the value itself. Do not treat it as
proof that the intended colour reached the texture.
