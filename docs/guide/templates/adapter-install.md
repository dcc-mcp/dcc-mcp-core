# Install dcc-mcp-DCC_SLUG

<!--
Replace every `{placeholder}`. Keep this runbook aligned with executable tests
and the adapter catalog instructions_url. Do not include workstation-specific
paths, credentials, or unpublished artifacts.
-->

This runbook installs, verifies, upgrades, and removes the DCC-MCP adapter for
`{DCC display name}`. The commands implement
[DCC-MCP Adapter Install SOP v1](https://dcc-mcp.github.io/dcc-mcp-core/guide/adapter-install-sop).

## Requirements

- **DCC:** `{DCC display name and required edition/components}`
- **Python:** `{minimum and maximum supported target interpreter versions}`
- **dcc-mcp-core:** `{minimum supported Core version/range}`
- **Platforms:** `{Windows/macOS/Linux support and known exclusions}`
- **Permissions:** `{user/project directories the installer must write}`

Install the Python package into the environment that owns the adapter
sidecar/runtime:

```bash
{target-python} -m pip install "dcc-mcp-{dcc}{version-constraint}"
```

Do not use an arbitrary `python` on `PATH` for an embedded-interpreter host.
Use `--python` when automatic host-specific interpreter detection is not
possible.

## Supported versions

| Adapter | dcc-mcp-core | `{DCC}` | Python | Platforms |
|---|---|---|---|---|
| `{adapter version/range}` | `{Core range}` | `{DCC range}` | `{Python range}` | `{platforms}` |

Unsupported versions fail preflight before modifying the host. `--dcc-path`
selects the exact host version and its matching profile/project layout.

## Agent quick path

Inspect the universal Core plan first:

```bash
dcc-mcp-cli install --dcc-type {dcc}
dcc-mcp-cli install --dcc-type {dcc} --execute --json
```

When this adapter owns host-specific lifecycle execution, use its standard
entry point:

```bash
dcc-mcp-{dcc} install \
  --dcc-path "{absolute-host-path}" \
  --python "{absolute-target-python}" \
  --json \
  --dry-run

dcc-mcp-{dcc} install \
  --dcc-path "{absolute-host-path}" \
  --python "{absolute-target-python}" \
  --json \
  --yes
```

The default and `--dry-run` are non-mutating plans. Before executing, inspect:

- `dcc_type`, host/profile selection, and target interpreter;
- `adapter_version` and `core_version`;
- current installation state and ordered `steps`;
- `receipt_path`;
- `verify`; and
- every machine-executable `next_steps` entry.

Stable exit codes are:

| Exit | Meaning |
|---:|---|
| `0` | completed/planned successfully |
| `10` | preflight failure |
| `20` | acquisition or integrity failure |
| `30` | install/uninstall/rollback failure |
| `40` | verify-to-usable failure |
| `50` | real loaded/locked artifact requires restart |

## Manual path

Use this path when reviewing each lifecycle phase manually. Do not hand-copy
over an existing installation.

1. Resolve the exact `{DCC}` executable and target interpreter.
2. Run the JSON dry-run command from **Agent quick path**.
3. Confirm the plan names only adapter-owned destination/configuration paths.
4. Execute with `--yes`; the installer stages the full payload and writes a
   receipt.
5. Complete any returned GUI-only `file_edit`/host step exactly as reported.
6. `{Open or restart behavior required by this host, or state that none is
   required.}`
7. Run the `verify` command below.

Host enablement performed by this adapter:

- `{module/add-on/package/plugin/startup-hook registration}`
- `{profile/project configuration file touched}`
- `{sidecar/bootstrap behavior and ownership}`

The installer never terminates the user's DCC process. When a loaded native
artifact is locked, it returns exit `50` with the exact deferred cleanup step.

## Verify

```bash
dcc-mcp-{dcc} verify \
  --dcc-path "{absolute-host-path}" \
  --python "{absolute-target-python}" \
  --json
```

A usable result has:

```json
{
  "schema_version": 1,
  "status": "ok",
  "dcc_type": "{dcc}",
  "adapter_version": "{installed-adapter-version}",
  "core_version": "{installed-core-version}",
  "steps": [
    {"id": "receipt", "status": "ok"},
    {"id": "package", "status": "ok"},
    {"id": "import", "status": "ok"},
    {"id": "host-enablement", "status": "ok"},
    {"id": "readiness", "status": "ok"}
  ],
  "next_steps": [],
  "receipt_path": "{absolute-receipt-path}",
  "verify": {
    "directly_usable": true,
    "failure_stage": null,
    "failure_reason": null
  }
}
```

Verification checks the receipt and artifact digests, the installed package,
an import in the target interpreter, host enablement/bootstrap state, and the
typed readiness probe `{dcc}_diagnostics__ping` when the host can run.

For a non-mutating state summary:

```bash
dcc-mcp-{dcc} status \
  --dcc-path "{absolute-host-path}" \
  --python "{absolute-target-python}" \
  --json
```

## Upgrade

Review the upgrade plan before execution:

```bash
{target-python} -m pip install --upgrade "dcc-mcp-{dcc}{version-constraint}"

dcc-mcp-{dcc} upgrade \
  --dcc-path "{absolute-host-path}" \
  --python "{absolute-target-python}" \
  --json \
  --dry-run

dcc-mcp-{dcc} upgrade \
  --dcc-path "{absolute-host-path}" \
  --python "{absolute-target-python}" \
  --json \
  --yes
```

The upgrade stages the new version before moving the prior receipted state. A
commit or verification failure restores the prior version. If the host locks a
loaded native file, close/restart the host only when the exit `50` result asks
for it, then repeat the same command.

## Uninstall

Review the receipt-driven removal plan, then execute it:

```bash
dcc-mcp-{dcc} uninstall \
  --dcc-path "{absolute-host-path}" \
  --python "{absolute-target-python}" \
  --json \
  --dry-run

dcc-mcp-{dcc} uninstall \
  --dcc-path "{absolute-host-path}" \
  --python "{absolute-target-python}" \
  --json \
  --yes
```

Uninstall removes only files and configuration recorded in the receipt. It
preserves scenes/projects and user-owned host configuration. Running uninstall
again after successful removal is safe and reports the already-absent state.

To remove the Python distribution after host cleanup succeeds:

```bash
{target-python} -m pip uninstall dcc-mcp-{dcc}
```

## Troubleshooting

| Result | Diagnosis | Action |
|---|---|---|
| Exit `10`, host | `{host/profile resolution failure}` | Pass the exact `--dcc-path`; confirm the version table. |
| Exit `10`, Python | `{wrong/missing interpreter}` | Pass the interpreter owning the adapter runtime with `--python`. |
| Exit `10`, partial | `{unreceipted or stale state}` | Inspect the exact reported paths; do not delete user files. |
| Exit `20` | `{artifact/integrity failure}` | Re-acquire only from the pinned official source and verify the digest. |
| Exit `30` | `{transaction/rollback failure}` | Preserve the report and receipt; follow its exact recovery command. |
| Exit `40`, import | `{package/version mismatch}` | Install the reported adapter/Core versions into the target interpreter. |
| Exit `40`, bootstrap | `{startup hook failure}` | Inspect `{adapter-specific bootstrap log/host console location}`. |
| Exit `40`, readiness | `{host/bridge not callable}` | Start `{DCC}`, wait for its plugin, then rerun `verify`. |
| Exit `50` | `{loaded/locked artifact}` | Save work, close/restart only the reported host, and repeat the command. |

Bootstrap diagnostics are stored at `{bounded adapter-specific path/location}`
and include timestamp, stage, error class, and message. Logging failures remain
visible alongside the original startup error.

For shared runtime diagnosis, preserve the failed result and run:

```bash
dcc-mcp-cli doctor
dcc-mcp-cli list
```

Catalog `instructions_url` for this runbook:

```text
https://raw.githubusercontent.com/dcc-mcp/dcc-mcp-{dcc}/main/install.md
```
