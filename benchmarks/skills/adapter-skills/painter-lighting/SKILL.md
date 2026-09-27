---
name: painter-lighting
description: >-
  Inspect and control Substance 3D Painter scene lighting. Painter has no light
  objects, so lighting is the environment map and its exposure and rotation.
  Use when setting up an HDRI, matching a look-dev environment, or reading the
  current lighting configuration. Not for lamps, shadows, or light rigs.
license: MIT
compatibility: "Substance 3D Painter Python API; dcc-mcp-core 0.20.15+"
allowed-tools: Python
metadata:
  dcc-mcp:
    dcc: substance3d_painter
    version: "1.0.0"
    layer: domain
    stage: pipeline
    search-hint: "substance painter lighting environment hdri exposure rotation background ibl lookdev"
    tags: "substance, painter, lighting, environment, hdri, ibl, exposure, rotation, background"
    tools: tools.yaml
---

# Painter lighting

Substance 3D Painter has no light objects, no shadow-casting rig, and no
per-light controls. All scene illumination comes from a single image-based
environment, so this skill maps the lighting domain onto Painter's real
environment primitives:

| Lighting concept | Painter native equivalent |
| --- | --- |
| HDRI / IBL | environment map resource |
| Light intensity | environment exposure |
| Light direction / key angle | environment rotation |
| Backplate | background texture resource |

Call `inspect_environment` first. It reports both the current values *and* which
environment capabilities the running Painter build actually exposes, so you can
see what is controllable before attempting a mutation.

Painter's environment module is spelled with the French `environnement`, and the
individual accessor names have differed between releases. Every tool here probes
a bounded set of known spellings and never guesses: if a capability is absent,
the tool fails with `environment_capability_unsupported` and names what it
looked for, rather than silently doing nothing.

A malformed resource URL is a different failure and is reported as
`invalid_resource_url`; it is a caller input error, not a host capability gap.
Read `capabilities` from `inspect_environment` to see whether the running
Painter build supports a given operation before attempting it. `readable` means
the host exposes a getter, not that the current value is populated, so an unset
background texture is still reported as readable.

Exposure is a scalar in stops and rotation is expressed in turns on the interval
`0.0` to `1.0`. Environment and background maps are addressed by Painter
resource URL; import an `.hdr` or `.exr` file with
`painter_project__import_resource` using the `environment` usage and pass the
URL it returns. Every mutation is confirmed by reading the value back from
Painter before reporting success.
