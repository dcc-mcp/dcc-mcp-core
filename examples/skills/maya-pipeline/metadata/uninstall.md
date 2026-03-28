# Maya Pipeline — Uninstallation

## Cleanup Steps

1. Remove the skill directory from `DCC_MCP_SKILL_PATHS`
2. Clear the SkillScanner cache:
   ```python
   scanner = dcc_mcp_core.SkillScanner()
   scanner.clear_cache()
   ```
3. Optionally remove dependent skills if no longer needed:
   ```bash
   rm -rf /path/to/skills/maya-geometry
   rm -rf /path/to/skills/usd-tools
   ```

## Notes

- Removing this skill does NOT automatically remove its dependencies
- Other skills may still depend on `maya-geometry` or `usd-tools`
