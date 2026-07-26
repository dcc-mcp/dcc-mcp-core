# Verify — Common Errors

Diagnostic patterns and fixes for verification failures.

## Instance not ready

**Symptom**: `dcc-mcp-cli list` shows `direct_control.ready=false`

| dispatch_status | Likely cause | Fix |
|-----------------|-------------|-----|
| `Pending` | DCC still loading skills | `dcc-mcp-cli wait-ready --timeout 30` |
| `Failed` | Crash or init error | Check DCC logs; restart adapter |
| `Unknown` | Sidecar not reporting | Verify sidecar process; check `host_rpc_*` fields |

```bash
# Check failure diagnostics
dcc-mcp-cli list --output json | python -c "
import sys, json
data = json.load(sys.stdin)
for i in data.get('instances', []):
    dc = i.get('direct_control', {})
    if not dc.get('ready'):
        print(f'{i[\"instance_short\"]}:')
        diag = dc.get('diagnostics', {})
        print(f'  stage={diag.get(\"failure_stage\")}')
        print(f'  reason={diag.get(\"failure_reason\")}')
        print(f'  action={dc.get(\"recommended_next_action\")}')
"
```

## Gateway unreachable

**Symptom**: `dcc-mcp-cli health` fails or times out

1. Verify gateway daemon is running
2. Check `DCC_MCP_BASE_URL` is correct
3. For remote gateways, verify network connectivity
4. Run `dcc-mcp-cli doctor` for full diagnostics

```bash
# Check if gateway process is running
dcc-mcp-cli doctor | grep -A5 "daemon"

# Restart gateway (auto-starts on next CLI command)
dcc-mcp-cli health  # triggers auto-start if missing
```

## Zero instances

**Symptom**: `dcc-mcp-cli list` returns `total: 0`

1. No DCC adapter is running
2. Adapter started but hasn't registered yet
3. Wrong profile selected

```bash
# Check which profile is active
dcc-mcp-cli doctor | grep "selected"

# Switch to local profile
dcc-mcp-cli gateway set local

# Bootstrap guidance for zero instances
# See dcc-mcp/references/ZERO_INSTANCES_CLI.md
```

## Capability not found

**Symptom**: `search` returns no results for expected capability

1. Wrong dcc_type filter
2. Skill not loaded on this instance
3. Different tool name than expected

```bash
# Broaden search
dcc-mcp-cli search --query "<shorter query>" --limit 20

# Try without dcc_type filter
dcc-mcp-cli search --query "<query>" --limit 20

# Check what skills are loaded
dcc-mcp-cli describe <instance>  # lists available skill catalog
```

## Python 3.7 lite mode

**Symptom**: `host_rpc is required` or `py37-lite` in diagnostics

On Maya 2022 / Blender 2.83 with Python 3.7 and core >= 0.19.5:
- The core may enter `py37-lite` fallback mode
- Gateway discovery works but dispatch requires native wheel
- Pin core to compatible version or use native py37 build

```bash
# Check if py37-lite is active
dcc-mcp-cli list --output json | python -c "
import sys, json
data = json.load(sys.stdin)
for i in data.get('instances', []):
    diag = i.get('direct_control', {}).get('diagnostics', {})
    if 'py37-lite' in str(diag):
        print(f'{i[\"instance_short\"]}: py37-lite active — dispatch unavailable')
"
```

## Instance-leased

**Symptom**: `call` returns `instance-leased` error

Another workflow holds the lease. Either:
1. Wait for the lease to expire
2. Use the same `lease_owner` in your `--meta-json`
3. Pick a different instance

```bash
# Check lease status
dcc-mcp-cli list --output json | python -c "
import sys, json
data = json.load(sys.stdin)
for i in data.get('instances', []):
    if i.get('lease'):
        print(f'{i[\"instance_short\"]}: leased by {i[\"lease\"][\"owner\"]} '
              f'until {i[\"lease\"].get(\"expires\")}')
"
```
