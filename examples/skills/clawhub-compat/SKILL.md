---
name: clawhub-compat
description: "Demonstrates full compatibility with the ClawHub/OpenClaw skill format. Use as a reference when creating skills for both the dcc-mcp-core ecosystem and ClawHub marketplace."
license: MIT
compatibility: Requires curl binary on PATH
allowed-tools: Bash Read
metadata:
  category: example
  openclaw:
    requires:
      env:
        - EXAMPLE_API_KEY
      bins:
        - curl
    primaryEnv: EXAMPLE_API_KEY
    emoji: "🦀"
    homepage: https://github.com/loonghao/dcc-mcp-core
    install:
      - kind: node
        package: prettier
        bins: [prettier]
tags: [example, clawhub, openclaw, compatibility]
dcc: python
version: "1.0.0"
search-hint: "clawhub, openclaw, compatibility, marketplace, example format"
---

# ClawHub Compatible Skill

This skill demonstrates that `dcc-mcp-core` skills are **fully compatible**
with the [ClawHub](https://clawhub.ai/) / [OpenClaw](https://openclaw.ai/)
ecosystem format.

## Three-Standard Compatibility

| Field | agentskills.io | ClawHub | dcc-mcp-core | This Skill |
|-------|---------------|---------|--------------|------------|
| `name` | ✅ Required | ✅ Required | ✅ Required | ✅ |
| `description` | ✅ Required | ✅ Required | ✅ Required | ✅ |
| `license` | ✅ Optional | MIT-0 only | ✅ Optional | ✅ MIT |
| `compatibility` | ✅ Optional | — | ✅ Optional | ✅ |
| `allowed-tools` | ✅ Optional | — | ✅ Optional | ✅ |
| `metadata` | KV strings | `openclaw.*` | `serde_json::Value` | ✅ |
| `metadata.openclaw.requires` | — | ✅ | ✅ Parsed | ✅ |
| `metadata.openclaw.primaryEnv` | — | ✅ | ✅ Parsed | ✅ |
| `version` | — | ✅ Required | ✅ Optional | ✅ |
| `dcc` | — | — | ✅ Extension | ✅ |
| `tags` | — | — | ✅ Extension | ✅ |

## Publishing to ClawHub

```bash
# This skill can be published as-is to ClawHub
clawhub publish ./clawhub-compat --slug clawhub-compat --version 1.0.0
```

## Using ClawHub Skills with dcc-mcp-core

```python
import dcc_mcp_core

# Skills installed via ClawHub are discoverable automatically
catalog = dcc_mcp_core.SkillCatalog(dcc_mcp_core.ToolRegistry())
catalog.discover(extra_paths=["~/.openclaw/skills"])

# Access ClawHub-specific metadata
for skill in catalog.list_skills():
    info = catalog.get_skill_info(skill["name"])
    # info["required_bins"], info["emoji"], info["homepage"]
```
