# Debug — Common Errors

Diagnostic patterns for recurring DCC tool failures.

## "mcp tool execution failed"

The most common vague error. Always start with `dcc_diagnostics__error_report`.

| Likely cause | Signal | Fix |
|-------------|--------|-----|
| Host API exception | Log contains traceback | Fix the Python code in the tool script |
| Sandbox denial | audit_log shows `denied` | Adjust sandbox policy or use an allowed action |
| Import error | Log shows `ModuleNotFoundError` | Install missing dependency |
| Timeout | Job status shows `interrupted` | Increase timeout or optimize the tool |
| Host not running | process_status shows dead | Restart the DCC adapter |

## "ModuleNotFoundError: No module named 'X'"

**Pattern**: Tool script imports a package not in the DCC's Python environment.

```bash
# Check what's installed
dcc-mcp-cli call <instance>__dcc_introspect__eval \
  --json '{"code": "import importlib; print(importlib.util.find_spec(\"X\"))"}'

# Install via the DCC's Python
# Maya: mayapy -m pip install X
# Blender: blender --python-expr "import subprocess; subprocess.check_call(['pip', 'install', 'X'])"
```

## Tool works locally but fails remotely

**Pattern**: Environment difference between local and remote instances.

1. Check Python versions match
2. Check dcc-mcp-core versions match
3. Check skill versions match (`dcc-mcp-cli marketplace list`)
4. Check environment variables

## "instance-leased" or "lease-owner-mismatch"

Another workflow holds the instance lease.

```bash
# Find the lease owner
dcc-mcp-cli list --output json | python -c "
import sys, json
data = json.load(sys.stdin)
for i in data.get('instances', []):
    lease = i.get('lease')
    if lease:
        print(f'{i[\"instance_short\"]} leased by {lease[\"owner\"]}')
"

# Option A: Use same lease_owner
dcc-mcp-cli call <tool> --json '{...}' --meta-json '{"lease_owner": "<existing_owner>"}'

# Option B: Wait for expiry
# Option C: Use different instance
```

## Screenshot returns blank/black image

| DCC | Likely cause | Fix |
|-----|-------------|-----|
| Any | Window minimized | Restore window before capture |
| Maya | VP2 viewport not refreshed | Force viewport refresh |
| Blender | GPU render in progress | Wait for render completion |

## Audit log shows no entries

**Cause**: Sandbox auditing is not enabled.

```python
# Ensure SandboxContext is active in adapter init
from dcc_mcp_core import SandboxContext
SandboxContext.enable_audit_logging()
```

## Tool metrics show high P95 latency

**Pattern**: Tool is consistently slow at P95 but median is fine.

Likely main-thread contention. Check:
1. Is the tool running on `affinity: main` when `any` would work?
2. Are there other main-thread tools queued?
3. Is the DCC doing heavy work (rendering, simulation)?

## Gateway stats show `configured_route_recorded=false`

**Cause**: Call was routed directly (local_mcp_direct), not through gateway.

Add `--require-gateway --agent-session-id <id>` to the CLI call to force gateway routing and enable stats recording.
