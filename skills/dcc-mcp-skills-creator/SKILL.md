---
name: dcc-mcp-skills-creator
description: >-
  Infrastructure skill - create, validate, scaffold, and review DCC-MCP skills
  for the dcc-mcp-core ecosystem. Use when authoring SKILL.md, tools.yaml,
  scripts, groups, prompts, or skill taxonomy. Not for creating a full DCC-MCP
  adapter repository - use dcc-mcp-creator.
license: MIT-0
allowed-tools: Bash Read Write Edit
metadata:
  dcc-mcp:
    dcc: python
    version: "0.19.59"  # x-release-please-version
    layer: infrastructure
    compatibility: "Python 3.7+, dcc-mcp-core 0.17+"
    search-hint: "create dcc mcp skill, validate skill, scaffold skill, SKILL.md, tools.yaml, scripts, groups, prompts, skill taxonomy"
    tools: tools.yaml
    prompts: prompts.yaml
    skill-reference-docs:
      - "references/*.md"
  openclaw:
    homepage: https://github.com/dcc-mcp/dcc-mcp-core/blob/main/skills/dcc-mcp-skills-creator/SKILL.md
---

# DCC-MCP Skills Creator

A first-class meta-skill for creating, validating, and reviewing DCC-MCP skill
packages. It bundles scaffold/validation tools together with agent-facing
authoring guidance for `SKILL.md`, `tools.yaml`, scripts, groups, prompts, and
progressive-loading taxonomy.

Use `dcc-mcp-creator` when the task is to create a full adapter repository for
a host such as Nuke, Blender, 3ds Max, Unreal, ZBrush, Houdini, or Maya. Use
this skill when the task is to create or improve the skill packages loaded by
those adapters.

## Installation

This skill ships with dcc-mcp-core. Add it to your skill path:

```bash
# Linux/macOS
export DCC_MCP_SKILL_PATHS="${DCC_MCP_SKILL_PATHS}:$(python -c 'import dcc_mcp_core; print(dcc_mcp_core.__file__)')/../skills"

# Windows
set DCC_MCP_SKILL_PATHS=%DCC_MCP_SKILL_PATHS%;C:\path\to\dcc-mcp-core\skills
```

Or reference it directly when starting your MCP server:

```python
from dcc_mcp_core import create_skill_server, McpHttpConfig

server = create_skill_server(
    "maya",
    McpHttpConfig(),
    extra_paths=["/path/to/dcc-mcp-core/skills"],
)
handle = server.start()
print(handle.mcp_url())
```

The local instance port is OS-assigned by default. The CLI and gateway discover
the resolved URL through the shared registry; pass an explicit port only when
an external integration requires a fixed listener.

## CLI-First Control Path

Use the `dcc-mcp` skill and `dcc-mcp-cli` for skill discovery, loading,
validation, and live calls whenever the agent can run shell commands. If the
CLI is missing, follow the consent-gated official installation instructions in
`dcc-mcp`. Keep it current with `dcc-mcp-cli update check`, then
`dcc-mcp-cli update apply`; the apply step stages the next CLI launch and does
not replace a running server binary.

## Quick Start

### Create a new skill

```python
# Call the loaded MCP tool:
# dcc_mcp_skills_creator__create_skill(
#     name="maya-rigging",
#     parent_dir="/path/to/skills/dir",
#     dcc="maya",
#     tool_name="create_locator",
#     affinity="main",
# )
```

### Validate an existing skill

```python
from dcc_mcp_core import validate_skill

report = validate_skill("/path/to/my-skill")
if report.has_errors:
    for issue in report.issues:
        print(f"[{issue.severity}] {issue.category}: {issue.message}")
else:
    print("Skill is valid!")
```

### Get a SKILL.md template

```python
# Call the loaded MCP tool:
# dcc_mcp_skills_creator__skill_template()
```

## Skill Directory Structure

```
my-skill/
|-- SKILL.md              # Required: metadata frontmatter + instructions
|-- tools.yaml            # Required when metadata.dcc-mcp.tools points here
|-- scripts/              # Optional: tool implementation scripts
|   `-- create_locator.py
`-- references/           # Optional: recipes, examples, and long-form docs
    |-- RECIPES.md
    `-- NOTES.md
```

## Current Tool Contract

Generated `tools.yaml` entries follow the modern contract:

- Local tool names are snake_case and client-safe. Do not use dotted names.
- Loaded tools are published as `<skill-name>__<tool_name>` when namespacing is needed.
- Skill package version metadata lives at `metadata.dcc-mcp.version` in
  `SKILL.md`; a top-level `version` key is rejected by the strict loader.
- Inter-skill dependencies live at `metadata.dcc-mcp.depends` as skill names,
  not repo names or prose-only instructions. Use it when one skill must be
  discovered or loaded before another, for example `depends: ["qt-ui-inspector"]`.
- `input_schema` and `output_schema` are declared explicitly.
- Runtime discovery never imports or executes tool scripts to infer missing
  schemas by default. Treat Python-derived schemas as an authoring-time helper:
  generate them before publishing, then commit the JSON Schema to `tools.yaml`.
- Keep MCP-facing `input_schema` shapes simple: prefer a top-level object with
  `properties`, `required`, primitive `type`, bounds, and descriptions. Put
  mutually exclusive forms, conditional requirements, and cross-field rules in
  the tool script or handler validation instead of `anyOf`, `oneOf`, `allOf`,
  `not`, `if`/`then`/`else`, or dependent-schema keywords.
- `execution` is `sync` or `async`; use `async` for deferred/long-running work.
- `affinity` is explicit. Use `main` for host API or scene mutation work and `any` for pure work.
- `enforce_thread_affinity: true` is emitted so adapter dispatch stays honest.
- `annotations` use MCP hints: read-only, destructive, idempotent, open-world, and deferred.
- `call_examples`: optional list of ready-to-copy argument payloads. Each entry has `arguments` (JSON object matching `input_schema.properties`) and an optional `note`. Surfaced in describe responses at `metadata.dcc.call_examples` so agents can construct correct arguments on the first attempt.

### Computer Use Fallback Contract

- Reuse the bundled `app-ui` skill instead of creating another screenshot,
  pointer, keyboard, or Windows `SendInput` tool set. Declare
  `metadata.dcc-mcp.depends: ["app-ui"]` only when it is a hard workflow
  dependency.
- Keep the visual loop as `app_ui__snapshot` -> `app_ui__act` ->
  `app_ui__snapshot`, and pass the latest `snapshot_id` unchanged. End every
  path with `app_ui__stop_computer_use`. Screenshot coordinates belong to that
  observation only.
- Stateful UI tools must declare `requires_in_process: true` independently of
  `affinity`; keep Computer Use at `affinity: any` so it does not block the DCC
  UI thread while preserving one process-local observation. On Windows, the
  Ctrl+Alt+Esc latch and input owner are shared across adapter processes in the same
  logon session.
- Prefer a `control_id` and semantic UI Automation action. Use raw coordinates
  only when the UI does not expose a stable semantic control.
- Never set `DCC_MCP_COMPUTER_USE_ALLOW_RAW_INPUT` from a skill script. It is an
  operator-owned environment ceiling. Native input also requires the
  adapter/operator to bind its DCC with `DCC_MCP_APP_UI_UIA_PROCESS_ID` or
  `DCC_MCP_APP_UI_UIA_WINDOW_HANDLE`; a skill request may only narrow that
  trusted scope. Propagate `user_interrupted` immediately;
  do not retry the action or fall back to another input path after Ctrl+Alt+Esc.
- Never enter or retry another UI/input path after a policy, authorization,
  authentication, security, confirmation, `desktop_unavailable`, or
  `user_interrupted` result. Computer Use is a capability fallback, not a way
  around a control boundary.
- Keep mutating Computer Use tools annotated as destructive so the calling
  host can apply its confirmation policy. Never add a model-controlled
  `confirmed` argument or treat an environment variable as per-action user
  approval.

## Authoring Workflow

1. Decide whether the skill is infrastructure, domain, thin-harness, or example.
2. Give the skill a kebab-case name and each local tool a snake_case name.
3. Keep host API calls inside scripts, with lazy imports so discovery works without the host running.
4. Import same-directory helper modules directly; in-process runners expose the executing script's directory only for the call, so scripts must not mutate `sys.path` for sibling imports. In particular, do not repeat the legacy pattern shown in [houdini#157](https://github.com/dcc-mcp/dcc-mcp-houdini/pull/157/changes#diff-20f6c4a5b206da54475e771ac54351c25975cbcb533595f074c7f26d07ad09a2R11-R13):

   ```python
   script_dir = str(Path(__file__).resolve().parent)
   if script_dir not in sys.path:
       sys.path.insert(0, script_dir)
   ```

   That mutates process-global import state and leaks across skills. Script-directory lifetime is runtime ownership; use a direct sibling import and let the executor scope resolution to the current call.
5. Import dependency-light runtime helpers from `dcc_mcp_core.skills_helper` first: JSON/YAML codecs, bounded HTTP helpers, safe file/path helpers, validation, cancellation checks, and result helpers.
6. Declare `metadata.dcc-mcp.depends` for prerequisite skills, then declare `execution`, `affinity`, `timeout_hint_secs`, schemas, annotations, and failure recovery chains in `tools.yaml`. Do not rely on runtime Python introspection for missing schemas. For high-frequency tools, add `call_examples` so agents can copy argument payloads without trial-and-error.
7. Put long examples, recipes, and host-specific notes under `references/`.
8. Validate with `validate_skill_dir` or `dcc_mcp_core.validate_skill()` before loading it in an adapter.
9. If the desired behavior requires parsing core internals or adapter-private YAML at runtime, stop and request a core API instead.

## Improve Skills From Completed Tasks

Use retained gateway evidence only after the user-visible task and its
validation are complete. Keep one stable `session_id` in call metadata, then
query the narrowest useful slice:

```bash
dcc-mcp-cli stats --range 24h --dcc-type <dcc> --session-id <session-id>
```

Get the `review_skill_improvement` prompt from this skill and supply the stats
JSON plus bounded task and validation summaries. Treat `total_calls == 0` as
missing evidence, not success. Never include hidden reasoning, raw prompts,
credentials, or unredacted payloads.

Prefer `no_change`, then improving an existing skill, and create a new skill
only for a repeated, reusable workflow that no current skill owns. Validate any
accepted change with `validate_skill_dir` or `dcc-mcp-cli lint` before loading
it. Statistics inform a proposal; they never authorize editing or publishing a
skill without the task owner's requested scope.

When reviewing existing skills, reject top-level DCC-MCP extension keys such
as `dcc`, `version`, `tags`, `tools`, `groups`, `depends`, `search-hint`,
`runtimes`, `prompts`, and `resources`. Move them under
`metadata.dcc-mcp.*`; for version metadata, use
`metadata.dcc-mcp.version: "1.0.0"`. Validate the installable skill directory
that contains the `SKILL.md` loaded by adapters, not only mirrored repository
docs or marketplace metadata.

Read [AUTHORING_WORKFLOW.md](references/AUTHORING_WORKFLOW.md) and
[DCC_TOOL_CONTRACTS.md](references/DCC_TOOL_CONTRACTS.md) before changing a
production skill package.

## Gateway-Facing Tag Taxonomy

Gateway search treats `tags` as a narrowing filter. Use a small shared vocabulary
so pipeline, production-tracking, and documentation connectors rank and filter
consistently across hosts. When authoring `SKILL.md` frontmatter, include the
appropriate tags under `metadata.dcc-mcp.tags`:

| Tag | Use for |
|-----|---------|
| `pipeline` | Studio pipeline systems, publish/intake/review automation, and production data hand-offs. |
| `production-tracking` | Shot/asset/task/status tracking systems regardless of vendor. |
| `shotgrid` | Autodesk Flow Production Tracking / ShotGrid-specific tools. |
| `ftrack` | ftrack-specific tools. |
| `docs` | Documentation, product help, reference lookup, and guide resources. |
| `read-only` | Discovery/read operations. Also set MCP `readOnlyHint` (`annotations.read_only_hint: true` in `tools.yaml`); the tag is for search, not policy. |
| `destructive` | Mutating or irreversible operations. Also set MCP `destructiveHint` (`annotations.destructive_hint: true` in `tools.yaml`); the tag is for search, not policy. |

**Filter semantics:**
- `dcc_type` (singular) + `dcc_types[]` — **OR**: a result matching any listed
  DCC family passes. Include `dcc_type: "maya"` with `dcc_types: ["blender"]`
  to match records from either host in one request.
- `tags[]` — **AND**: a result must carry every listed tag. Use `pipeline` +
  `production-tracking` to narrow to records that carry both.
- `tags_any[]` — **OR**: a result carrying any listed tag passes. Combines with
  the AND filter above: `tags: ["pipeline"]` + `tags_any: ["read-only", "docs"]`
  returns pipeline records that are read-only OR documentation.

**Vendor tags** can be added when they sharpen routing without replacing the
canonical tags. For example, Autodesk Product Help should use `docs`,
`read-only`, and the vendor tag `autodesk`. Do not add `docs` to a
production-tracking search unless the user explicitly asks for help or reference
material.

### Python 3.7 Policy

All authored skills must declare `compatibility: "Python 3.7+"` in their
frontmatter when they are installed into an LTS DCC host. This applies to every
skill that is installed into a DCC host embedding Python 3.7 (Maya 2022,
Blender 2.83, 3ds Max 2022, etc.). `py37-lite` is a supported fallback but
does not replace the native Linux and Windows cp37 compatibility gates. See
ADR 011 and `compatibility/python.json` for the deprecation and CI contract.
In lite mode, `create_skill_server()` supports local metadata discovery
(`list_skills`, `search_skills`, and `get_skill`) only. The Rust sidecar is
dispatch-only, so gateway discovery and declarative `load_skill` execution
require a native Python 3.7 wheel; lite activation fails explicitly.

For hermetic CI or tests, set `DCC_MCP_DISABLE_DEFAULT_SKILL_PATHS=1` so an
operator's local/platform defaults, marketplace installs, and Admin custom
paths cannot alter discovery results. Explicit, bundled, and
`DCC_MCP_*_SKILL_PATHS` paths remain active under this mode.

**Skill SKILL.md example** (frontmatter excerpt):

```yaml
metadata:
  dcc-mcp:
    dcc: shotgrid
    layer: domain
    tags: [pipeline, production-tracking, shotgrid]
    search-hint: "ShotGrid task status, find shots, update task assignments"
    tools: tools.yaml
```

```yaml
# Read-only docs connector (SKILL.md excerpt)
metadata:
  dcc-mcp:
    dcc: autodesk-help
    layer: infrastructure
    tags: [docs, autodesk, read-only, infrastructure]
    search-hint: "Autodesk Product Help, Maya help, 3ds Max help, API reference"
    tools: tools.yaml
```

Individual read tools should also carry `read-only` in their tool-level tags;
mutating publish/update tools should carry `destructive` when applicable.

## Validation Rules

The validator checks:

- **SKILL.md** exists and is readable
- **YAML frontmatter** is well-formed
- **Required fields**: `name`, `description`
- **Name format**: kebab-case, <=64 chars, matches directory name
- **Field lengths**: description <=1024, compatibility <=500
- **Tool declarations**: non-empty names, no duplicates, snake_case client-safe format
- **Script files**: `source_file` references exist in `scripts/`
- **Sidecar files**: `metadata.dcc-mcp.tools/groups/prompts` references exist
- **Dependencies**: `metadata.dcc-mcp.depends` consistency
- **Spec compliance**: non-standard top-level keys are frontmatter errors; dcc-mcp-core extensions must live under `metadata.dcc-mcp.*` and point to sibling files
- **Version metadata**: `metadata.dcc-mcp.version` is accepted and projected
  to `SkillMetadata.version`; top-level `version` fails with an actionable
  migration hint
- **Skill helper adoption**: `validate_skill_dir` emits `skill-helper-adoption` warnings when scripts import avoidable dependencies covered by `dcc_mcp_core.skills_helper`, such as `requests`, `httpx`, PyYAML, or local JSON/HTTP/file/path helper modules
