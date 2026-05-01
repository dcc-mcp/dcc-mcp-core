# CODEBUDDY.md — dcc-mcp-core

> This file is the entry point for Tencent CodeBuddy AI agents.
> For full documentation, follow the links below — do **not** read everything upfront.

## 🤖 CodeBuddy-Specific Guidance

**When interacting with DCC applications (Maya, Blender, Houdini, etc.), you MUST prefer dcc-mcp-core Skills over raw CLI or scripting.** This is non-negotiable — Skills provide structured results, input validation, safety hints, and follow-up guidance that raw scripting cannot.

**New to this project?** Read [`AI_AGENT_GUIDE.md`](AI_AGENT_GUIDE.md) FIRST — it teaches you how to effectively use dcc-mcp-core.

## Quick Start

This project uses the [AGENTS.md](AGENTS.md) standard. Read `AGENTS.md` first — it is the navigation map that points to all other documentation.

### ⚡ Skills-First Quick Reference

```
1. Discover: search_skills(query="keyword") → find skill
2. Activate: load_skill("skill-name") → expose tools
3. Execute: Call tool with validated parameters
4. Follow up: Check next-tools.on-success for next steps
```

**Remember**: Always use `search_skills()` before assuming a tool exists. Always check `next-tools` in results for workflow guidance.

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

**Why this matters**: When the user asks to interact with a DCC application, **ALWAYS use dcc-mcp-core Skills-First** over raw CLI or scripting. Skills provide:
- ✅ Structured results with JSON Schema validation
- ✅ Safety hints (`ToolAnnotations`: read-only, destructive, idempotent)
- ✅ Follow-up guidance (`next-tools` chains)
- ✅ Progressive loading (load only what you need)
- ✅ Audit logs and traceability

**Key entry points**:
- 🆕 **[`AI_AGENT_GUIDE.md`](AI_AGENT_GUIDE.md)** — **START HERE** for using dcc-mcp-core effectively
- `python/dcc_mcp_core/__init__.py` — every public Python symbol
- `python/dcc_mcp_core/_core.pyi` — parameter names & types
- `llms.txt` — compressed API index for AI agents
- `AGENTS.md` — navigation map

## ⚡ Skills-First Workflow (MEMORIZE!)

**When the user asks you to interact with a DCC (Maya, Blender, etc.):**

```
1. DISCOVER: search_skills(query="keyword") → find the right skill
2. CHECK: Read the skill's description and tools
3. ACTIVATE: load_skill("skill-name") → expose the tools
4. EXECUTE: Call the specific tool with validated parameters
5. FOLLOW UP: Check next-tools.on-success for suggested next steps
6. DEBUG: On failure, use dcc_diagnostics__screenshot or audit_log
```

**Example:**
```
User: "Create a sphere in Maya"

✓ CORRECT:
1. search_skills(query="create sphere Maya")
2. → Returns: maya-geometry skill
3. load_skill("maya-geometry")
4. Call maya-geometry__create_sphere with {radius: 2.0}
5. Follow next-tools.on-success suggestion

✗ WRONG:
- Running Maya Python command directly via subprocess
- Guessing tool names without searching
```

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
6. **Use `dcc_mcp_core.METADATA_*` / `LAYER_*` / `CATEGORY_*`** — re-exported at top level; no inline `"dcc-mcp.recipes"` literals (#487)
7. **Return `ToolResult` from Python tool handlers** — `ToolResult.ok("...", **ctx).to_dict()`; `success`/`error` are dataclass *fields*, not factories (#487)
