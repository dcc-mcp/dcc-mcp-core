# Zero instances — CLI setup guide

Use this document only when:

- `dcc-mcp-cli health` (or `python scripts/dcc_gateway.py health`) succeeds,
- `dcc-mcp-cli list` returns `"total": 0` for local inventory, or
  `dcc-mcp-cli list --gateway <name>` returns `"total": 0` for a remote profile, and
- the user has explicitly approved setup guidance.

Until all three are true, do not run install commands, edit environment files,
launch GUI applications, or modify MCP host configuration.

---

## User consent

Before any setup step, confirm:

1. Which DCC product the user needs.
2. Whether they want commands suggested or executed.
3. That they will confirm after each DCC-side step so you can re-run
   `dcc-mcp-cli list`.

---

## Diagnose

| Check | Meaning | Next step |
|-------|---------|-----------|
| `dcc-mcp-cli doctor` reports local profile, registry path, zero local inventory, and server binary diagnostics | Confirms which local state the CLI is reading before adapter setup | Use the reported registry path when checking sidecar/server logs |
| `dcc-mcp-cli list` returns `total == 0` in local mode | The loopback gateway was ensured, but no local DCC sidecar/server is registered in the FileRegistry | Start a DCC adapter |
| `dcc-mcp-cli list --gateway <name>` fails | Remote gateway profile is unreachable; remote gateways cannot be auto-started | Inspect the selected profile and remote gateway URL before adapter setup |
| `dcc-mcp-cli health` fails | CLI auto-ensure could not start or reach the local loopback gateway | Inspect structured CLI output before endpoint/admin/update workflows |

Local `dcc-mcp-cli list` first ensures the machine-wide loopback gateway, then
reads the FileRegistry directly. In the built-in `local` profile, `search`,
`describe`, `load-skill`, `call`, `wait-ready`, and guarded `stop-instance` use
the registered DCC instance's own MCP/readyz/safe-stop endpoints after the same
gateway lifecycle check. Endpoint/admin/update workflows also auto-ensure a
machine-wide gateway daemon when they target loopback HTTP.
Per-DCC adapters register themselves through their own sidecar/server runtime.
The legacy first-wins election is only for explicit
`dcc-mcp-server auto --legacy-gateway-election` setups.

---

## Adapter discovery

With user approval, build an adapter package plan via the CLI:

```bash
dcc-mcp-cli install --dcc-type maya
dcc-mcp-cli install --dcc-type blender
```

The `install` command returns an auditable plan. Treat it as guidance unless the
user explicitly asks you to execute installation steps. If the adapter's
`install.md` asks for a host Python interpreter, pass it with `--python` before
execution:

```bash
dcc-mcp-cli install --dcc-type <dcc> --python "<dcc-python>" --execute
```

Execution installs/verifies packages only. The online registration signal is
still `dcc-mcp-cli list`: the DCC plugin or sidecar must start, stay alive, and
self-register in the FileRegistry or selected gateway.

If the returned plan has `install_policy.auto_install_enabled=false`, automatic
install execution is disabled for this environment. Do not call `--execute`;
show `install_policy.prompt` to the user and hand off to the studio Pipeline TD
or deployment workflow named in that prompt.

The plan JSON includes a `next_steps` array. If it includes
`read-install-instructions`, read the referenced raw `install.md` from the
adapter repository first; that runbook owns host-specific setup. Then follow the
remaining steps after installation: start/enable the DCC plugin, run
`dcc-mcp-cli doctor`, confirm `dcc-mcp-cli list`, wait with
`dcc-mcp-cli wait-ready --dcc-type <dcc>`, search tools, then install optional
marketplace skills by running marketplace search, inspecting the selected
package, installing it, and finally running
`dcc-mcp-cli reload-skills --dcc-type <dcc>`.

Alternatively, when the CLI binary is not yet available:

```bash
python scripts/dcc_gateway.py install --dcc-type maya
python scripts/dcc_gateway.py install --dcc-type blender
```

The Python fallback auto-downloads the CLI if needed (with user consent, pass
`--ensure-cli`), then delegates to `dcc-mcp-cli install`.

---

## Generic Adapter Checklist

Build the plan first:

```bash
dcc-mcp-cli install --dcc-type maya
dcc-mcp-cli install --dcc-type blender
dcc-mcp-cli install --dcc-type houdini
dcc-mcp-cli install --dcc-type photoshop
dcc-mcp-cli install --dcc-type 3dsmax
```

Then:

1. Read the `read-install-instructions.url` from the plan when present.
2. Follow that adapter-maintained `install.md` for host-specific plugin
   enablement, setup scripts, and smoke prompts.
3. Run `--execute` only after user consent, and only with the interpreter/path
   arguments requested by that adapter runbook.
4. Start or reload the DCC plugin so its sidecar self-registers.
5. Re-run:

```bash
dcc-mcp-cli list
```

When `total >= 1`, continue with the plan's CLI next steps:
`doctor -> wait-ready -> search -> describe -> call`. If a listed row has
`direct_control.ready=false`, inspect `direct_control.diagnostics` first; it
carries sidecar `failure_stage`, `failure_reason`, host RPC metadata, gateway
recovery fields, and any supervisor-recorded stdout/stderr log paths. If
marketplace skills are installed, finish with `reload-skills`.

If Python is not available as `python` / `py`, install vx first:

```bash
# Linux / macOS
curl -fsSL https://raw.githubusercontent.com/loonghao/vx/main/install.sh | bash

# Windows PowerShell
powershell -c "irm https://raw.githubusercontent.com/loonghao/vx/main/install.ps1 | iex"
```
