---
name: unreal-automotive-rain-film
description: >-
  Build, audit, and render a UE 5.8 rain-soaked Audi A5 promotional film from
  Epic's Automotive Configurator sample. Use for the official BP_AudiA5 asset,
  wet PBR LookDev, Niagara rain/impact fields, HDRI lighting, cinematic DOF,
  and 4K Movie Render Queue output. Not for placeholder or low-poly cars.
license: MIT
compatibility: Unreal Engine 5.8+, Python 3.11+
metadata:
  dcc-mcp:
    dcc: unreal
    version: "0.1.0"
    layer: workflow
    stage: production
    search-hint: "UE 5.8 Automotive Configurator Audi A5 rain wet PBR Niagara HDRI cinematic 4K MRQ"
    tags: [unreal, automotive, audi, lookdev, niagara, hdri, cinematic, render]
    tools: tools.yaml
---

# UE 5.8 Automotive Rain Film

This workflow preserves Epic's official `BP_AudiA5` assembly and Automotive
materials. It builds a dedicated wet stage around that asset, grounds the car
from its component bounds, adds animated day-to-night HDRI and multi-light
reflections, Niagara rain/impact/water-wash particle fields, atmospheric
perspective, five DOF shots, and a 10-second 4K sequence.

Run `audit_audi_asset` first. Run `build_audi_rain_film`, inspect the returned
preview, render the five visual gates with `render_audi_phase_stills`, then use
`render_audi_rain_film` for final Movie Render Queue frames.

## Scripts

- `audit_audi_asset.py` measures the official Blueprint assembly.
- `build_audi_rain_film.py` authors the level, lighting phases, particles,
  cameras, and sequence.
- `render_audi_phase_stills.py` renders day, dusk, night, storm-wash, and dawn
  acceptance frames without relying on the editor viewport.
- `render_audi_rain_film.py` starts the final 4K Movie Render Queue job.
