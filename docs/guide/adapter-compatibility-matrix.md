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
| Maya | [dcc-mcp-maya](https://github.com/dcc-mcp/dcc-mcp-maya) | 0.8.6 | >=0.18.20,<1.0.0 | 2024+ | Qt sidecar + HostUiDispatcherBase | 2026-06 |
| 3ds Max | [dcc-mcp-3dsmax](https://github.com/dcc-mcp/dcc-mcp-3dsmax) | 0.1.19 | >=0.18.20,<1.0.0 | 2025+ | Sidecar + HostPumpController | 2026-06 |
| Blender | [dcc-mcp-blender](https://github.com/dcc-mcp/dcc-mcp-blender) | 0.1.13 | >=0.18.9,<1.0.0 | 3.6+ | In-process MCP + optional diagnostics sidecar | 2026-06 |
| Houdini | [dcc-mcp-houdini](https://github.com/dcc-mcp/dcc-mcp-houdini) | 0.6.1 | >=0.18.14,<1.0.0 | 20.5+ | Event-loop callback | 2026-06 |
| FPT | [dcc-mcp-fpt](https://github.com/dcc-mcp/dcc-mcp-fpt) | 0.2.0 | >=0.18.0,<1.0.0 | — | REST bridge | 2026-06 |
| Nuke | _(planned)_ | — | — | — | — | — |
| Unreal | _(planned)_ | — | — | — | — | — |
| ZBrush | _(planned)_ | — | — | — | — | — |
| Photoshop | [dcc-mcp-photoshop](https://github.com/dcc-mcp/dcc-mcp-photoshop) | 0.1.15 | >=0.18.14,<1.0.0 | Photoshop UXP | WebSocket bridge | 2026-06 |
| Custom Studio Tool | _(your repo here)_ | _your version_ | _your pin_ | _your min_ | _your pattern_ | _date_ |

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
