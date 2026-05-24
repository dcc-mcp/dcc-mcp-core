# Skill package maintenance (DCC adapters + bundled core skills)

This guide is the **single maintenance contract** for SKILL packages in
dcc-mcp-core and downstream adapters (e.g. dcc-mcp-maya). In-repo **reference
implementations** live under:

- `python/dcc_mcp_core/skills/dcc-diagnostics/` — infrastructure skill: rich
  frontmatter `description`, `search-hint`, `layer`, tool purposes spelled out
  in SKILL.md body.
- `python/dcc_mcp_core/skills/media/` — infrastructure skill: typed
  vx-managed FFmpeg/FFprobe wrappers for DCC render/playblast artifacts without
  exposing arbitrary shell or vx execution.
- `python/dcc_mcp_core/skills/workflow/` — orchestration skill: example JSON
  chains and explicit “when not to use” boundaries.

Use those two trees when authoring or reviewing any new skill.

## Ownership before implementation

Before adding or changing bundled adapter skills, read [`docs/POLICY_SKILL_OWNERSHIP.md`](../POLICY_SKILL_OWNERSHIP.md) and the relevant adapter's `SKILL_OWNERSHIP.yml` if it exists.

- Common file operations (`open`, `save`, `import`, `export`, `read_file`, `write_file`, path probes) must have one primary owning skill package for each adapter.
- Do not copy a file-operation tool into a second skill just to improve discoverability; add aliases, search hints, recipes, or `next-tools` pointing to the primary owner instead.
- If a duplicate is unavoidable, record the rationale and owner in `SKILL_OWNERSHIP.yml` in the same PR.

## Frontmatter (SKILL.md)

- Keep `description` as the **primary agent-facing contract** (MCP
  `get_skill_info` / search summaries do not ship the Markdown body).
- Under `metadata.dcc-mcp`: always set `tools`, `layer`, `dcc`, `version`,
  `search-hint`, `tags` as required by your adapter policy.
- Optional but recommended for long-form notes:
  - `recipes:` — anchor-based snippets (`recipes__*` tools when the host
    registers them).
  - `skill-reference-docs:` — **glob list** relative to the skill root so
    `skill_refs__list` / `skill_refs__read` can serve arbitrary Markdown/text
    under `references/` (or other dirs) without hard-coding one filename.
- Legacy `introspection:` single path is still honoured by
  `skill-reference-docs` resolution; prefer `skill-reference-docs` for new
  packages.

## tools.yaml

- Every tool **must** declare `execution`, `affinity`, and realistic
  `timeout_hint_secs` when `execution: async`.
- **Import / export / save / paths**: descriptions should state **absolute vs
  workspace-relative paths**, required plugins, and common failure follow-ups
  (e.g. parent directory must exist, use `file_exists`, save scene before
  export). Short descriptions make gateway `describe_tool` useless — aim for
  enough text that an agent can succeed without guessing.

## Python (or other) scripts

- Validate inputs early; return `skill_error` / `ToolResult`-style envelopes
  with `possible_solutions` instead of letting Maya open modal dialogs.
- For writes: ensure parent directory exists or return a structured error.
- After export: verify output file exists and non-zero size when feasible.

## Linting (dcc-mcp-maya)

Run from the Maya adapter repo:

```bash
python tools/lint_skills.py
```

Rules include IO description length hints and `references/` metadata coverage.
Extend `tools/lint_skills.py` when you add new cross-cutting conventions.

## Gateway-facing agents

- Prefer gateway MCP `search` → `describe` followed by REST `/v1/call`
  (or per-host `load_skill` then typed tools). Long prose belongs in `recipes` /
  `skill-reference-docs`, not only in SKILL.md body below the frontmatter.
