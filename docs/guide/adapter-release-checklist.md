# Adapter Release Train Checklist

Use this checklist when cutting a release for any DCC-MCP adapter (Maya, Blender,
Houdini, 3ds Max, Nuke, ZBrush, Photoshop, Unreal, custom studio tool). Follow it
in order. Tick every box before merging the release PR.

## 0. Pre-Release Preparation

- [ ] Core dependency is pinned to `>=0.<latest_minor>.0,<1.0.0` in `pyproject.toml`.
      Use the latest *released* minor version of `dcc-mcp-core`, not `main`.
      Example: `"dcc-mcp-core>=0.18.0,<1.0.0"`.
- [ ] `dcc-mcp-server` binary dependency (if used) also follows the same range.
- [ ] Adapter `adapter_version` is set via `DccServerOptions.from_env(..., adapter_version=...)`
      and stamped into the gateway sentinel and file registry row.
- [ ] Compatibility matrix in the core docs has been updated with the new row
      (see [Adapter Compatibility Matrix](adapter-compatibility-matrix.md)).

## 1. Required Sidecar Metadata

Every adapter that launches a sidecar (`dcc-mcp-server sidecar`) must expose
these metadata fields in the discovery and registry records:

| Metadata key | Source | Example |
|---|---|---|
| `dcc_type` | `DccName::parse` or `dcc_name` | `"maya"` |
| `adapter_version` | `DccServerOptions.adapter_version` | `"1.2.3"` |
| `dcc_version` | Runtime-reported host version | `"2026.3"` |
| `dispatch_contract` | `build_sidecar_command().dispatch_contract` | `remote` or `sidecar` |
| `dcc_pid` | `DccServerOptions.dcc_pid` or OS PID | `12345` |

Declare these in the adapter's `start_server()` or composition root so they
flow into `gateway://instances` and `POST /v1/instances`.

## 2. Gateway Smoke Steps

Copy these steps from `TESTING_AND_RELEASE.md` into the release PR notes.
Adapt the port and DCC name as needed:

```bash
# 1. Start the adapter (or ensure it is running)
# 2. Gateway readiness
curl -s http://127.0.0.1:9765/v1/readyz | python -m json.tool

# 3. Discover a skill through gateway search
curl -s -X POST http://127.0.0.1:9765/v1/search \
  -H 'Content-Type: application/json' \
  -d '{"query": "ping", "dcc_type": "<dcc_name>"}'

# 4. Describe a discovered tool
curl -s -X POST http://127.0.0.1:9765/v1/describe \
  -H 'Content-Type: application/json' \
  -d '{"tool_slug": "<dcc>.<id8>.<tool>"}'

# 5. Call one safe typed tool
curl -s -X POST http://127.0.0.1:9765/v1/call \
  -H 'Content-Type: application/json' \
  -d '{"tool_slug": "<dcc>.<id8>.ping", "arguments": {}}'

# 6. Verify instance rows
curl -s http://127.0.0.1:9765/v1/instances | python -m json.tool

# 7. Check gateway diagnostics
curl -s http://127.0.0.1:9765/admin/api/health | python -m json.tool
```

If the real DCC is unavailable, mock the HTTP test in CI and document the manual
smoke command in the adapter repository.

## 3. Release-Please & Tag Naming

### Tag Convention

- **Per-adapter repos** use their own release-please config with `release-type: python`.
  Tags are prefixed with the major.minor.patch of that adapter, *not* core.
  Example: `v1.2.3` for `dcc-mcp-maya`.

- **Core mono-repo** tags the root package at `v<semver>` (e.g. `v0.18.0`).

### Release-Please Setup

Every adapter repository should include a `release-please-config.json`:

```json
{
  "release-type": "python",
  "include-v-in-tag": true,
  "packages": {
    ".": {
      "package-name": "dcc-mcp-<dcc>",
      "include-component-in-tag": false,
      "bump-minor-pre-major": true,
      "bump-patch-for-minor-pre-major": true,
      "extra-files": [
        {
          "type": "generic",
          "path": "src/adapter/__init__.py"
        }
      ]
    }
  },
  "changelog-sections": [
    {"type": "feat", "section": "Features", "hidden": false},
    {"type": "fix", "section": "Bug Fixes", "hidden": false},
    {"type": "perf", "section": "Performance Improvements", "hidden": false},
    {"type": "docs", "section": "Documentation", "hidden": false}
  ]
}
```

Refer to the core `.release-please-manifest.json` at
[release-please-config.json](../../release-please-config.json) for the canonical pattern.

### CHANGELOG Convention

Use [Conventional Commits](https://www.conventionalcommits.org/) as the source
of truth. Merge the release-please PR to generate the changelog. Do not write
changelog entries by hand.

## 4. Validation Gates

Before the release PR is merged, run these gates:

- [ ] `ruff check src tests`
- [ ] `ruff format --check src tests`
- [ ] `pytest` (unit + integration)
- [ ] Release-please PR guard passes (if the repo has one)
- [ ] Gateway smoke commands run without error (Section 2)
- [ ] Core dependency range is still valid: `>=0.<core_latest>.0,<1.0.0`

## 5. PR Notes Template

The release PR description must include:

```markdown
## Summary

<!-- One-line summary of what this release changes -->

## Validation

<!-- Paste the gateway smoke output (Section 2) -->

## Compatibility

- **core pin**: `>=0.<N>.0,<1.0.0`
- **adapter_version**: `<version>`
- **DCC version**: `<minimum DCC version>`

## Gaps

<!-- Any live-DCC test gap, known issues, deferred features -->
```

## 6. Post-Release

- [ ] Compatibility matrix row is merged into core `main`.
- [ ] If the release changes an established adapter pattern (dispatcher wiring,
      readiness, resource registration), the `dcc-mcp-creator` skill references
      in core are updated accordingly.
- [ ] If a new major.minor of core was bumped, re-run gateway smoke on the
      adapter to confirm no regressions.
