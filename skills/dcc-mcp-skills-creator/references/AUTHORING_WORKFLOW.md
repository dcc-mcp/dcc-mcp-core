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

## 3. Keep Runtime Scripts Host-Safe

Scripts should lazy-import host APIs inside the callable function. This keeps
catalog discovery, validation, and server startup available without a running
host process.

When the full `dcc-mcp-core` wheel is available, Python scripts should import
standard result helpers, dependency-light JSON/YAML codecs, bounded HTTP,
file/path safety, hashing, compression, validation, normalization, and
cancellation helpers from `dcc_mcp_core.skills_helper`. Keep `requests`,
PyYAML, or domain-specific dependencies only for behavior that namespace does
not cover, such as sessions, streaming, multipart upload, custom auth/retry
flows, YAML comment preservation, or host SDKs.

Use host-thread affinity only where needed:

- `affinity: main` for host API calls and scene mutations.
- `affinity: any` for pure filesystem, math, parsing, or metadata work.

## 4. Validate Before Loading

Run the creator validation tool or `dcc_mcp_core.validate_skill()` before adding
the skill to an adapter's default path. Treat validation warnings as design
feedback, not only syntax feedback.
