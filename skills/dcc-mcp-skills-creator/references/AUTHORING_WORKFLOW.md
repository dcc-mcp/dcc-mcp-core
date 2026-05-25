# DCC-MCP Skill Authoring Workflow

Use this workflow when creating or modernizing a skill package that will be
loaded by a DCC-MCP adapter.

## 1. Pick The Right Scope

- Use `infrastructure` for reusable primitives shared across hosts.
- Use `domain` for host or workflow-specific operations, such as `nuke-comp` or `maya-geometry`.
- Use `thin-harness` for a deliberately small raw scripting fallback with recipes.
- Use `example` for authoring references that should not be loaded in production.

If the task is to create the adapter repository itself, switch to
`dcc-mcp-creator`.

## 2. Shape Discovery First

Agents find skills from `name`, `description`, and `metadata.dcc-mcp.search-hint`.
Keep those fields concrete:

- Say what the skill does.
- Say when to use it.
- Say when not to use it, and name the better skill when one exists.

## 3. Keep Runtime Scripts Host-Safe

Scripts should lazy-import host APIs inside the callable function. This keeps
catalog discovery, validation, and server startup available without a running
host process.

Use host-thread affinity only where needed:

- `affinity: main` for host API calls and scene mutations.
- `affinity: any` for pure filesystem, math, parsing, or metadata work.

## 4. Validate Before Loading

Run the creator validation tool or `dcc_mcp_core.validate_skill()` before adding
the skill to an adapter's default path. Treat validation warnings as design
feedback, not only syntax feedback.
