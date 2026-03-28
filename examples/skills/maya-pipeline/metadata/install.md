# Maya Pipeline — Installation

## Prerequisites

- Python 3.8+
- Autodesk Maya 2022+ (optional — scripts work standalone for testing)
- OpenUSD (`pip install usd-core`) for USD validation

## Install Steps

1. Ensure dependent skills are available:
   ```bash
   # These will be auto-resolved if using SkillScanner
   # Manual install:
   clawhub install maya-geometry
   clawhub install usd-tools
   ```

2. Set environment variable for skill discovery:
   ```bash
   export DCC_MCP_SKILL_PATHS="/path/to/skills"
   ```

3. Verify installation:
   ```bash
   python -c "
   import dcc_mcp_core
   scanner = dcc_mcp_core.SkillScanner()
   dirs = scanner.scan(dcc_name='maya')
   print(f'Found {len(dirs)} skills')
   "
   ```

## Post-Install Validation

```python
import dcc_mcp_core

meta = dcc_mcp_core._core.parse_skill_md('/path/to/maya-pipeline')
assert meta is not None
assert meta.name == 'maya-pipeline'
assert len(meta.scripts) >= 2
```
