# Generic MCP clients from the terminal

Use `dcc-mcp-cli mcp` as a thin adapter to an explicit existing mcpc client;
keep its current DCC control, marketplace and gateway lifecycle commands. Both connect
to the same Core gateway, so resources, prompt definitions and tool schemas
remain owned by Core and its backends. Do not add a second MCP server or a
parallel REST contract for each terminal command.

## Selected companion and boundaries

`@apify/mcpc` is the preferred companion for resources and parameterized prompts.
The interoperability fixture pins 0.7.0 (Apache-2.0, Node >=22.12) and uses a
private `MCPC_HOME_DIR`; it does not install globally or copy credentials.
The native adapter forwards the companion command rather than duplicating MCP
methods, JSON-RPC types, session negotiation or SSE parsing. Resources, prompts
and tools operations share that same calling base. It first consumes the
session overview and rejects a missing advertised capability before forwarding
the operation: bare mcpc 0.7.0 otherwise returns `[]` successfully for
`prompts-list` on a server that does not advertise prompts. This is an observed
upstream limitation, not an empty, supported inventory.

Its bridge retains a task-owned session between terminal invocations. Close
that session after the workflow. A future production install still requires
the normal explicit installer authorization; the test does not deploy a client.

MCPorter 0.14.2 (MIT, Node >=24) is a useful tools/resources comparison and can
generate focused tool CLIs. Its documented CLI does not supply the same prompts
surface, and `serve`/`generate-cli` is not full-protocol acceptance. Do not build
another gateway around it just to expose Core's existing endpoint.

Official references: [mcpc](https://github.com/apify/mcpc),
[MCPorter](https://mcporter.sh/cli-reference.html).
These versions are test pins, not an assertion that every current/future MCP
feature works. The fixture covers legacy 2025-03-26 initialization and actual
Core resource/prompt returns. It does not certify 2026-07-28 discovery,
subscriptions, notification delivery, sampling, elicitation, MCP Apps or tasks.
A single request followed by process exit cannot prove a retained subscription.

The repository locks rmcp 1.7.0. Its existing SDK client can perform typed
operations and custom requests; an in-process Rust wrapper remains an option
if single-file native distribution becomes a requirement. It is not needed for
this companion-client workflow. Do not extend the current one-shot HTTP helper
into a second session/SSE implementation or automatically upgrade the SDK.

## Explicit session workflow

Select a reviewed endpoint; do not run bare `connect` to import all user MCP
configurations. An approved task-local client can be invoked by its absolute
`bin/mcpc` path using Node; an already installed `mcpc` has the same syntax.

```powershell
$clientArgs = @('--output', 'json', 'mcp', '--client-entry', '<absolute mcpc bin/mcpc path>', '--state-dir', '<absolute task-owned-directory>')
dcc-mcp-cli @clientArgs -- connect http://127.0.0.1:<port>/mcp @dcc --no-profile --protocol-version 2025-03-26
dcc-mcp-cli @clientArgs -- @dcc
dcc-mcp-cli @clientArgs -- @dcc resources-list
dcc-mcp-cli @clientArgs -- @dcc resources-read gateway://docs/agent-workflows
dcc-mcp-cli @clientArgs -- @dcc prompts-list
dcc-mcp-cli @clientArgs -- @dcc prompts-get '<name copied from prompts-list>' '{"seed":"42"}'
dcc-mcp-cli @clientArgs -- close @dcc
```

`--node` may select an explicit Node executable. Global output/timeout flags
belong to `dcc-mcp-cli`; the forwarded operation follows `@session` immediately.
Companion output is consumed as JSON and rendered by the existing CLI writer.
Non-JSON output and nonzero client/protocol errors remain errors, without
REST/native fallback. This path does not auto-start a gateway or apply staged
CLI updates. It only accepts explicit connect/close or session operations;
it does not expose account/profile/global configuration management. On Windows
its small helper is stored only in the explicit state directory; existing
different helper content is not overwritten. The total timeout covers capability
preflight and the requested operation.

The endpoint and prompt arguments above are examples, not permission to start
a host or execute returned prompt messages. Copy the actual names, argument
schemas and server capabilities. JSON output preserves `contents` and `messages`;
consume the returned content, rather than treating command success as task
completion. Core prompt names are namespaced by instance. Do not run prompt
messages as commands automatically.

## The same production gate

Before production, consume initialize instructions and read the existing
`gateway://docs/agent-workflows` resource. Its `contents[].text` decodes to a
JSON payload with `production_reuse_contract` and Markdown `document`. Require
`dcc-mcp-production-reuse/v1` plus the substantive Production reuse gate section.
The public Skill's existing `check_production_guide.py` can check the decoded
payload shape; the agent must still read and execute the rules.

A successfully read URI on an older gateway can return an older document.
Use the verified canonical Core source if it contains v1. If neither source
contains v1, report the rollout dependency and stop dependent production;
planning/read-only discovery may continue. The guide's rollout dependency on
Core #2745 still applies. Showing instructions is not evidence an agent obeyed.

Core currently does not implement `resources/templates/list`; expect a protocol
error, not an invented empty list. Likewise, a server without prompts or an
unknown prompt must remain an error. Do not silently retry a different protocol
or a REST/native production action after an uncertain write call.

## Reproduce the isolated interoperability check

The opt-in Rust test mounts Core's real HTTP handlers on a random loopback port,
registers only a canned prompt backend in a temporary registry, and invokes
actual external client processes. It never starts a real DCC or gateway daemon.
The Node harness uses explicit endpoints/config, `imports: []` for MCPorter,
anonymous mcpc connection, task-owned state and explicit session close.
On Windows a task-owned preload hides spawned helper windows; it does not change
protocol traffic or the installed package.

Install the fixed test packages into a task-owned directory after authorization:

```text
npm install --prefix <clients-directory> --ignore-scripts --no-audit --no-fund --save-exact @apify/mcpc@0.7.0 mcporter@0.14.2
```

Set `DCC_MCP_INTEROP_CLIENT_DIR` to that directory and
`DCC_MCP_INTEROP_OUTPUT_DIR` to a task-owned evidence directory. Build the
candidate CLI and set `DCC_MCP_INTEROP_CLI` to its absolute executable path,
then run:

```text
cargo build -p dcc-mcp-cli
cargo test -p dcc-mcp-gateway external_mcp_clients_consume_core_protocol -- --ignored --nocapture
```

`commands.json` retains actual commands, exit status, stdout and stderr;
`summary.json` is written only after assertions pass. npm's task-local lockfile
records package integrity. The test checks initialization instructions, the
versioned guide in structured/raw output, actual parameterized prompt messages,
and unsupported protocol behavior. No discovered capability is assumed callable
without its returned evidence.
