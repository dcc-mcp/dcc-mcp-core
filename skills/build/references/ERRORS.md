# Build — Common Errors

Diagnostic patterns for skill creation, validation, and publishing failures.

## Validation fails: "name does not match directory"

**Cause**: SKILL.md `name` field doesn't match the directory name.

Fix: Ensure the directory name and `name:` frontmatter value are identical
(both kebab-case).

## Validation fails: "top-level version key is rejected"

**Cause**: `version` is at the top level of SKILL.md frontmatter instead of
under `metadata.dcc-mcp.version`.

```yaml
# WRONG
version: "1.0.0"

# RIGHT
metadata:
  dcc-mcp:
    version: "1.0.0"
```

## Validation fails: "source_file not found"

**Cause**: `tools.yaml` references a script that doesn't exist in `scripts/`.

Fix: Create the script file or update the `source_file` path.

## Scaffold creates files but tool doesn't appear

1. Check the adapter has the skill in its search path
2. Run `dcc-mcp-cli reload-skills --dcc-type <dcc>`
3. Verify with `dcc-mcp-cli search --query "<skill capability>"`

## Package fails: "validation failed"

The package step runs validation first. Fix validation errors before packaging.

```bash
# Diagnose
dcc-mcp-cli call <gateway>.build__validate \
  --json '{"skill_path": "./skills/my-skill"}'

# Fix issues, then retry
dcc-mcp-cli call <gateway>.build__package \
  --json '{"skill_path": "./skills/my-skill"}'
```

## Publish fails: "already exists"

A package with this version is already published.

Options:
- Bump version: `"version": "patch"` or `"version": "minor"`
- Check what's published: `dcc-mcp-cli marketplace search --query "<skill-name>"`

## Script-helper-adoption warnings

**Warning**: `scripts/tool.py imports 'requests' — consider dcc_mcp_core.skills_helper`

Replace heavy dependencies with the bundled helper:

```python
# BEFORE
import requests
resp = requests.get(url, timeout=10)

# AFTER
from dcc_mcp_core.skills_helper import http_get
resp_data = http_get(url, timeout=10)
```

```python
# BEFORE
import yaml
data = yaml.safe_load(path)

# AFTER
from dcc_mcp_core.skills_helper import read_yaml
data = read_yaml(path)
```

## Skills not appearing after reload

1. Check `DCC_MCP_SKILL_PATHS` includes the skill's parent directory
2. Check `DCC_MCP_DISABLE_DEFAULT_SKILL_PATHS` is not set
3. Verify the skill directory has a valid `SKILL.md`

```bash
# Debug skill discovery
export DCC_MCP_LOG_LEVEL=DEBUG
dcc-mcp-cli reload-skills --dcc-type python
# Look for "Discovering skills in" log lines
```

## Python 3.7: skill scripts use f-strings or walrus operator

**Cause**: Python 3.7 doesn't support all modern syntax.

```python
# AVOID (Python 3.8+)
x := get_value()

# USE
x = get_value()

# AVOID (Python 3.12+)
f"value is {x=}"

# USE
f"value is x={x}"
```

All build skill scripts maintain Python 3.7 compatibility per ADR 011.
