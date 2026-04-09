---
name: git-automation
description: "Git repository analysis and automation — inspect commits, branches, diffs, and file history. Use when analysing a codebase, reviewing changes, or automating version control workflows."
license: MIT
compatibility: Requires git on PATH
allowed-tools: Bash Read
metadata:
  category: devops
  openclaw:
    requires:
      bins:
        - git
    emoji: "🔀"
    homepage: https://git-scm.com
tags: [git, vcs, automation, devops]
dcc: python
version: "1.0.0"
tools:
  - name: log
    description: Show recent commit history
    input_schema:
      type: object
      properties:
        limit:
          type: integer
          description: Number of commits to show
          default: 20
        format:
          type: string
          description: Log format (oneline, short, full)
          default: oneline
    read_only: true
    idempotent: true
    source_file: scripts/log.py

  - name: diff
    description: Show changes between commits or working tree
    input_schema:
      type: object
      properties:
        from_ref:
          type: string
          description: Base commit/branch (default HEAD)
        to_ref:
          type: string
          description: Target commit/branch
    read_only: true
    idempotent: true
    source_file: scripts/diff.py
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
