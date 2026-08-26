# CLI cheatsheet — DCC-MCP gateway

Default profile: `local`. Remote gateways are selected with
`dcc-mcp-cli gateway set <name>` or one-off `--gateway <name>`.
Local calls use direct MCP for compatibility unless `--require-gateway` (or
`DCC_MCP_CLI_REQUIRE_GATEWAY=true`) is set. Add a stable
`--agent-session-id <task-id>` whenever Gateway stats are required evidence.

Primary tool: `dcc-mcp-cli` — the CLI is the **default path for every
shell-capable AI agent**. Native MCP is the fallback for MCP-only clients or an
explicit user choice.

## CLI setup

If `dcc-mcp-cli` is missing, obtain user consent before installing the latest
official release. From the installed `dcc-mcp` Skill directory, run the bundled
verified helper:

```bash
python scripts/check_cli.py --ensure-cli --pretty
```

The helper accepts only the official `dcc-mcp/dcc-mcp-core` release manifest,
validates its version and asset URL, and verifies the downloaded binary's
SHA-256 before an atomic replacement. It fails closed and preserves an existing
CLI when the manifest, URL, download, or digest is invalid. SHA-256 provides
release-manifest integrity checking; it is not a publisher signature.

Keep an official build current through the release manifest:

```bash
dcc-mcp-cli update check
dcc-mcp-cli update apply
dcc-mcp-cli components status dcc-cua
dcc-mcp-cli components ensure dcc-cua --yes
```

`update apply` requires the available entry's 64-hex SHA-256, verifies the
streamed download, and stages one component bound to the exact CLI installation.
The next launch re-verifies the bytes before replacement and restarts with the
original arguments. Legacy unsigned staging is quarantined. It does not update
a running `dcc-mcp-server`; update that server in its own environment. Gateway
Admin remains check-only for every binary.
The official CLI installer also reconciles the independently released
`dcc-cua` sibling. `components status` is read-only; `components ensure`
requires explicit `--yes`, a mandatory archive SHA-256, and an exact official
release-manifest binding.

For repository development only, the same consent-gated verified
bootstrap/fallback is:

```bash
vx python scripts/dcc_gateway.py --ensure-cli list
```

## Discovery and health

| Command | Purpose |
|---------|---------|
| `dcc-mcp-cli dcc-types` | List adapter-backed DCC identifiers from the bundled release catalog without starting a gateway |
| `dcc-mcp-cli dcc-types --catalog path/to/catalog.yml` | Inspect a studio or test catalog through the same typed contract |
| `dcc-mcp-cli list` | Ensure the local loopback gateway, then list local DCC instances from the FileRegistry |
| `dcc-mcp-cli doctor` | Report profile, registry, local inventory, direct-control readiness counts, gateway daemon status, and server binary diagnostics without launching services |
| `dcc-mcp-cli search --query "create sphere" --dcc-type maya --limit 20` | Search local instances directly through MCP in the `local` profile; this form remains compatible with released CLI builds |
| `dcc-mcp-cli search --require-gateway --query "create sphere" --dcc-type maya --limit 20` | Fail closed unless the local gateway serves the control request; use this route for measured workflows |
| `dcc-mcp-cli list --gateway pcA` | List DCC instances through a named remote gateway profile |
| `dcc-mcp-cli health` (or `python scripts/dcc_gateway.py health`) | Check gateway liveness; CLI auto-starts loopback gateway targets |
| `dcc-mcp-cli gateway register https://host:19293 --name pcA` | Persist a named remote gateway profile |
| `dcc-mcp-cli gateway list` | Inspect configured remote profiles and the active selection |
| `dcc-mcp-cli gateway set pcA` / `dcc-mcp-cli gateway set local` | Switch active gateway profile |
| `dcc-mcp-cli gateway daemon start` | Start the explicit local machine-wide daemon; default idle timeout is `0`, so it stays alive with no DCC backend |
| `dcc-mcp-cli gateway daemon restart` | Stop the pidfile-tracked daemon, then start it again with the same persistent default |
| `dcc-mcp-cli gateway daemon stop` | Stop the pidfile-tracked local daemon |
| `dcc-mcp-cli gateway daemon status` | Explicit local daemon lifecycle check with registry dir, PID file, health URL, and CLI version |
| `dcc-mcp-cli list --pretty` (or `python scripts/dcc_gateway.py --pretty list`) | Human-readable JSON |

## Capability workflow

| Command | Purpose |
|---------|---------|
| `dcc-mcp-cli search --query "create sphere" --dcc-type maya --limit 20` | Find tools with a natural-language phrase |
| `dcc-mcp-cli describe <slug>` | Inspect schema |
| `dcc-mcp-cli call <slug> --require-gateway --agent-session-id task-42 --json '{"radius":2}'` | Invoke one tool through the measured gateway route with a stable task-scoped stats identifier |
| `dcc-mcp-cli call <slug> --require-gateway --agent-session-id task-42 --json '{"radius":2}' --meta-json '{"lease_owner":"workflow-42"}'` | Invoke a measured tool call on an instance leased by this workflow |
| `dcc-mcp-cli call <slug> --require-gateway --wait --wait-timeout-secs 600 --json '{}'` | Wait inside the CLI for an asynchronous job instead of spending agent calls on status polling |

`dcc-types` reports the release catalog, not running instances. Entries include
their canonical `dcc_type`, adapters, version/source data when available, and
`catalog_install_available`. Unknown/custom DCC identifiers remain valid at the
core boundary even when no catalog install plan exists.

### Job strategy and recovery

Read `metadata.dcc.jobStrategy` from `search` or `describe`; do not infer it
from prompt length:

- absent or `monolithic` is one indivisible host call and is suitable only for
  expected-short work.
- `chunked` advances bounded adapter-authored steps on host event-loop ticks.
  Call it once, preserve the returned core `job_id`, and poll status.
- `isolated` returns an adapter operation ID with a typed status tool. Use it
  for long native work that must remain queryable after adapter restart.

Never split arbitrary Python or native DCC code. Automatic selection means
choosing among tools whose authors declared these contracts.

A transport timeout does not prove cancellation or failure. Preserve every
known operation ID, do not replay the mutation, and re-run `list` with bounded
backoff. A live-owned `unreachable` registry row is recoverable and must not be
treated as deletion. When the same instance becomes ready, query the core job.
For an `isolated` operation, rediscover its typed status tool and query the
operation ID even after adapter restart.

`call --wait` keeps polling the same job across transient gateway connection,
404, 429, 502, 503, and 504 failures until the total wait timeout. It emits
`control_plane_reconnecting` and adds `wait_recovery` after recovery; neither
path resubmits work. A 410 lifecycle response becomes
`tracking_status=owner_exited`. Read `previous_status`, `retryable`, and
`recommended_next_action` instead of treating every missing route as 503.

Async responses identify the wrapper as `core_job_id` with
`job_id_owner=core`; legacy `job_id` is the same Core ID. A direct or terminal
Core call may expose a second `adapter_job_id`. `call --wait` resolves the Core
wrapper and automatically follows `adapter_job.poll` on the same instance when
the declared status tool is synchronous, read-only, idempotent, and declares a
string `job_id` as its only required input. Every other input must be optional
and safe when omitted. Never pass an adapter ID to `jobs_get_status`, and do not assume
Core parent cancellation propagates to it. Missing/unsafe poll metadata,
non-canonical status, or a different returned ID fails closed without replay;
inspect `wait` and `tracking_status` instead of treating the launch as terminal.

If owner death or remote TTL expiry removes the row, wait for an explicitly
authorized DCC restart, then use the replacement instance and fresh
`search`/`describe` results. Old instance IDs, slugs, direct URLs, and core jobs
must not be reused; active core jobs become `interrupted`. Never silently replay
a non-idempotent mutation.

## Post-task evidence

After acceptance, query only the task scope:

```bash
dcc-mcp-cli stats --range 24h --dcc-type maya --session-id task-42
```

Read `stats_coverage` before the aggregate. Gateway stats exclude
`local_mcp_direct`; `configured_route_recorded=false` means the configured
single-call route was not measurable. A `total_calls` value of `0` means there is no
telemetry evidence, not that no calls occurred. Feed the JSON
plus bounded task and validation summaries to the `review_skill_improvement`
prompt in `dcc-mcp-skills-creator`; do not include raw prompts, secrets, private
paths, or full tool payloads.

## Failure findings and public-safe bundles

```bash
dcc-mcp-cli feedback route finding.json --json
dcc-mcp-cli feedback bundle reviewed-finding.json --json
dcc-mcp-cli feedback bundle reviewed-finding.json \
  --install-report install-report.json --json
dcc-mcp-cli feedback bundle reviewed-finding.json \
  --dcc-pid 4321 --log-dir /safe/log/root --host-error-lines 50 --json
dcc-mcp-cli feedback file reviewed-finding.json --json
# Only after reviewing the plan and receiving explicit user authorization,
# execute the returned next_step.argv exactly.
```

`route` resolves exact ownership without a Gateway. `bundle` also avoids
Gateway auto-start, but uses an already reachable endpoint for the stable
public-safe issue report when the Finding has a request id. Run it only after
human review has set `redaction_status.mode=public-safe` and every exclusion
flag to true. The host-error source is one exact regular file, capped at 256
KiB and at most 200 requested records; raw messages, tracebacks, metadata,
paths, tokens, and PID are excluded. When `install --execute --json` produced a
terminal report, save that single stdout object and pass it with
`--install-report`. The bundle accepts only a regular non-symlink file up to
256 KiB whose DCC/core/adapter identity matches the Finding, then projects only
public-safe Install SOP v1 fields. Invalid or mismatched input fails closed.
The raw report is validated against the published Draft 2020-12 schema first,
including mutually exclusive `command`/`file_edit` next steps. Public output
redacts sensitive option/value pairs, relative and absolute paths, and all URL
schemes; it omits `file_edit.content` and the input report path.
Treat `unavailable` components and `complete=false` as incomplete evidence.
Keep raw issue reports and host logs local, and never infer permission to create
or attach to an external issue. A
failed `DccServerBase.start()` is available through `feedback list`/`export` as
a `needs-review` startup Finding without a request id; review and redact its
exception-derived observed text before using `route`, `bundle`, or `file`.
`file` is read-only unless `--yes` is paired with exactly one explicit decision
and the complete authorization binding from the plan. It searches the routed
repository by fingerprint before title keywords. The replay binds the canonical
Finding path, canonical catalog path or exact bundled-catalog sentinel, Finding
content SHA-256, fingerprint, repository, and catalog SHA-256, then rechecks
that binding and the exact match immediately before a write. Every `gh` call is
pinned to `github.com`, limited to 30 seconds, and started in an owned process
tree; a timeout terminates and reaps the full tree under bounded pipe cleanup.
Issue/comment bodies above 65,536 Unicode scalar values are rejected before
tracker I/O. Exact conflicts, drift, closed issues, tracker errors, and
ambiguous candidates stop the command. Never auto-select keyword-only results,
reconstruct the replay argv, or add `--yes` without user authorization.

## Install and marketplace

| Command | Purpose |
|---------|---------|
| `dcc-mcp-cli install --dcc-type maya` | Build an auditable adapter install plan with a catalog-pinned wheel URL, version, SHA-256, and machine-readable `next_steps` |
| `dcc-mcp-cli install --dcc-type maya --python "<mayapy>" --execute` | Execute the verified wheel after consent; roll back on failure and verify the installed package version |
| `dcc-mcp-cli install --dcc-type maya --dcc-path "<maya-executable>"` | Supply a non-standard DCC executable/application path when the host is not found automatically |
| `dcc-mcp-cli marketplace search --query "maya rigging" --limit 20` | Find installable Skill packages with released and current CLI builds |
| `dcc-mcp-cli marketplace inspect <package_name>` | Inspect the selected skill package metadata before installing |
| `dcc-mcp-cli marketplace install <package_name> --dcc maya --reload` | Install an exact package ID and ask running Maya adapters to re-scan skill paths |
| `dcc-mcp-cli marketplace install <package_name> --target game:the-bazaar` | Install a typed CUA Profile for a generic application target; no DCC reload is requested |
| `dcc-mcp-cli reload-skills --dcc-type maya` | Ask running Maya adapters to re-scan installed skill paths |
| `dcc-mcp-cli marketplace update <package_name> --dcc maya` | Update an installed skill package from the catalog |
| `dcc-mcp-cli marketplace add-repo <repo> --commit <40-hex-oid> --dcc maya` | Install a direct repository source only at the reviewed immutable commit; `--list` may omit the commit |
| `dcc-mcp-cli marketplace uninstall <package_name> --reload` | Remove an installed skill package; infer its DCC when it is installed for one DCC and refresh the adapter |

After adapter package install, follow the plan's `next_steps`: read the
adapter-maintained `install.md` when `read-install-instructions` is present,
start or enable the DCC host plugin, run `doctor`, and confirm the sidecar
self-registered with `dcc-mcp-cli list`.
If `install_policy.auto_install_enabled=false`, stop and show
`install_policy.prompt`; the studio pipeline owns adapter deployment.
`list` keeps live diagnostic rows visible; `search`, `describe`, `load-skill`,
`call`, and `reload-skills` only route to rows ready for local CLI control. A
per-DCC sidecar row is routable once `direct_control.ready=true`; if a row is
booting or `dispatch_status=unavailable`, inspect
`direct_control.diagnostics.failure_stage`, `failure_reason`, `host_rpc_*`, and
any log paths, then run `wait-ready` or `doctor` before calling tools.
Marketplace search and inspect do not require a live DCC instance. Always query
the CLI before recommending a marketplace Skill. If the first query is empty,
retry once with fewer capability words or without the DCC filter; never invent
a package name. Inspect unfamiliar packages before a consent-gated mutation;
an exact known ID can be installed directly, and `--dcc` can be omitted for a
single-DCC package. Prefer install `--reload`; after updates or installs without
that flag, run `reload-skills`, then use `load-skill` if needed.

## Example: inventory

```bash
# CLI (primary)
dcc-mcp-cli list
dcc-mcp-cli health

# Python fallback (when CLI is unavailable)
python scripts/dcc_gateway.py health
python scripts/dcc_gateway.py list
```

## Example: search

```bash
# CLI (primary)
dcc-mcp-cli search --query "create sphere" --dcc-type maya --limit 10

# Python fallback
python scripts/dcc_gateway.py search --query sphere --dcc-type maya --limit 10
```

## Example: describe

```bash
# CLI (primary)
dcc-mcp-cli describe maya.a1b2c3d4.maya_primitives__create_sphere

# Python fallback
python scripts/dcc_gateway.py describe maya.a1b2c3d4.maya_primitives__create_sphere
```

## Example: call

```bash
# CLI (primary)
dcc-mcp-cli call maya.a1b2c3d4.maya_primitives__create_sphere \
  --require-gateway \
  --agent-session-id task-42 \
  --json '{"radius":2.0}'

# Python fallback
python scripts/dcc_gateway.py call maya.a1b2c3d4.maya_primitives__create_sphere \
  --json '{"radius":2.0}'
```

## Slug rules

- Slugs are returned by `search`; local and remote modes use the same
  `dcc.instance.tool` shape.
- Do not invent slugs from DCC names or tool names.
- Re-run `list` and `search` after a DCC restart.

## Common errors

| Symptom | Action |
|---------|--------|
| CLI not found | Ask user permission, then run `vx python scripts/dcc_gateway.py --ensure-cli list`; it verifies the official manifest and SHA-256 before install, and Python fallback runs if verification or download fails |
| Gateway health fails | Run `dcc-mcp-cli doctor` and inspect the CLI JSON/stderr. Agent-control and endpoint/admin/update commands auto-ensure only loopback gateway targets. For remote profiles or `--base-url`, auto-start is not possible. Ask before installing adapters or launching GUI DCC apps |
| `gateway_stats_recorded=false` | The call used compatibility direct MCP. If stats are required, repeat the workflow with `--require-gateway --agent-session-id <task-id>`; do not infer from the partial stats count |
| `--agent-session-id` conflicts with `--meta-json` | Keep one exact task ID. `--agent-session-id` owns `_meta.agent_context.session_id`; UI Control's argument-level `session_id` is a different scoped UI session |
| `total == 0` | Start a DCC adapter, then re-run `dcc-mcp-cli list` |
| Listed row is booting or `dispatch_status=unavailable` | Read `direct_control.recommended_next_action` and `direct_control.diagnostics`, then run `dcc-mcp-cli wait-ready --dcc-type <dcc> --instance-id <id>` or `dcc-mcp-cli doctor`; do not call tools until `direct_control.ready=true` |
| `unknown-slug` | Re-run `search`; the instance may have restarted |
| `instance-offline` | Read `previous_status`: `never-registered` is 404, temporary unroutability is 503, and `exited` / `host-died` / `heartbeat-timeout` is 410. Preserve any job ID and never replay a mutation blindly |
| `invalid-params` | Fix the JSON object per `describe` output |
| `instance-leased` / `lease-owner-mismatch` | Pass the exact workflow owner with `--meta-json`, or select another instance; do not guess another owner's value |
