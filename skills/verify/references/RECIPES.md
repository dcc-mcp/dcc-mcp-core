# Verify Recipes

Copy-pasteable verification sequences for common DCC preflight scenarios.

## Quick readiness check

```bash
# Check if any Maya instance is ready
dcc-mcp-cli list | python -c "
import sys, json
data = json.load(sys.stdin)
ready = [i for i in data.get('instances', [])
         if i.get('direct_control', {}).get('ready')]
print(f'{len(ready)}/{len(data.get(\"instances\", []))} ready')
for r in ready:
    print(f'  {r[\"instance_short\"]} ({r[\"dcc_type\"]}) — {r[\"direct_control\"][\"dispatch_status\"]}')
"

# Wait for a booting instance
dcc-mcp-cli wait-ready --dcc-type maya --timeout 30
```

## Pre-dispatch preflight

```bash
# 1. Inventory
dcc-mcp-cli list

# 2. Doctor (no-launch diagnostics)
dcc-mcp-cli doctor

# 3. Search for needed capability
dcc-mcp-cli search --query "create sphere" --dcc-type maya --limit 5

# 4. If tool found, describe it
dcc-mcp-cli describe maya.a1b2c3d4.maya_primitives__create_sphere

# 5. All clear → dispatch
```

## Remote gateway verification

```bash
# List remote profiles
dcc-mcp-cli gateway list

# Set remote profile
dcc-mcp-cli gateway set pcA

# Check remote inventory
dcc-mcp-cli list --gateway pcA

# Verify remote health
dcc-mcp-cli health --gateway pcA
```

## Capability search

```bash
# Search by intent words
dcc-mcp-cli search --query "rig character" --dcc-type maya

# If no results, broaden
dcc-mcp-cli search --query "rig" --limit 20

# Still nothing? Check what skills are loaded
dcc-mcp-cli list  # then inspect skill catalog
```

## Environment validation

```bash
# Check Python version on the DCC instance
dcc-mcp-cli call <instance>__dcc_introspect__eval \
  --json '{"code": "import sys; print(sys.version)"}'

# Check dcc-mcp-core version
dcc-mcp-cli call <instance>__dcc_introspect__eval \
  --json '{"code": "import dcc_mcp_core; print(dcc_mcp_core.__version__)"}'

# List installed skills
dcc-mcp-cli call <instance>__dcc_introspect__search \
  --json '{"query": ""}'
```
