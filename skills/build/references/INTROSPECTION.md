# Build — Introspection

How to explore skill structure, discover installed packages, and understand
the build toolchain.

## Skill directory anatomy

```
my-skill/
├── SKILL.md              # Required: YAML frontmatter + markdown instructions
├── tools.yaml            # Required when tools are declared
├── scripts/              # Tool implementations (source_file references)
│   └── <tool_name>.py
├── references/           # Optional: long-form docs loaded on demand
│   ├── RECIPES.md        # Copy-pasteable usage snippets
│   ├── INTROSPECTION.md  # How to explore/discover
│   └── ERRORS.md         # Common errors and fixes
└── agents/               # Optional: agent-specific configs
    └── openai.yaml
```

## SKILL.md frontmatter reference

```yaml
---
name: my-skill                    # Required: kebab-case, ≤64 chars, matches dir
description: >-                   # Required: ≤1024 chars
  What the skill does, when to use it, when NOT to use it.
license: MIT                      # Optional
allowed-tools: ["Bash", "Read"]   # Optional: agent tool allowlist
metadata:
  dcc-mcp:
    dcc: python                   # Target DCC or 'python' for cross-DCC
    layer: domain                 # domain | infrastructure | thin-harness | example
    version: "1.0.0"              # Skill version (under metadata.dcc-mcp!)
    compatibility: "Python 3.7+, dcc-mcp-core 0.19+"
    tags: [tag1, tag2]            # Gateway search filter tags
    search-hint: "..."            # Agent-facing search description
    tools: tools.yaml             # Points to tool definitions
    depends: [other-skill]        # Machine-readable skill dependencies
---
```

## Validation checks

The validator (`validate_skill_dir` / `dcc_mcp_core.validate_skill()`) checks:

| Check | What it validates |
|-------|-------------------|
| SKILL.md exists | File is present and readable |
| YAML frontmatter | Well-formed, between `---` markers |
| Required fields | `name`, `description` present |
| Name format | kebab-case, ≤64 chars, matches directory |
| Field lengths | description ≤1024, compatibility ≤500 |
| Tool declarations | Non-empty, no duplicates, snake_case |
| Script files | Referenced `source_file` exists in scripts/ |
| Sidecar files | tools.yaml, groups.yaml, prompts.yaml referenced |
| Dependencies | `depends` list consistency |
| Version metadata | Under `metadata.dcc-mcp.version`, not top-level |
| Skill-helper adoption | Suggested helper usage over raw deps |

## Querying installed skills

```bash
# Via marketplace
dcc-mcp-cli marketplace list --installed

# Via CLI list (shows loaded skills per instance)
dcc-mcp-cli list

# Via Python
python -c "
from dcc_mcp_core import discover_skills
skills = discover_skills()
for s in skills:
    print(f'{s.name} v{s.version} [{s.layer}] → {s.dcc}')
"
```

## Skill layer taxonomy

| Layer | Purpose | When to use |
|-------|---------|-------------|
| `infrastructure` | Cross-DCC primitives | Reusable tools shared across hosts |
| `domain` | Host/workflow-specific | Operations for a specific DCC or pipeline stage |
| `thin-harness` | Raw scripting fallback | Minimal wrapper + recipes for well-trained APIs |
| `example` | Authoring reference | Templates and examples, not for production loading |
