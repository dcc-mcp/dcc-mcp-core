# Build Recipes

Copy-pasteable sequences for creating and publishing DCC-MCP skills.

## Scaffold a new skill

```bash
# Using the build skill tool
dcc-mcp-cli call <gateway>.build__scaffold \
  --json '{"name": "maya-rigging", "dcc": "maya", "tool_name": "create_rig", "affinity": "main"}'

# Or using the skills-creator directly
python -c "
from dcc_mcp_core.skills_helper import scaffold_skill
scaffold_skill('maya-rigging', parent_dir='./skills', dcc='maya', tool_name='create_rig')
"
```

## Validate a skill

```bash
# CLI
dcc-mcp-cli call <gateway>.build__validate \
  --json '{"skill_path": "./skills/maya-rigging"}'

# Python
python -c "
from dcc_mcp_core import validate_skill
report = validate_skill('./skills/maya-rigging')
for issue in report.issues:
    print(f'[{issue.severity}] {issue.category}: {issue.message}')
print('Valid!' if not report.has_errors else 'Fix errors above.')
"

# Or via the skills-creator tool
dcc-mcp-cli call <instance>__dcc_mcp_skills_creator__validate_skill_dir \
  --json '{"skill_dir": "/path/to/skill"}'
```

## Build a package

```bash
# Using build skill
dcc-mcp-cli call <gateway>.build__package \
  --json '{"skill_path": "./skills/maya-rigging", "output_dir": "./dist"}'

# Manual packaging with marketplace tools
dcc-mcp-cli marketplace pack --skill-dir ./skills/maya-rigging --output ./dist
```

## Publish to marketplace

```bash
# Dry run first
dcc-mcp-cli call <gateway>.build__publish \
  --json '{"skill_path": "./skills/maya-rigging", "dry_run": true}'

# Publish with version bump
dcc-mcp-cli call <gateway>.build__publish \
  --json '{"skill_path": "./skills/maya-rigging", "version": "patch"}'

# Manual publish
dcc-mcp-cli marketplace publish --package ./dist/maya-rigging-1.0.0.zip
```

## List installed skills

```bash
# Using build skill
dcc-mcp-cli call <gateway>.build__list_installed --json '{}'

# Via marketplace
dcc-mcp-cli marketplace list --installed

# Check a specific skill
dcc-mcp-cli marketplace inspect maya-rigging
```

## Full create-to-publish pipeline

```bash
# 1. Scaffold
dcc-mcp-cli call <gateway>.build__scaffold \
  --json '{"name": "my-skill", "dcc": "python", "layer": "domain"}'

# 2. Edit SKILL.md, implement script, add references

# 3. Validate
dcc-mcp-cli call <gateway>.build__validate \
  --json '{"skill_path": "./skills/my-skill", "strict": true}'

# 4. Test locally
dcc-mcp-cli reload-skills --dcc-type python
dcc-mcp-cli search --query "my skill capability"

# 5. Publish
dcc-mcp-cli call <gateway>.build__publish \
  --json '{"skill_path": "./skills/my-skill", "version": "minor"}'
```
