# DCC-MCP CLI Cheatsheet

Use the canonical route first: `search` → follow `next_step` → `describe`
only when requested → `call`. Keep `request_id` for failures.

```bash
dcc-mcp-cli search --query "create sphere" --dcc-type maya
dcc-mcp-cli describe <tool_slug>
dcc-mcp-cli call <tool_slug> --json '{"radius": 2.0}'
dcc-mcp-cli call --batch --steps '<steps-json>'
```

## Reuse and iteration

`materialize_script` and `workflows_*` are MCP/REST tools. Discover them
with `search`; the CLI `call` wrapper passes their arguments unchanged.
`file_path` values are host-local paths returned by the materializer.

```text
# 1. Materialize once; record file_path, sha256, reused.
tool: materialize_script
arguments: {"content": "<script-source>", "reuse": true, "reuse_key": "<stable-key>"}

# 2. Execute the same file_path. Never resend unchanged code.
tool: <dcc>-scripting__execute_python
arguments: {"file_path": "<returned-path>", "params": {"radius": 2.0}}

# 3. Iterate with params only.
tool: <dcc>-scripting__execute_python
arguments: {"file_path": "<same-returned-path>", "params": {"radius": 4.0}}

# 4. Promote a repeated multi-step sequence.
tool: workflows_run
arguments: {"spec": {"...": "... with idempotency_key per mutating step"}}

tool: workflows_get_status
arguments: {"workflow_id": "<workflow-id>"}

tool: workflows_resume
arguments: {"workflow_id": "<workflow-id>", "force_steps": [], "strict": true}
```

`--json-file` is acceptable for a large one-off argument object, but it does
not create a reusable script identity. A byte-identical repeat must use
`materialize_script(reuse=true, reuse_key=...)` and the returned `file_path`.
Params-only reruns have no CLI flag and no params file.