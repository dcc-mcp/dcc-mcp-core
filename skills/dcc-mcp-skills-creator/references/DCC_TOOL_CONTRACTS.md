# DCC-MCP Tool Contracts

Use this checklist for every `tools.yaml` entry.

## Required Shape

- `name`: local snake_case tool name, never dotted.
- `description`: concise action description shown to agents.
- `source_file`: script path relative to the skill directory.
- `input_schema`: JSON Schema for parameters.
- `output_schema`: JSON Schema for returned data when practical.
- `execution`: `sync` for quick calls, `async` for long-running work.
- `affinity`: `main` for host API calls, `any` for pure work.
- `timeout_hint_secs`: realistic upper bound for dispatch and UX.
- `annotations`: MCP safety hints.

## Recovery Chains

Domain tools should include `next-tools.on-failure` entries that point to
diagnostic or observation tools, such as screenshots, audit logs, or scene
snapshots. Infrastructure tools can omit failure chains when they are already
the recovery target.

## Call Examples

For high-frequency or parameter-rich tools, add `call_examples` so agents can
construct valid arguments on the first attempt without trial-and-error describe
retries. Each example is a ready-to-copy payload.

```yaml
tools:
  - name: export_fbx
    # ... other fields ...
    call_examples:
      - arguments:
          path: "C:/exports/scene.fbx"
          selected_only: true
        note: "Export selected objects to FBX with default settings"
      - arguments:
          path: "C:/exports/animation.fbx"
          bake_animation: true
          start_frame: 1
          end_frame: 120
```

Guidelines:
- Each entry must have an `arguments` object matching `input_schema.properties`.
- Optional `note` describes what the example demonstrates.
- List at most 3 examples; one well-chosen example beats three generic ones.
- Server passes examples through to describe responses at
  `metadata.dcc.call_examples` — agents see them without extra round trips.
- This is an optional field. Tools with simple schemas (≤2 properties) or that
  are always called with different arguments can omit it.

## Core Boundary

Keep configuration in `SKILL.md` frontmatter under `metadata.dcc-mcp.*`, and
keep large payloads in sibling files such as `tools.yaml`, `prompts/*.yaml`,
`workflows/*.yaml`, or `references/*.md`.

Do not parse `SKILL.md`, `tools.yaml`, `groups.yaml`, prompts, or workflows from
adapter runtime code when core exposes a catalog or typed skill object API. If a
needed transform or hook is missing, create a core RFC and keep the adapter shim
narrow until the core API exists.
