---
name: dcc-mcp
description: >-
  Public agent entry point for CLI+REST control of live DCC hosts. Use when
  discovering tools, loading skills, calling typed DCC tools, materializing
  scripts, or repeating multi-step DCC work. Prefer typed tools, then reuse
  file-backed scripts and params-only iteration instead of re-sending code.
license: MIT-0
metadata:
  dcc-mcp:
    dcc: python
    layer: infrastructure
    version: "1.0.0"
    search-hint: "dcc mcp cli rest gateway script reuse materialize params workflow resume capabilities"
    tags: "dcc-mcp, agent, cli, rest, materialize, workflow"
---

# DCC-MCP Agent Entry Point

Start with one narrow `search` / `describe` / `call` loop through
`dcc-mcp-cli` or gateway `POST /v1/*`. Follow the hit's `next_step` and pass
correlated load arguments unchanged. Do not guess tool slugs or send raw DCC
script code when a typed tool exists.

## Iteration Playbook — second and subsequent runs

When a task runs again, use the correct rung of this ladder:

1. **Materialize once.** If no typed tool fits, write the script to the DCC
   host once with `materialize_script(content=..., reuse=True,
   reuse_key="<stable-key>")`. Store the returned `file_path`, `sha256`,
   and `reused` value. A byte-identical repeat with the same key returns the
   same path with `reused=true`; a changed byte intentionally produces a new
   script identity. Verify the descriptor before execution.
2. **Execute by `file_path`.** Pass the host-local `file_path` (or the
   execution tool's supported `script_path` alias) to the execution tool.
   Never resend unchanged code, and never treat inline `code` as the repeat
   path.
3. **Iterate with `params` only.** Author generated scripts as
   `def main(**params)` with typed values, then repeat by sending the same
   `file_path` and a new `params` object. `FileBackedScriptExecutionParams`
   validates `params` against `parameters_schema` from
   `derive_script_parameters_schema`, which parses the module without
   importing it. There is no CLI flag and no params file; do not invent one.
   A CLI `--json-file` payload that carries unchanged source is not reuse.
4. **Graduate repeatable multi-step work.** After the same sequence succeeds
   twice, promote it to a workflow spec with `workflows_run`, giving mutating
   steps stable `idempotency_key` values. Poll with `workflows_get_status`.
   After an interruption, use `workflows_resume` with `workflow_id` and
   optional `force_steps`, `expected_spec_hash`, `strict`; it requires
   `WorkflowStorage` plus the `job-persist-sqlite` feature. Do not replay a
   completed run from step one.

Normative contract:
`docs/guide/agents-reference.md#iteration-playbook`. CLI command shapes:
`skills/dcc-mcp/references/CLI_CHEATSHEET.md`.

## Critical rules

| Rule | Action |
|------|--------|
| Typed tool first | Search once, follow `next_step`, then call the typed tool. |
| Materialize once | Reuse `file_path` plus `reuse_key`; never resend unchanged source. |
| Params are the iteration channel | Change `params`, not the script body. |
| Persist repeated sequences | Use `workflows_run` / `workflows_resume` for multi-step work. |
| Preserve evidence | Record `file_path`, `sha256`, `reused`, `request_id`, `workflow_id`. |

