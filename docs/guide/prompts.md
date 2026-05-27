# MCP Prompts Primitive

> Implements [MCP 2025-03-26 — Prompts](https://modelcontextprotocol.io/specification/2025-03-26/server/prompts)
> for dcc-mcp-core. Issues [#351](https://github.com/loonghao/dcc-mcp-core/issues/351)
> and [#355](https://github.com/loonghao/dcc-mcp-core/issues/355).

The **prompts** primitive is the third MCP surface `McpHttpServer`
advertises, alongside `tools` and `resources`. It gives AI clients a way
to discover reusable **prompt templates** — natural-language instructions
parameterised with arguments — that a skill author has hand-crafted to
elicit the right behavioural chain from the model.

Unlike `tools/call` (which executes side effects) and `resources/read`
(which returns opaque bytes), `prompts/get` returns a **rendered message
array** the client can splice straight into the conversation, preserving
the skill author's intent without the model having to guess the right
phrasing.

## Wire protocol

When enabled, the server advertises the primitive in `initialize`:

```json
{
  "capabilities": {
    "prompts": { "listChanged": true }
  }
}
```

Three JSON-RPC methods are exposed:

| Method | Purpose |
|--------|---------|
| `prompts/list` | Return every registered prompt — name, description, argument schema. |
| `prompts/get` | Render one prompt by name with caller-supplied arguments. |
| `notifications/prompts/list_changed` | Server-pushed SSE event whenever a skill is loaded / unloaded. |

## Source: explicit sibling `prompts.yaml`

Following the project-wide **sibling-file rule** ([#356](https://github.com/loonghao/dcc-mcp-core/issues/356)),
hand-authored prompt templates never inline into `SKILL.md`. The skill author
references a sibling file from the `metadata.dcc-mcp` namespace:

```yaml
---
name: maya-geometry
description: "Maya geometry primitives and editing."
metadata:
  dcc-mcp:
    dcc: maya
    prompts: prompts.yaml     # single file, or
    # prompts: prompts/*.prompt.yaml   # glob, one file per prompt
---
```

`prompts.yaml` contains two top-level lists — both optional:

::: v-pre
```yaml
prompts:
  - name: bevel_all_edges
    description: "Bevel every selected edge with a consistent chamfer width."
    arguments:
      - name: chamfer_width
        description: "Chamfer width in Maya units."
        required: true
      - name: segments
        description: "Number of chamfer segments (default 2)."
        required: false
    template: |
      Use `maya_geometry__select_edges` to capture the current selection,
      then call `maya_geometry__bevel_edges` with width={{chamfer_width}}
      and segments={{segments}}. Verify the result with
      `dcc_diagnostics__screenshot` before saving.

workflows:
  - file: workflows/bake_proxies.workflow.yaml
    prompt_name: bake_proxies_summary      # optional rename
```
:::

### Explicit prompts

Each entry under `prompts:` is a `PromptSpec`:

| Field         | Type              | Required | Notes                                                         |
|---------------|-------------------|----------|---------------------------------------------------------------|
| `name`        | string            | ✅        | Unique within the skill. Fully-qualified when served as MCP. |
| `description` | string            | ✅        | One-line summary the client shows to the user.               |
| `arguments`   | list[ArgumentSpec] | ❌       | Typed placeholders for the template.                         |
| `template`    | string            | ✅        | <code v-pre>`{{name}}`</code> placeholders resolved against call-site arguments. |

`ArgumentSpec` fields: `name`, `description`, `required` (default `false`).

### Workflow-derived prompts

The `workflows:` list auto-generates a summary prompt per referenced
workflow. This is a minimal behavioural chain hint — "here are the steps
that workflow runs, in order" — suitable for agents that want to narrate
the workflow before (or instead of) executing it:

```yaml
workflows:
  - file: workflows/bake_proxies.workflow.yaml
```

No `template` is needed; the registry summarises the workflow's
description + step list into a user-role message. Use `prompt_name` to
override the default auto-generated name.

### Single file vs glob

Both forms are accepted in `metadata.dcc-mcp.prompts`:

- `prompts.yaml` — single file with `prompts:` + `workflows:` lists.
- `prompts/*.prompt.yaml` — glob, one file per prompt. Each file has the
  same shape as a single entry in `prompts:`.

Parsing is **lazy**: the path is recorded at scan / load time; the file
contents are only read when the server handles `prompts/list` or
`prompts/get`.

## Derived prompts

If a loaded skill does not declare `metadata.dcc-mcp.prompts`, the registry
tries conservative, explainable derivation from prompt-worthy metadata:

| Metadata | Source files | Derived prompt name |
|----------|--------------|---------------------|
| `metadata.dcc-mcp.examples` | Markdown/text examples such as `references/EXAMPLES.md` or `examples/*.md` | `<skill>.examples` or `<skill>.examples.<file-stem>` |
| `metadata.dcc-mcp.recipes` | Markdown, YAML, TOML, or text recipe references | `<skill>.recipes` or `<skill>.recipes.<file-stem>` |
| `metadata.dcc-mcp.workflows` | `*.workflow.yaml` workflow specs | `<skill>.<workflow-name>` |

Derived prompts are meant to make `prompts/list` useful for adapters that
already ship examples, recipes, or workflow metadata. They are intentionally
plain: the rendered prompt includes the referenced guidance or a workflow step
summary and tells the agent to prefer the skill's declared MCP tools.

Use explicit `prompts.yaml` when you need argument schemas, carefully worded
model instructions, stable prompt names, or curated UX copy. Use derived
prompts as a zero-adapter-code fallback for existing examples and workflow
guidance.

Each prompt entry carries source metadata:

```json
{
  "_meta": {
    "dcc.prompt_source": {
      "skill": "maya-geometry",
      "source": "examples"
    }
  }
}
```

The gateway preserves that metadata while also adding `_instance_id`,
`_instance_short`, and `_dcc_type` for the backend that supplied the prompt.

## Templating engine

The rendering engine is intentionally minimal — one token only:
::: v-pre
`{{placeholder}}`.
:::

- Whitespace inside braces is trimmed: <code v-pre>`{{ foo }}`</code> == <code v-pre>`{{foo}}`</code>.
- An undeclared required argument raises
  `INVALID_PARAMS: missing required argument: <name>`.
- Brace content that isn't a bare identifier (<code v-pre>`{{ 1 + 1 }}`</code>) is left
  untouched — the engine never evaluates expressions.
- Unclosed <code v-pre>`{{`</code> with no matching `}}` is emitted verbatim.

Keep templates small and declarative. If a template needs loops,
conditionals, or data fetches, author it as a **workflow** (issue #348)
instead and reference it from the `workflows:` list.

## Server configuration

The primitive is **enabled by default**. Disable it globally with
`McpHttpConfig.enable_prompts = false`:

```python
from dcc_mcp_core import McpHttpConfig, create_skill_server

cfg = McpHttpConfig(port=8765)
cfg.enable_prompts = False     # opt out — capability vanishes from initialize
server = create_skill_server("maya", cfg)
server.start()
```

When disabled, the server omits the `prompts` capability and rejects
`prompts/list` / `prompts/get` with `Method not found`.

## Diagnostics

An empty `prompts/list` response includes a diagnostic path in `_meta` so
clients can distinguish "no loaded skills" from "loaded skills have no prompt
metadata" and "prompt-capable metadata failed to load":

```json
{
  "prompts": [],
  "_meta": {
    "dcc.prompt_diagnostics": {
      "enabled": true,
      "loaded_skill_count": 1,
      "prompt_count": 0,
      "prompt_capable_skill_count": 0,
      "notes": [
        "Loaded skills did not declare metadata.dcc-mcp.prompts, examples, recipes, or workflows."
      ]
    }
  }
}
```

The REST `GET /v1/prompts` surface exposes the same information under
`diagnostics`. Gateway aggregation adds backend-level diagnostics under
`_meta["dcc.prompt_diagnostics"].backends`, including per-backend prompt counts
and transport/protocol errors, while still returning healthy backend prompts.

## Dynamic registration from Python

Python adapters can also register prompt templates directly before or after
server start. `server.prompts()` returns a `PromptHandle`; `register_prompt`
upserts into that handle and does not return a new handle.

```python
from dcc_mcp_core import McpHttpConfig, McpHttpServer, ToolRegistry

registry = ToolRegistry()
server = McpHttpServer(registry, McpHttpConfig(port=8765))

prompts = server.prompts()
prompts.register_prompt(
    name="bake_animation",
    description="Guide an agent through baking animation keys.",
    arguments=[
        {"name": "frame_start", "description": "First frame", "required": True},
        {"name": "frame_end", "description": "Last frame", "required": True},
    ],
    template="Bake the active animation from {{frame_start}} to {{frame_end}}.",
)

# Later dynamic updates are visible to subsequent prompts/list calls.
prompts.unregister_prompt("bake_animation")
prompts.clear()
```

Argument names must be non-empty and unique. Prefer sibling `prompts.yaml` for
packaged skills; use dynamic registration for adapter-owned prompts that depend
on runtime capabilities.

## `list_changed` invariants

- `notifications/prompts/list_changed` fires whenever the set of loaded
  skills changes (`skills/load`, `skills/unload`, hot-reload).
- The registry's internal cache is invalidated in the same critical
  section that emits the notification — clients may call `prompts/list`
  immediately after and will observe the new set.
- The notification fans out to every active SSE session; there is no
  per-session subscription model for prompts.

## Guidance for skill authors

1. Keep `template` bodies **under 50 lines**. Longer guidance belongs in
   `references/` and should be pulled in by the workflow layer.
2. Prefer **fully-qualified tool names** inside templates
   (`maya_geometry__bevel_edges`) — the agent then doesn't have to guess.
3. If the behavioural chain is more than 3-4 tool calls, extract it to a
   `workflow.yaml` and let the server auto-generate the summary prompt.
4. Don't put secrets or environment-specific paths in templates —
   prompts are surfaced verbatim to the model.
5. Treat derived prompts as a fallback. Once an example or recipe becomes a
   product-facing workflow, promote it to an explicit `prompts.yaml` entry with
   a stable name and argument declarations.

## Related issues

- [#351](https://github.com/loonghao/dcc-mcp-core/issues/351) — MCP prompts primitive
- [#355](https://github.com/loonghao/dcc-mcp-core/issues/355) — prompts derived from SKILL.md examples + workflows
- [#348](https://github.com/loonghao/dcc-mcp-core/issues/348) — workflow specs (source for auto-derived prompts)
- [#356](https://github.com/loonghao/dcc-mcp-core/issues/356) — sibling-file pattern
- [#350](https://github.com/loonghao/dcc-mcp-core/issues/350) — MCP resources primitive (sister feature)
