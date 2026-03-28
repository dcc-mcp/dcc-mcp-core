# Skills API

`dcc_mcp_core.skills`

## SkillScanner

```python
scanner = SkillScanner()
skill_dirs = scanner.scan(extra_paths=["/my/skills"], dcc_name="maya")
```

## Functions

### parse_skill_md

```python
parse_skill_md(skill_dir: str) -> Optional[SkillMetadata]
```

Parse a SKILL.md file and return metadata.

### load_skill

```python
load_skill(
    skill_dir: str,
    registry: ActionRegistry = None,
    dcc_name: str = None
) -> List[Type[Action]]
```

Load a skill directory and register all script actions.

### create_script_action

```python
create_script_action(
    skill_name: str,
    script_path: str,
    metadata: SkillMetadata,
    dcc_name: str
) -> Type[Action]
```

Create an Action subclass from a script file.

### scan_skill_paths

```python
scan_skill_paths(extra_paths: List[str] = None) -> List[str]
```

Convenience function to scan all skill paths.

## Environment Variables

| Variable | Description |
|----------|-------------|
| `DCC_MCP_SKILL_PATHS` | Skill search paths (platform path separator) |
