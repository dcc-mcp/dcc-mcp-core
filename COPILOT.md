# COPILOT.md — dcc-mcp-core

> This file is the entry point for GitHub Copilot agents.
> For full documentation, follow the links below — do **not** read everything upfront.

## Quick Start

This project uses the [agents.md](AGENTS.md) standard. Read `AGENTS.md` first — it is the navigation map that points to all other documentation.

## Document Hierarchy

| Layer | File | When to read it |
|-------|------|-----------------|
| Navigation | [AGENTS.md](AGENTS.md) | First contact — defines response language, PR rules, merge workflow |
| AI-friendly index | [llms.txt](llms.txt) | When you need to *use* APIs |
| Full index | [llms-full.txt](llms-full.txt) | When `llms.txt` lacks detail |
| Detailed rules & traps | [docs/guide/agents-reference.md](docs/guide/agents-reference.md) | Before writing code |
| Conceptual docs | [docs/guide/](docs/guide/) + [docs/api/](docs/api/) | Building a new adapter or skill |
| Skill authoring | [skills/README.md](skills/README.md) + [examples/skills/](examples/skills/) | Creating or modifying skills |

## Response Language

- Reply to the user in **Simplified Chinese** (中文简体) by default.
- Keep all code, identifiers, commit messages, branch names, docstrings, comments, and file contents in **English**.

## Project Overview

**What**: Rust-powered MCP library for DCC software (Maya, Blender, Houdini, Photoshop…). PyO3/maturin. Zero Python runtime deps. MCP 2025-03-26 Streamable HTTP.

**Key entry points**:
- `python/dcc_mcp_core/__init__.py` — every public Python symbol
- `llms.txt` — compressed API index for AI agents
- `AGENTS.md` — navigation map (this document chain starts here)

## Build & Test

```bash
vx just dev      # build wheel
vx just test     # run tests
vx just preflight  # pre-commit check + docs dead-link check
```

## Top Traps — Read Before Coding

See [AGENTS.md → Top Traps](AGENTS.md#top-traps--memorize-these) and [docs/guide/agents-reference.md](docs/guide/agents-reference.md) for the full list.

1. **`scan_and_load` returns a 2-tuple** — always `skills, skipped = scan_and_load(...)`
2. **`success_result` kwargs become context** — `success_result("msg", count=5)`, never `context=`
3. **`ToolDispatcher` uses `.dispatch()`** — never `.call()`
4. **Register ALL handlers BEFORE `server.start()`**
5. **SKILL.md extensions use `metadata.dcc-mcp.<feature>`** — never top-level keys (v0.15+ / #356)
