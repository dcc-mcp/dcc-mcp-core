---
name: git-automation
description: "Git repository analysis and automation tools"
tools: ["Bash", "Read"]
tags: ["git", "vcs", "automation", "devops"]
dcc: python
version: "1.0.0"
metadata:
  openclaw:
    requires:
      bins:
        - git
---

# Git Automation Skill

Provides Git repository analysis and automation tools as MCP-discoverable actions.
Demonstrates that `dcc-mcp-core` skills extend beyond DCC applications — any
developer workflow tool can be wrapped as a skill.

## Scripts

- **repo_stats.py** — Analyze repository statistics (commits, contributors, file counts)
- **changelog_gen.py** — Generate changelog from git log between two refs

## Why This Matters

DCC pipelines often involve version-controlled assets. This skill shows how
`dcc-mcp-core` can bridge the gap between creative tools and developer workflows.
