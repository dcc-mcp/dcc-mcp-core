# DCC-MCP Tool Contracts

Use this checklist for every `tools.yaml` entry.

## Required Shape

- `name`: local snake_case tool name, never dotted.
- `description`: concise action description shown to agents.
- `source_file`: script path relative to the skill directory; Python sources
  define module-level `main(...)` and call `run_main(main)` when run directly.
- `input_schema`: JSON Schema for parameters.
- `output_schema`: JSON Schema for returned data when practical.
- `execution`: `sync` for quick calls, `async` for long-running work.
- `affinity`: `main` for host API calls, `any` for pure work.
- `timeout_hint_secs`: realistic upper bound for dispatch and UX.
- `annotations`: MCP safety hints. Explicitly set all four boolean fields:
  `read_only_hint`, `destructive_hint`, `idempotent_hint`, and
  `open_world_hint`.

Runtime discovery is manifest-first. Missing `input_schema` falls back to a
permissive `{"type": "object"}` instead of importing or executing the script.
If you derive schemas from Python annotations, do it while authoring and write
the result into `tools.yaml`.

## Result Envelope

Python skill scripts should return `skill_success(...)`, `skill_error(...)`,
or another helper from `dcc_mcp_core.skill`. Lower-level handlers may use
`ToolResultEnvelope` from `dcc_mcp_core.result_envelope`. Do not hand-roll a
result mapping.

The canonical fields are `success`, `message`, `error`, `prompt`, `context`,
and optional `_meta`. A failure's `error` must be a stable string code (for
example `invalid_input`, `RuntimeError`, or `SandboxDenied`), never an object.
Put structured exception details under `_meta["dcc.error"]` and raw DCC call
diagnostics under `_meta["dcc.raw_trace"]`. Keep ordinary tool outputs and
identifiers in `context`.

The general builder may omit empty optional fields. Skill helpers intentionally
retain their historical fixed-key shape, so consumers should rely on field
types and semantics rather than treating omission and `None` as different
outcomes. Top-level `dcc_mcp_core.ToolResult` is the distinct Rust-backed
runtime model; use `ToolResultEnvelope` when building a Python wire mapping.

A zero-argument tool must not use that permissive fallback. Declare the closed
empty-object contract explicitly:

```yaml
input_schema:
  type: object
  properties: {}
  additionalProperties: false
```

The gateway may skip `describe` only when the tool is known to take no
arguments and all four safety annotations are present as booleans. Missing
safety fields fail closed to `describe`. Schemas that use `$ref`, composition,
conditionals, dependent schemas, pattern properties, or other complex JSON
Schema features also require `describe`; do not treat a compact property list
as the full validation contract.

## Progressive Loading

Keep every tool group independently usable. When a search result supplies a
correlated `target_tool_slug`, loading activates only that target tool's group.
Do not depend on default-active sibling groups being enabled as a side effect.
An ordinary uncorrelated skill load keeps the declared default activation
behavior.

## Performance Regression Checks

For discovery and load-path performance regressions, assert deterministic
backend operation counts: searches, catalog/tool-list refreshes, loads, group
activations, and describes. Wall-clock thresholds vary with CI load and should
only supplement those contract checks.

## Sibling Imports

Import same-directory helpers directly, for example:

```python
from _material_common import get_node
```

Do not mutate global import state inside a skill script:

```python
# Invalid: do not change sys.path inside skill scripts.
_SCRIPT_DIR = str(Path(__file__).resolve().parent)
if _SCRIPT_DIR not in sys.path:
    sys.path.insert(0, _SCRIPT_DIR)
```

Do not copy the historical path-insertion pattern shown in
[dcc-mcp-houdini PR #157, lines 11-13](https://github.com/dcc-mcp/dcc-mcp-houdini/pull/157/changes#diff-20f6c4a5b206da54475e771ac54351c25975cbcb533595f074c7f26d07ad09a2R11-R13).
It is an explicit example of what a Skill script must not do.

The in-process runner temporarily exposes the executing script directory for
the call. If another runner cannot resolve a sibling import, fix that shared
runner contract instead of adding `sys.path.insert()` or `sys.path.append()` to
every script.

## Recovery Chains

Domain tools should include `next-tools.on-failure` entries that point to
diagnostic or observation tools, such as screenshots, audit logs, or scene
snapshots. Infrastructure tools can omit failure chains when they are already
the recovery target.

## Long-Running Progress

Use `execution: async` for render, cook, bake, simulation, and export work that
outlives one request. Choose `job_strategy: chunked` for bounded host-main
steps and `job_strategy: isolated` when a renderer, farm, subprocess, or service
owns a durable operation. Do not claim cancellation or resumability for a
monolithic native call that cannot provide them.

Every status surface should reuse the Core job vocabulary:

- `status`: `pending`, `running`, `completed`, `failed`, `cancelled`, or `interrupted`.
- `progress.current`: monotonic completed work units.
- `progress.total`: monotonic total work units using the same unit.
- `progress.message`: optional bounded phase/frame/node description.

For frame rendering, use completed frames and requested frames. Prefer native
renderer/cook counters; if verified output files are the only available source,
derive the count inside the typed status tool and report missing/failed units.
The agent must not reconstruct progress with repeated shell directory scans.

Declare read-only status and mutating cancel tools in `next-tools`. An agent may
create a one-shot cross-session status check only after explicit user consent;
the check keeps the existing job/operation id, never launches work, and removes
itself at terminal state. Core `schedules.yaml` is for predefined cron/webhook
workflows, not ad-hoc polling of one render.

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
