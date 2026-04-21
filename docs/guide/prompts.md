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

## Source: sibling `prompts.yaml`

Following the project-wide **sibling-file rule** ([#356](https://github.com/loonghao/dcc-mcp-core/issues/356)),
prompt templates never inline into `SKILL.md`. The skill author references
a sibling file from the `metadata.dcc-mcp` namespace:

```yaml
---
name: maya-geometry
description: "Maya geometry primitives and editing."
metadata:
  dcc-mcp.dcc: maya
  dcc-mcp.prompts: prompts.yaml     # single file, or
  # dcc-mcp.prompts: prompts/*.prompt.yaml   # glob, one file per prompt
---
```

`prompts.yaml` contains two top-level lists — both optional:

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
      `diagnostics__screenshot` before saving.

workflows:
  - file: workflows/bake_proxies.workflow.yaml
    prompt_name: bake_proxies_summary      # optional rename
```

### Explicit prompts

Each entry under `prompts:` is a `PromptSpec`:

| Field         | Type              | Required | Notes                                                         |
|---------------|-------------------|----------|---------------------------------------------------------------|
| `name`        | string            | ✅        | Unique within the skill. Fully-qualified when served as MCP. |
| `description` | string            | ✅        | One-line summary the client shows to the user.               |
| `arguments`   | list[ArgumentSpec] | ❌       | Typed placeholders for the template.                         |
| `template`    | string            | ✅        | `{{name}}` placeholders resolved against call-site arguments. |

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

## Templating engine

The rendering engine is intentionally minimal — one token only:
`{{placeholder}}`.

- Whitespace inside braces is trimmed: `{{ foo }}` == `{{foo}}`.
- An undeclared required argument raises
  `INVALID_PARAMS: missing required argument: <name>`.
- Brace content that isn't a bare identifier (`{{ 1 + 1 }}`) is left
  untouched — the engine never evaluates expressions.
- Unclosed `{{` with no matching `}}` is emitted verbatim.

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

## Related issues

- [#351](https://github.com/loonghao/dcc-mcp-core/issues/351) — MCP prompts primitive
- [#355](https://github.com/loonghao/dcc-mcp-core/issues/355) — prompts derived from SKILL.md examples + workflows
- [#348](https://github.com/loonghao/dcc-mcp-core/issues/348) — workflow specs (source for auto-derived prompts)
- [#356](https://github.com/loonghao/dcc-mcp-core/issues/356) — sibling-file pattern
- [#350](https://github.com/loonghao/dcc-mcp-core/issues/350) — MCP resources primitive (sister feature)
