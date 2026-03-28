---
name: clawhub-compat
description: "Demonstrates full compatibility with the ClawHub/OpenClaw skill format"
version: "1.0.0"
metadata:
  openclaw:
    requires:
      env:
        - EXAMPLE_API_KEY
      bins:
        - curl
    primaryEnv: EXAMPLE_API_KEY
    emoji: "\U0001F980"
    homepage: https://github.com/loonghao/dcc-mcp-core
    install:
      - kind: node
        package: prettier
        bins: [prettier]
---

# ClawHub Compatible Skill

This skill demonstrates that `dcc-mcp-core` skills are **fully compatible**
with the [ClawHub](https://clawhub.ai/) / [OpenClaw](https://openclaw.ai/)
ecosystem format.

## What This Proves

1. **Same SKILL.md format** — Our YAML frontmatter parser handles all ClawHub
   fields including `metadata.openclaw.requires`, `install`, `primaryEnv`, etc.

2. **Bidirectional reuse** — Skills created for ClawHub can be used directly
   with `dcc-mcp-core`, and vice versa.

3. **Extended fields** — We support additional fields like `dcc` and `tools`
   that are specific to the DCC ecosystem while remaining backward-compatible.

## ClawHub ↔ dcc-mcp-core Field Mapping

| ClawHub Field | dcc-mcp-core Field | Notes |
|---|---|---|
| `name` | `name` | Identical |
| `description` | `description` | Identical |
| `version` | `version` | Identical |
| `metadata.openclaw.requires.bins` | (parsed as metadata) | Available via SkillMetadata |
| `metadata.openclaw.requires.env` | (parsed as metadata) | Available via SkillMetadata |
| — | `dcc` | DCC-specific extension |
| — | `tools` | OpenClaw tools annotation |
| — | `tags` | Tag-based discovery |

## Publishing to ClawHub

```bash
# This skill can be published as-is
clawhub publish ./clawhub-compat --slug clawhub-compat --version 1.0.0
```

## Using ClawHub Skills with dcc-mcp-core

```python
import dcc_mcp_core

# Install a skill from ClawHub
# $ clawhub install some-skill

# Point the scanner at your ClawHub skills directory
scanner = dcc_mcp_core.SkillScanner()
skills = scanner.scan(extra_paths=["~/.openclaw/skills"])
# All ClawHub-installed skills are now discoverable!
```
