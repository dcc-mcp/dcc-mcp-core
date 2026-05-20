# Skill Authoring Checklist

Use this before creating or reviewing a DCC-MCP adapter skill package.

## SKILL.md

- Use agentskills.io frontmatter only at the root: `name`, `description`,
  `license`, `compatibility`, `metadata`, and `allowed-tools`.
- Put all DCC-MCP extensions under `metadata.dcc-mcp.*`.
- Include `metadata.dcc-mcp.layer`.
- Use `metadata.dcc-mcp.dcc` for host-bound skills.
- Use `metadata.dcc-mcp.tools: tools.yaml` when tools exist.
- Keep the body short and move detailed patterns to `references/`.
- Write the description as routing metadata:
  `<Layer> skill - <scope>. Use when <trigger>. Not for <counter-example>.`

## tools.yaml

Every tool declaration should include:

- `name`: snake_case action name.
- `description`: specific user intent, side effects, and output.
- `source_file`: path under `scripts/`.
- `execution`: `sync` or `async`.
- `affinity`: `main` for scene or host API work; `any` only for pure work.
- Safety annotations: `read_only`, `destructive`, and `idempotent`.
- `input_schema`: JSON Schema with useful descriptions.
- `timeout_hint_secs`: required for async or long-running operations.
- `next-tools.on-failure`: diagnostics for domain tools, usually screenshot and
  audit log.

Prefer one typed tool per durable user intent. Do not expose raw script
execution as the primary workflow when a schema can guide the agent safely.

## scripts/

Keep scripts importable in ordinary Python when possible:

```python
def create_thing(name: str) -> dict:
    # Import host APIs inside the function so metadata parsing stays safe.
    import host_api

    node = host_api.create_node(name)
    return {"success": True, "message": "Created node", "context": {"node": node}}
```

Adapter helpers are better than hand-built result dictionaries when available:

- Maya: use `with_maya`, `maya_success`, and `maya_error` patterns.
- Future adapters: add `<dcc>_success`, `<dcc>_error`, or `with_<dcc>` only when
  they remove repeated host-specific error handling.
- Core-only scripts: prefer `ToolResult.ok(...).to_dict()` or
  `ToolResult.fail(...).to_dict()`.

Never import `maya.cmds`, `bpy`, `pymxs`, or other host modules at file import
time. Keep imports inside tool functions or adapter-dispatched callables.

## Stage Taxonomy

Use `metadata.dcc-mcp.stage` when the adapter has enough skills to benefit from
progressive loading presets:

- `bootstrap`: health, setup, environment, plugin discovery.
- `scene`: scene inventory, selection, hierarchy, units, frame range.
- `authoring`: create or modify geometry, materials, animation, rigs.
- `interchange`: import, export, USD, FBX, Alembic, cache operations.
- `pipeline`: studio workflows, publish, validation, render farm handoff.

Keep stage names data-driven. Core should not need to know every host's stage
vocabulary.

## Review Questions

- Can an agent find this skill with a user-intent query?
- Does each tool tell the agent what it changes and what it returns?
- Are DCC identity, paths, and examples parameterized?
- Does every scene-touching operation enforce main-thread affinity?
- Can the script be imported without the DCC installed?
- Is there a low-level unit test and at least one discovery/load/call path?
