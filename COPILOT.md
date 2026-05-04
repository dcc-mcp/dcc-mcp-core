# COPILOT.md — dcc-mcp-core

> Entry point for GitHub Copilot agents.
> This file is an agent-specific entry point. It intentionally delegates to
> `AGENTS.md` so project rules stay single-sourced and progressively disclosed.
>
> Do not copy detailed project guidance here. Update `AGENTS.md`, `llms.txt`,
> `llms-full.txt`, or `docs/guide/*` instead.

## Read Order

1. `AGENTS.md` — navigation map, response rules, PR/merge rules, and top traps.
2. `AI_AGENT_GUIDE.md` — how an AI agent should use dcc-mcp-core effectively.
3. `llms.txt` — compact API index when you need to use APIs.
4. `llms-full.txt` — complete API index when `llms.txt` lacks detail.
5. `docs/guide/agents-reference.md` — detailed rules, examples, and rationale before coding.

## Mandatory DCC Workflow

When interacting with Maya, Blender, Houdini, Photoshop, ZBrush, Unreal, Unity,
Figma, or any custom DCC host, use the Skills-First workflow from `AGENTS.md`.
Prefer `search_skills` → `load_skill` → tool call on a per-DCC server, or
`search_tools` → `describe_tool` → `call_tool` on a gateway/slim REST surface.
Do not jump straight to raw subprocesses or host scripting unless the docs say a
low-level fallback is required.

## Build & Test

Use `vx just dev`, `vx just test`, and `vx just preflight`. For docs-only changes,
still run the fastest available docs validation before opening a PR.
