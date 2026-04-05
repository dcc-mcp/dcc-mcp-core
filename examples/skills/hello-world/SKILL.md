---
name: hello-world
description: "A minimal example skill that prints a greeting message"
tools: ["Bash", "Read"]
tags: ["example", "beginner"]
dcc: python
version: "1.0.0"
---

# Hello World Skill

The simplest possible skill — demonstrates the minimum required structure for a dcc-mcp-core skill.

## Purpose

Use this skill to:
1. **Verify** that the skill scanning and loading pipeline works correctly
2. **Learn** the minimum structure required for any skill (SKILL.md + scripts/ directory)
3. **Test** your `DCC_MCP_SKILL_PATHS` configuration

## Scripts

- **greet.py** — Prints a greeting message; demonstrates the simplest possible action body

## Action Name (auto-derived)

- `hello_world__greet` — from `scripts/greet.py`

## Quick Test

```python
import os
from dcc_mcp_core import scan_and_load, parse_skill_md

# Test direct parse
meta = parse_skill_md("examples/skills/hello-world")
assert meta.name == "hello-world"
assert meta.dcc == "python"
assert len(meta.scripts) == 1

# Or load via env var
os.environ["DCC_MCP_SKILL_PATHS"] = "examples/skills"
skills, skipped = scan_and_load()
hello = next(s for s in skills if s.name == "hello-world")
print(f"Loaded: {hello.name} with {len(hello.scripts)} scripts")
```

## Minimum Skill Structure

```
hello-world/
├── SKILL.md          ← Required: YAML frontmatter (name, dcc required)
└── scripts/
    └── greet.py      ← Required: at least one script
```
