---
name: git-automation
description: >-
  Infrastructure skill — Git repository analysis and automation: inspect
  commits, branches, diffs, and file history. Use when analysing a codebase,
  reviewing changes, or automating version control workflows independent of any
  DCC. Not for DCC asset versioning or Perforce operations — use a domain skill
  bound to the specific VCS for those.
license: MIT
compatibility: Requires git on PATH
allowed-tools: Bash Read
metadata:
  dcc-mcp.dcc: python
  dcc-mcp.version: "1.0.0"
  dcc-mcp.layer: infrastructure
  dcc-mcp.search-hint: "git commit, git diff, git branch, git log, version control, codebase analysis, git history"
  dcc-mcp.tags: "git, vcs, automation, devops, infrastructure"
  dcc-mcp.tools: tools.yaml
  openclaw:
    requires:
      bins:
        - git
    emoji: "🔀"
    homepage: https://git-scm.com
---

# Git Automation Tools

Analyse and automate Git repositories from within an AI agent workflow.

## Tools

### `git_automation__log`
Show commit history with configurable depth and format.

### `git_automation__diff`
Show the diff between commits, branches, or the working tree.

## Prerequisites

Git must be installed and the working directory must be inside a Git repository.
