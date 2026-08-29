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

## 1. Install SOP v1

The release MUST conform to
[Adapter Install SOP v1](adapter-install-sop.md):

- [ ] Root `install.md` is based on
      [the reusable template](templates/adapter-install.md) and contains
      Requirements / Supported versions / Agent quick path / Manual path /
      Verify / Upgrade / Uninstall / Troubleshooting.
- [ ] The adapter exposes
      `dcc-mcp-<dcc> install|status|verify|uninstall|upgrade` with the uniform
      `--json --yes --dry-run --dcc-path --python` flags where applicable.
- [ ] Every verb emits schema version 1 and validates against the packaged
      `dcc_mcp_core/schemas/adapter-install-sop-v1.schema.json` resource.
- [ ] Exit codes keep the `0/10/20/30/40/50` mapping exported by
      `dcc_mcp_core.deployment`.
- [ ] Install and upgrade stage changes, preserve the previous state until
      commit, and restore that state plus its receipt on failure.
- [ ] Uninstall is receipt-driven, idempotent, and refuses ambiguous
      unreceipted user-owned files.
- [ ] Verify checks artifact digest, target-interpreter package/import,
      host enablement/bootstrap state, and a typed readiness probe when the
      host can run. `directly_usable` is never inferred from copied files or a
      process alone.
- [ ] Bootstrap failures remain visible through `capture_bootstrap_errors` or
      an equivalent structured host-owned record.
- [ ] CI proves plan -> execute -> verify -> status -> uninstall, dry-run
      no-mutation, receipt round-trip, idempotency, and rollback fault
      injection. Any unavailable live-host smoke is stated as a release gap.
- [ ] Catalog `instructions_url` points to:
      `https://raw.githubusercontent.com/dcc-mcp/dcc-mcp-<dcc>/main/install.md`.

Do not substitute a bespoke installer executable, prose-only next steps,
delete-then-copy overwrite, or README-only instructions.

## 2. Required Sidecar Metadata

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

## 3. Gateway Smoke Steps

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

## 4. Release-Please & Tag Naming

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

### Generated-lock credential boundary

When a release-please or Renovate pull request needs generated lock updates,
copy the boundary implemented by
`.github/workflows/release-please-lock-sync.yml` and
[`scripts/ci/generated_lock_sync.py`](../../scripts/ci/generated_lock_sync.py):

- declare `contents: read` for the job and set
  `persist-credentials: false` on checkout;
- run every generator with the scrubbed environment provided by
  `generated_lock_sync.py`, which removes write tokens, disables Git prompts,
  and ignores global/system Git configuration;
- bind repository, PR number, head repository, branch, title, and exact head
  SHA, then re-capture them before committing; reject forks, stale heads, and
  branches outside the approved release/automation classes;
- reject any generated diff outside `Cargo.lock`, `uv.lock`, and
  `crates/workspace-hack/Cargo.toml`;
- expose a write token only to the final fixed `--force-with-lease` push, and
  unset it on exit. A failed lease or any identity mismatch must result in no
  remote mutation.

Do not broaden this workflow to publish packages or alter release gates. Keep
the same checks and output allowlist when adapting the pattern downstream.

### CHANGELOG Convention

Use [Conventional Commits](https://www.conventionalcommits.org/) as the source
of truth. Merge the release-please PR to generate the changelog. Do not write
changelog entries by hand.

## 5. Validation Gates

Before the release PR is merged, run these gates:

- [ ] `ruff check src tests`
- [ ] `ruff format --check src tests`
- [ ] `pytest` (unit + integration)
- [ ] Release-please PR guard passes (if the repo has one)
- [ ] Gateway smoke commands run without error (Section 3)
- [ ] Install SOP v1 checks and JSON-schema contract tests pass (Section 1)
- [ ] Core dependency range is still valid: `>=0.<core_latest>.0,<1.0.0`

## 6. PR Notes Template

The release PR description must include:

```markdown
## Summary

<!-- One-line summary of what this release changes -->

## Validation

<!-- Paste the gateway smoke output (Section 3) -->

## Compatibility

- **core pin**: `>=0.<N>.0,<1.0.0`
- **adapter_version**: `<version>`
- **DCC version**: `<minimum DCC version>`

## Gaps

<!-- Any live-DCC test gap, known issues, deferred features -->
```

## 7. Post-Release

- [ ] Compatibility matrix row is merged into core `main`.
- [ ] If the release changes an established adapter pattern (dispatcher wiring,
      readiness, resource registration), the `dcc-mcp-creator` skill references
      in core are updated accordingly.
- [ ] If a new major.minor of core was bumped, re-run gateway smoke on the
      adapter to confirm no regressions.
