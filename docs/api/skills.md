# Skills API

`dcc_mcp_core.SkillScanner`, `dcc_mcp_core.parse_skill_md`, `dcc_mcp_core.scan_skill_paths`

## SkillScanner

Scanner for discovering Skill packages in directories. Caches file modification times for efficient repeated scans.

```python
from dcc_mcp_core import SkillScanner

scanner = SkillScanner()
```

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `scan(extra_paths=None, dcc_name=None, force_refresh=False)` | `List[str]` | Scan paths for skill directories |
| `clear_cache()` | — | Clear the mtime cache and discovered list |

### Properties

| Property | Type | Description |
|----------|------|-------------|
| `discovered_skills` | `List[str]` | Previously discovered skill directory paths |

### Dunder Methods

| Method | Description |
|--------|-------------|
| `__repr__` | `SkillScanner(cached=N, discovered=N)` |

## Functions

### parse_skill_md

```python
parse_skill_md(skill_dir: str) -> Optional[SkillMetadata]
```

Parse a SKILL.md file from a skill directory. Returns `None` if the file is missing or invalid.

- Extracts YAML frontmatter between `---` delimiters
- Enumerates scripts in `scripts/` subdirectory
- Discovers `.md` files in `metadata/` subdirectory
- Merges dependencies from `metadata/depends.md`

### scan_skill_paths

```python
scan_skill_paths(extra_paths: Optional[List[str]] = None, dcc_name: Optional[str] = None) -> List[str]
```

Convenience function: creates a fresh `SkillScanner` and scans all paths.

## Search Path Priority

1. `extra_paths` parameter (highest priority)
2. `DCC_MCP_SKILL_PATHS` environment variable
3. Platform-specific skills directory (DCC-specific)
4. Platform-specific skills directory (global)

## Environment Variables

| Variable | Description |
|----------|-------------|
| `DCC_MCP_SKILL_PATHS` | Skill search paths (`;` on Windows, `:` on Unix) |
