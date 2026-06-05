# Adapter Compatibility Matrix

This matrix tracks every known DCC-MCP adapter, its core version pin, adapter
version, and supported DCC version range. Every adapter release **must** submit
a PR updating this matrix before the release PR merges.

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
| Maya | [dcc-mcp-maya](https://github.com/dcc-mcp/dcc-mcp-maya) | 0.3.0 | >=0.18.0,<1.0.0 | 2024+ | Qt sidecar + HostUiDispatcherBase | 2026-06 |
| 3ds Max | [dcc-mcp-3dsmax](https://github.com/dcc-mcp/dcc-mcp-3dsmax) | 0.2.0 | >=0.18.0,<1.0.0 | 2025+ | Qt sidecar + HostPumpController | 2026-05 |
| Blender | [dcc-mcp-blender](https://github.com/dcc-mcp/dcc-mcp-blender) | 0.1.0 | >=0.17.0,<1.0.0 | 3.6+ | Background blocking dispatcher | 2026-04 |
| Houdini | [dcc-mcp-houdini](https://github.com/dcc-mcp/dcc-mcp-houdini) | 0.1.0 | >=0.17.0,<1.0.0 | 20.5+ | Event-loop callback | 2026-04 |
| Nuke | _(planned)_ | — | — | — | — | — |
| Unreal | _(planned)_ | — | — | — | — | — |
| ZBrush | _(planned)_ | — | — | — | — | — |
| Photoshop | _(planned)_ | — | — | — | — | — |
| Custom Studio Tool | _(your repo here)_ | _your version_ | _your pin_ | _your min_ | _your pattern_ | _date_ |

## Column Reference

| Column | Description |
|--------|------------|
| **DCC** | Canonical DCC name (lowercase, kebab-case). |
| **Repository** | GitHub URL for the adapter source code. |
| **Adapter Version** | Latest released semver of the adapter. |
| **Core Pin** | Dependency range for `dcc-mcp-core`. Must exclude `<1.0.0` until core reaches 1.0. |
| **DCC Min Version** | Minimum host version (e.g. `2024+`, `3.6+`, `20.5+`). |
| **Dispatcher Pattern** | One of: `Qt sidecar`, `Background blocking`, `Event-loop callback`, `InProcessCallableDispatcher`, `External bridge`. See [HOST_PATTERN_MATRIX.md](../../skills/dcc-mcp-creator/references/HOST_PATTERN_MATRIX.md) for details. |
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

## Outdated Policy

An adapter row is considered **stale** when:

- `Last Verified` is more than 6 months old, **or**
- `Core Pin` lower bound is more than 2 minor versions behind the latest core
  release.

Stale rows are flagged in the core release PR notes. Adapter maintainers should
prioritise a compatibility update before the next core release.
