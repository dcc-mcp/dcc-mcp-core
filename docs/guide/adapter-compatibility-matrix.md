# Adapter Compatibility Matrix

This matrix tracks every known DCC-MCP adapter, its core version pin, adapter
version, and supported DCC version range. It should match the adapter entries
in `dcc-mcp-catalog.yml`, because `dcc-mcp-cli install --dcc-type <dcc>` uses
that catalog as its first-party install source. Every adapter release **must**
submit a PR updating this matrix before the release PR merges.

## How to Add a New Adapter

1. Find the next empty row in the table below.
2. Fill in every column with the adapter's latest released values at the time
   of the PR.
3. Submit the PR against `docs/guide/adapter-compatibility-matrix.md`.

## How to Update an Existing Adapter

1. Change the `Adapter Version` and/or `Core Pin` columns to match the new
   release.
2. Update `Last Verified` to the date the release smoke was run.
3. If the DCC minimum version changed, update `DCC Min Version`.

## Matrix

| DCC | Repository | Adapter Version | Core Pin | DCC Min Version | Dispatcher Pattern | Last Verified |
|-----|-----------|----------------|----------|-----------------|-------------------|---------------|
| Maya | [dcc-mcp-maya](https://github.com/dcc-mcp/dcc-mcp-maya) | 0.9.22 | >=0.19.45,<1.0.0 | 2024+ | Qt sidecar + HostUiDispatcherBase | 2026-08 |
| Marmoset Toolbag | [dcc-mcp-marmoset](https://github.com/dcc-mcp/dcc-mcp-marmoset) | 0.1.1 | >=0.19.86,<1.0.0 | 4.03+ | External sidecar + Toolbag periodic callback | 2026-07 |
| OpenSCAD | [dcc-mcp-openscad](https://github.com/dcc-mcp/dcc-mcp-openscad) | 0.1.2 | >=0.19.91,<1.0.0 | 2021.01+ | External OpenSCAD CLI subprocess | 2026-08 |
| FreeCAD | [dcc-mcp-freecad](https://github.com/dcc-mcp/dcc-mcp-freecad) | 0.1.2 | >=0.19.91,<1.0.0 | 1.0+ | External FreeCADCmd subprocess | 2026-08 |
| Cinema 4D | [dcc-mcp-cinema4d](https://github.com/dcc-mcp/dcc-mcp-cinema4d) | 0.1.3 | >=0.19.91,<1.0.0 | R21+ | External headless c4dpy subprocess | 2026-08 |
| ComfyUI | [dcc-mcp-comfyui](https://github.com/dcc-mcp/dcc-mcp-comfyui) | 0.1.1 | >=0.19.91,<1.0.0 | 0.31+ | 17 typed workflow, catalog, queue, and artifact tools over the local REST bridge | 2026-08 |
| Shogun | [dcc-mcp-shogun](https://github.com/dcc-mcp/dcc-mcp-shogun) | 0.8.1 | >=0.19.86,<1.0.0 | Vicon Shogun Post | 66 typed Scene, channel, camera, file, Timeline, editing, production-context, and Offline tools; SDK-dependent surfaces remain capability-gated | 2026-08 |
| Mari | [dcc-mcp-mari](https://github.com/dcc-mcp/dcc-mcp-mari) | 0.2.1 | >=0.19.91,<1.0.0 | 5.0+ | Authenticated loopback sidecar + Qt UI timer | 2026-08 |
| 3ds Max | [dcc-mcp-3dsmax](https://github.com/dcc-mcp/dcc-mcp-3dsmax) | 0.1.40 | >=0.19.45,<1.0.0 | 2025+ | Sidecar + HostPumpController | 2026-08 |
| Blender | [dcc-mcp-blender](https://github.com/dcc-mcp/dcc-mcp-blender) | 0.1.43 | >=0.19.45,<1.0.0 | 3.6+ | In-process MCP + optional diagnostics sidecar | 2026-08 |
| Houdini | [dcc-mcp-houdini](https://github.com/dcc-mcp/dcc-mcp-houdini) | 0.31.5 | >=0.19.45,<1.0.0 | 20.5+ | Event-loop callback | 2026-08 |
| FPT / ShotGrid | [dcc-mcp-fpt](https://github.com/dcc-mcp/dcc-mcp-fpt) | 0.1.8 | >=0.19.45,<1.0.0 | — | REST bridge | 2026-06 |
| Nuke | [dcc-mcp-nuke](https://github.com/dcc-mcp/dcc-mcp-nuke) | 0.13.1 | >=0.19.45,<1.0.0 | — | Host main-thread dispatcher | — |
| Unreal | [dcc-mcp-unreal](https://github.com/dcc-mcp/dcc-mcp-unreal) | 0.3.0 | >=0.19.45,<1.0.0 | — | Unreal Python bridge | — |
| ZBrush | [dcc-mcp-zbrush](https://github.com/dcc-mcp/dcc-mcp-zbrush) | 0.2.18 | >=0.19.45,<1.0.0 | — | Socket bridge + sidecar | — |
| Photoshop | [dcc-mcp-photoshop](https://github.com/dcc-mcp/dcc-mcp-photoshop) | 0.1.37 | >=0.19.45,<1.0.0 | Photoshop UXP | WebSocket bridge | 2026-06 |
| Premiere Pro | [dcc-mcp-premiere](https://github.com/dcc-mcp/dcc-mcp-premiere) | 0.5.0 | >=0.19.45,<1.0.0 | 25.6+ | UXP WebSocket bridge | — |
| After Effects | [dcc-mcp-aftereffects](https://github.com/dcc-mcp/dcc-mcp-aftereffects) | 0.6.0 | >=0.19.91,<1.0.0 | — | Authenticated CEP bridge + broker | 2026-08 |
| Illustrator | [dcc-mcp-illustrator](https://github.com/dcc-mcp/dcc-mcp-illustrator) | 0.2.0 | >=0.19.91,<1.0.0 | — | Authenticated CEP bridge + broker | 2026-08 |
| GIMP | [dcc-mcp-gimp](https://github.com/dcc-mcp/dcc-mcp-gimp) | 0.3.0 | >=0.19.38,<1.0.0 | 3.0+ | Authenticated JSON-lines bridge + GLib main-thread dispatcher | — |
| Krita | [dcc-mcp-krita](https://github.com/dcc-mcp/dcc-mcp-krita) | 0.3.0 | >=0.19.38,<1.0.0 | — | Authenticated JSON-lines bridge + UI main-thread queue | — |
| SketchUp | [dcc-mcp-sketchup](https://github.com/dcc-mcp/dcc-mcp-sketchup) | 0.1.0 | >=0.19.91,<1.0.0 | 2021+ | Authenticated Ruby main-thread bridge + sidecar | 2026-08 |
| TouchDesigner | [dcc-mcp-touchdesigner](https://github.com/dcc-mcp/dcc-mcp-touchdesigner) | 0.1.1 | >=0.19.91,<1.0.0 | 2025 official build | 19 typed operator graph, parameter, DAT, timeline, capture, and project tools through the in-process HTTP runtime + `td.run()` main-thread dispatcher | 2026-08 |
| Tiled | [dcc-mcp-tiled](https://github.com/dcc-mcp/dcc-mcp-tiled) | 0.3.0 | >=0.19.38,<1.0.0 | 1.10+ | Standalone service + fixed JavaScript driver through `tiled --evaluate` | 2026-08 |
| Material Maker | [dcc-mcp-material-maker](https://github.com/dcc-mcp/dcc-mcp-material-maker) | 0.3.1 | >=0.19.38,<1.0.0 | 1.7 | Standalone `.ptex` parser + native CLI exporter | 2026-08 |
| Wwise | [dcc-mcp-wwise](https://github.com/dcc-mcp/dcc-mcp-wwise) | 0.1.2 | >=0.19.86,<1.0.0 | 2024.1+ | External WAAPI client + host PID binding | 2026-08 |
| Katana | [dcc-mcp-katana](https://github.com/dcc-mcp/dcc-mcp-katana) | 0.4.0 | >=0.19.45,<1.0.0 | — | Host main-thread dispatcher | — |
| MotionBuilder | [dcc-mcp-mobu](https://github.com/dcc-mcp/dcc-mcp-mobu) | 0.3.0 | >=0.19.45,<1.0.0 | — | Host main-thread dispatcher | — |
| RenderDoc | [dcc-mcp-renderdoc](https://github.com/dcc-mcp/dcc-mcp-renderdoc) | 0.3.0 | >=0.19.45,<1.0.0 | — | Headless CLI adapter | 2026-07 |
| Substance 3D Designer | [dcc-mcp-substance3d-designer](https://github.com/dcc-mcp/dcc-mcp-substance3d-designer) | 0.3.0 | >=0.19.45,<1.0.0 | — | Host bridge | — |
| Substance 3D Painter | [dcc-mcp-substance3d-painter](https://github.com/dcc-mcp/dcc-mcp-substance3d-painter) | 0.1.3 | >=0.19.3,<1.0.0 | — | Host bridge | — |
| Godot | [dcc-mcp-godot](https://github.com/dcc-mcp/dcc-mcp-godot) | 0.4.0 | >=0.19.45,<1.0.0 | 4.x | EditorPlugin + runtime bridge | 2026-07 |
| Unity | [dcc-mcp-unity](https://github.com/dcc-mcp/dcc-mcp-unity) | 0.11.2 | >=0.19.45,<1.0.0 | 2018.4.36f1+ | EditorApplication.update + WebSocket bridge | — |
| OpenUSD | [dcc-mcp-openusd](https://github.com/dcc-mcp/dcc-mcp-openusd) | 0.8.1 | >=0.19.45,<1.0.0 | — | Headless USD stage adapter | 2026-07 |
| Custom Studio Tool | _(your repo here)_ | _your version_ | _your pin_ | _your min_ | _your pattern_ | _date_ |

Tiled, Material Maker, and Wwise remain discoverable source projects, but the
bundled catalog omits automatic install metadata until each project publishes a
wheel that can be pinned by URL and SHA-256.

## Column Reference

| Column | Description |
|--------|------------|
| **DCC** | Canonical DCC name (lowercase, kebab-case). |
| **Repository** | GitHub URL for the adapter source code. |
| **Adapter Version** | Latest released semver of the adapter. |
| **Core Pin** | Dependency range for `dcc-mcp-core`. Must exclude `<1.0.0` until core reaches 1.0. |
| **DCC Min Version** | Minimum host version (e.g. `2024+`, `3.6+`, `20.5+`). |
| **Dispatcher Pattern** | A short summary of the adapter's runtime routing model, such as `Qt sidecar`, `Event-loop callback`, `InProcessCallableDispatcher`, diagnostics-only sidecar, or an external bridge. See `skills/dcc-mcp-creator/references/HOST_PATTERN_MATRIX.md` for details. |
| **Last Verified** | Month the last gateway smoke was run (format: `YYYY-MM`). |

## Core Version Policy

- Adapters **must** pin `dcc-mcp-core` with an open upper bound: `>=X.Y.0,<1.0.0`.
- The lower bound (`X.Y.0`) must be a **released** minor version of core.
  Never pin to `main` or a pre-release.
- When core bumps its minor version, adapter pins should be updated within one
  adapter release cycle.
- Major version zero (`0.x.y`) means breaking changes can happen at any minor
  bump; the `<1.0.0` guard ensures adapters don't silently consume a breaking
  core change.

## Legend

| Marker | Meaning |
|--------|---------|
| ⏳ | Release tag pending — adapter PR in review, version subject to change. Remove marker after tag. |

## CLI Catalog Contract

`dcc-mcp-catalog.yml` is the install source for first-party adapters. When an
adapter row above changes, update the matching catalog entry in the same PR:

- `name`, `url`, `version`, and `min_core_version` must match this matrix.
- Adapter rows must have the `adapter` tag and install metadata.
- Adapter install metadata should include `instructions_url` pointing at the
  adapter-maintained raw `install.md` so `dcc-mcp-cli install` can hand agents
  the current host-specific setup runbook.
- Skill pack rows may share the same `dcc`, but must not be selected by
  `dcc-mcp-cli install --dcc-type <dcc>` when an adapter row exists.

## Outdated Policy

An adapter row is considered **stale** when:

- `Last Verified` is more than 6 months old, **or**
- `Core Pin` lower bound is more than 2 minor versions behind the latest core
  release.

Stale rows are flagged in the core release PR notes. Adapter maintainers should
prioritise a compatibility update before the next core release.
