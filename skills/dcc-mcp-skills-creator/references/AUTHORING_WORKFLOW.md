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

The `metadata:` configuration block belongs in `SKILL.md` frontmatter. Put
DCC-MCP extension pointers such as `tools`, `prompts`, `recipes`, `workflows`,
and `depends` under `metadata.dcc-mcp.*`. Use `references/` for long-form docs,
recipes, examples, and notes that agents should load only when needed.

### Gateway-Facing Tags (`metadata.dcc-mcp.tags`)

Gateway search treats `tags` as a narrowing filter. Declare `tags` under
`metadata.dcc-mcp.tags` in `SKILL.md` frontmatter so pipeline,
production-tracking, and documentation connectors rank and filter consistently
across hosts. The canonical tag vocabulary:

| Tag | Use for |
|-----|---------|
| `pipeline` | Studio pipeline systems, publish/intake/review automation, and production data hand-offs. |
| `production-tracking` | Shot/asset/task/status tracking systems regardless of vendor. |
| `shotgrid` | Autodesk Flow Production Tracking / ShotGrid-specific tools. |
| `ftrack` | ftrack-specific tools. |
| `docs` | Documentation, product help, reference lookup, and guide resources. |
| `read-only` | Discovery/read operations. Also set MCP `readOnlyHint` (`annotations.read_only_hint: true` in `tools.yaml`); the tag is for search, not policy. |
| `destructive` | Mutating or irreversible operations. Also set MCP `destructiveHint` (`annotations.destructive_hint: true` in `tools.yaml`); the tag is for search, not policy. |

Current `tags` semantics are **AND**: a search containing both `pipeline` and
`production-tracking` returns records that carry both tags. Planned search
fields will add `tags_any` for OR-style matching and `dcc_types[]` for
multi-host filtering; until those fields land, send separate
`POST /v1/search` requests for alternatives and use singular `dcc_type` for one
backend family.

**Vendor tags** can be added when they sharpen routing without replacing the
canonical tags. For example, Autodesk Product Help should use `docs`,
`read-only`, and the vendor tag `autodesk`. Do not add `docs` to a
production-tracking search unless the user explicitly asks for help or reference
material.

**Tagging rule of thumb:**
- Pipeline/ShotGrid skills → `pipeline`, `production-tracking`, plus
  vendor-specific (`shotgrid` or `ftrack`).
- Documentation connectors → `docs`, `read-only`, plus vendor tag.
- Individual read tools → add `read-only` in `tools.yaml` tags.
- Mutating publish/update tools → add `destructive` in `tools.yaml` tags.

## 3. Keep Runtime Scripts Host-Safe

Scripts should lazy-import host APIs inside the callable function. This keeps
catalog discovery, validation, and server startup available without a running
host process.

Import shared helper APIs from `dcc_mcp_core.skills_helper` before adding small
dependencies or local utility modules. That namespace is the preferred path for
JSON/YAML codecs, bounded HTTP requests, safe file/path helpers, validation,
result envelopes, argument normalization, and cancellation checks. Keep
`requests`, PyYAML, custom HTTP/file helpers, or SDK-specific libraries only
when they provide behavior `skills_helper` intentionally does not cover, such
as sessions, streaming, multipart upload, custom retry/auth flows, or rich
domain file formats.

Use host-thread affinity only where needed:

- `affinity: main` for host API calls and scene mutations.
- `affinity: any` for pure filesystem, math, parsing, or metadata work.

## 4. Validate Before Loading

Run the creator validation tool or `dcc_mcp_core.validate_skill()` before adding
the skill to an adapter's default path. Treat validation warnings as design
feedback, not only syntax feedback.

`validate_skill_dir` adds `skill-helper-adoption` warnings for scripts that
import avoidable dependencies covered by `skills_helper`. New generated and
reference skills should ship without those warnings; legacy production skills
can migrate one helper category at a time while their existing tests stay green.
