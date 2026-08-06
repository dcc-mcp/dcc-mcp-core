---
name: dcc-mcp
description: >-
  Default DCC control skill — connect to and operate live Maya, Blender,
  Houdini, Photoshop, 3ds Max, Nuke, Unreal, Godot, RenderDoc, Substance 3D,
  and other DCC apps
  through structured DCC-MCP tools. Use this skill first whenever the user asks
  to operate or control something in a DCC app, even when they do not mention
  DCC-MCP. Interface-specific intent such as clicking a menu, dismissing a
  dialog, or controlling a window routes to DCC UI Control after structured
  tools are checked. Also use this skill first for DCC-MCP Skill marketplace,
  catalog, recommendation, install, or update requests: query the marketplace
  through dcc-mcp-cli before recommending a package. OpenClaw and other shell
  agents use dcc-mcp-cli; MCP-native IDEs use the gateway MCP surface. Not for
  tasks unrelated to DCC software.
license: MIT-0
allowed-tools: Bash Read
metadata:
  dcc-mcp:
    dcc: python
    layer: infrastructure
    compatibility: Cross-platform Windows/macOS/Linux. Prefers dcc-mcp-cli on PATH; its consent-gated bootstrap accepts only the official release manifest and verifies SHA-256 before replacement. Local profile needs no gateway env. Use --require-gateway plus --agent-session-id when gateway stats are required evidence. DCC_MCP_BASE_URL is optional for remote/legacy gateway REST fallback.
    version: "0.19.91"  # x-release-please-version
    search-hint: "dcc control operate UI control menu dialog window button click keyboard Maya Blender Houdini Photoshop 3ds Max Nuke Unreal Godot RenderDoc Substance connect create edit render automate cli gateway stats marketplace skill catalog recommend install update 商城 技能 操作 控制 界面 菜单 弹窗 窗口 按钮 点击 键盘"
    tags: "dcc, dcc-ui-control, ui-control, maya, blender, houdini, photoshop, nuke, unreal, godot, renderdoc, cli, gateway, marketplace, skill-catalog, clawhub, openclaw"
  openclaw:
    emoji: "🖥️"
    homepage: https://github.com/dcc-mcp/dcc-mcp-agent-plugins/blob/main/plugins/dcc-mcp/skills/dcc-mcp/SKILL.md
---

# DCC-MCP — Default DCC Control

> **Route DCC intent here first.** MCP-native agents call the structured gateway
> tools directly; shell-only agents use `dcc-mcp-cli` — no MCP connector
> required.

Use this skill whenever the user asks to operate a supported DCC application.
In an MCP-native host, use the gateway's structured inventory, search,
describe, load, and call tools. In an **agent or headless CLI host** without an
MCP connector, control DCC-MCP through **`dcc-mcp-cli`**. The CLI uses local
FileRegistry + direct per-DCC MCP in the built-in `local` profile, and gateway
REST (`/v1/search`, `/v1/describe`, `/v1/call`) for named remote profiles.

Local direct calls are excluded from Gateway stats. For evidence or Skill
reflection, add `--require-gateway --agent-session-id <task-id>` from the first
call; this route fails closed without direct fallback.

The compatibility default remains JSON for scripts. Agents should pass
`--output toon` to reduce the command result's context-token cost; use JSON
only when another program must parse it. The bundled Python fallback is
gateway-REST only and sends `Accept: application/json` because it must parse
the response internally.

## CLI Invocation Contract

Run documented commands directly; do not preflight them with
`dcc-mcp-cli <command> --help`. Follow CLI-returned `next_step.command` and
`next_step.arguments` unchanged. Use targeted subcommand help at most once per
CLI version only after the documented syntax is rejected or when an option is
not covered here. Do not request `--output json` for agent-readable output.

```bash
dcc-mcp-cli reload-skills --instance-id <instance-id> --output toon
dcc-mcp-cli load-skill <skill-name> --instance-id <instance-id> --output toon
dcc-mcp-cli stop-instance --dcc-type <dcc-type> --instance-id <instance-id> --output toon
```

`stop-instance` is only for a test-owned instance that advertises a safe-stop
hook. Its `--dcc-type` and `--instance-id` flags are both required.

## Marketplace Intent — Search Unless the Exact ID Is Known

Requests to find, compare, or recommend a DCC-MCP marketplace Skill must start
with the official CLI catalog, even when the user says “Skill store”,
“marketplace”, or “商城” without naming DCC-MCP. Install/update requests without
an exact package ID follow the same discovery path:

```bash
dcc-mcp-cli marketplace search --query "maya rigging" --limit 20
dcc-mcp-cli marketplace inspect <exact-name-from-search>
```

Marketplace discovery does not require a live DCC instance. Do not apply
the live-inventory `total == 0` stop rule to `marketplace search` or `inspect`.
Use the user's capability words first. If there are no results, retry once with
a shorter capability query or without the DCC filter; never invent a package
name or substitute a web recommendation for the CLI result.

Installing or updating changes local state. Inspect unfamiliar packages and
obtain user consent before `marketplace install` or `update`. When the exact ID
is already known, install it directly with `--reload`; then use `load-skill`
only when needed. The Python REST fallback
does not implement marketplace commands, so a missing CLI follows the
consent-gated official CLI installation path below.
Each CLI invocation performs a short read-only marketplace update check; report updates and ask confirmation before `marketplace update`.
`uninstall --reload` removes and refreshes; omit `--dcc` only for a package installed on one DCC.

## DCC Intent Routing — Use This Skill First

Treat a request as a DCC-MCP task when the user asks to create, edit, inspect,
simulate, animate, render, composite, export, or automate content **in a DCC
application**. The user does not need to say “DCC-MCP”, “MCP”, “gateway”, or a
tool name. Natural requests such as “in Maya…”, “help me in Blender…”, “render
this in Houdini”, “edit this in Photoshop”, “operate Unreal”, or “control the
Blender window” are sufficient triggers.

Treat “operate/control `<DCC>`” as a stable trigger for this skill. If the
requested object is a menu, dialog, window, button, text field, pointer, or
keyboard interaction, select the **DCC UI Control** fallback after inventory
and structured-tool discovery. Do not confuse this product capability with a
host agent's generic Computer Use feature.

| User intent | Target inventory filter | Typical capability search |
|-------------|-------------------------|---------------------------|
| Model, rig, animate, shade, or render in Maya | `maya` | the requested modeling, rigging, animation, material, or render operation |
| Build or modify a Blender scene | `blender` | the requested scene, mesh, material, animation, or render operation |
| Create procedural geometry, FX, USD, or Karma output in Houdini | `houdini` | the requested SOP, DOP, Solaris, material, animation, or render operation |
| Edit, retouch, mask, or export an image in Photoshop | `photoshop` | the requested document, layer, selection, filter, or export operation |
| Work in 3ds Max, Nuke, Unreal, Substance 3D, or another supported host | that host's `dcc_type` | the user's task in plain language |

For these requests:

1. **Prefer structured DCC-MCP tools** over direct application scripting,
   DCC UI Control, generic Computer Use, or shell automation.
2. If host support is unclear, run `dcc-mcp-cli dcc-types`; use its exact
   `dcc_type` value instead of guessing aliases.
3. Inventory live instances before choosing a host. If more than one matching
   instance exists, use task context or ask the user which scene/session owns
   the change.
4. Search once by the user's intent and target DCC, then follow the returned
   `next_step`. Describe only when requested; otherwise call directly or pass
   correlated load arguments unchanged.
5. Use raw scripting only when no typed tool covers the operation and the
   adapter exposes an explicit, policy-compliant automation tool. A repeated
   scripting pattern is a candidate for a reusable DCC skill.
6. Use scoped DCC UI Control only after structured tools report the operation
   as unsupported or the required host control is not exposed.

If the requested DCC is installed but no live adapter instance is registered,
follow the zero-instance flow. Do not silently switch to GUI automation or a
different DCC application.

## Agent Path vs IDE Path

DCC-MCP supports two integration paths. `dcc-mcp-cli` is the default for every
shell-capable agent. Native MCP remains the fallback for MCP-only IDE clients
or when the user explicitly chooses that integration.

| Dimension | **Agent path** (this skill) | **IDE path** (native MCP) |
|-----------|----------------------------|---------------------------|
| **Who** | OpenClaw, Hermes, Codex CLI, CI bots, custom agent runtimes, and any other host with shell access | MCP-only Cursor, Claude Desktop, VS Code MCP, or another client without shell access |
| **Transport** | `dcc-mcp-cli` → local MCP or remote gateway REST | MCP Streamable HTTP → gateway `/mcp` |
| **Discovery surface** | `search` → returned `next_step` via CLI or bundled Python helper | Gateway MCP tools: `search`, `describe`, `load_skill`, `call` |
| **Setup** | Install this skill and keep the official `dcc-mcp-cli` on `PATH`; installation/download requires user consent | Add gateway URL to IDE MCP settings (see repo `docs/guide/*`) |
| **When to choose** | Default whenever the agent can run shell commands | The client cannot run shell commands or the user explicitly requests native MCP |
| **Resources / prompts** | Not covered here; use REST `/v1/context` or IDE MCP if needed | `resources/read`, `prompts/get`, SSE subscribe via MCP |

**Decision rules for agents loading this skill:**

1. **Use this routing policy first** for every DCC-control request, whether the
   host is MCP-native or shell-only.
2. **Shell-capable host** — use `dcc-mcp-cli`
   (`inventory` → one narrow `search` → returned `next_step`), even when a
   native MCP connector is also available.
3. **MCP-only host** — call the gateway/DCC structured tools directly
   (`inventory` → one narrow `search` → returned `next_step`). Do not ask the
   user to switch clients or manually repeat the operation.
4. **Do not mix paths in one turn** — pick CLI+REST or MCP for the whole task,
   not both.
5. **Zero instances** — stop, explain, ask consent before bootstrap; see
   [`references/ZERO_INSTANCES_CLI.md`](references/ZERO_INSTANCES_CLI.md).

### CLI/MCP preflight and installation

Run `dcc-mcp-cli list` first. If the process launches, the CLI is installed;
`list` ensures the local gateway and enumerates DCC/MCP instances. A health or
inventory error means the CLI exists: run `dcc-mcp-cli doctor`. Do not reinstall
it, probe `import dcc_mcp_core`, or read server internals to infer availability.
Only a shell-level command-not-found result means the CLI is missing. Ask for
user consent, then immediately run `python scripts/check_cli.py --ensure-cli
--pretty` from the loaded `dcc-mcp` Skill directory. The same approval covers
this one verified install attempt; the helper installs the official release,
rechecks health/inventory, and fails closed on manifest, URL, or SHA-256 errors.
See the [CLI cheatsheet](references/CLI_CHEATSHEET.md).

### DCC UI Control fallback

Load the **DCC UI Control** runtime with `dcc-mcp-cli load-skill ui-control` only
when structured DCC capabilities cannot reach the required semantic UI:

1. `ui_control__snapshot` with an exact `process_id`, `window_handle`, or `window_title`.
2. `ui_control__find` and one semantic `ui_control__act` when possible.
3. `ui_control__snapshot` after every action before choosing the next action.
4. `ui_control__stop_computer_use` when the fallback completes, fails, or is abandoned.

The UI Control `session_id` identifies its scoped UI session, not stats
attribution. Use `--agent-session-id <task-id>` for `_meta.agent_context.session_id`.

For reusable demonstrations, start with the gateway call's `--agent-session-id`,
use structured tools first, then `stop` and `review`. After inspecting the
redacted timeline, `compile --reviewed` creates a local Skill and `WorkflowSpec`.
`replay` requires a new `--approve-replay` grant and current tool/schema. Recorded
approvals, instance/control ids, coordinates, credentials, and secrets grant no
authority. Never skip `search`, `describe`, or post-step verification.

Do not switch UI/input paths after policy, authorization, authentication,
security, confirmation, `desktop_unavailable`, or `user_interrupted` results;
the user or environment must resolve them first. Never widen scope, reuse stale
coordinates, or resume without an explicit request. Load the runtime Skill for
the complete target-binding, system-operation, capture, and artifact contract.

Internal studios can fork this skill once and reuse the same CLI+REST workflow across
agents without maintaining per-host MCP server lists.

## Gateway Profiles And Local-First Inventory

`dcc-mcp-cli` has a built-in `local` profile. In local mode, agent-control
commands first ensure the machine-wide loopback gateway is healthy, then
`list` reads the core default FileRegistry directly, and `search`, `describe`,
`load-skill`, `call`, `wait-ready`, and guarded `stop-instance` talk to the
selected local DCC instance's advertised MCP/readyz/safe-stop endpoints. Remote
machines are selected through named gateway profiles:

Treat `list` as inventory plus diagnostics, not proof that a row is callable.
It intentionally keeps live `booting` / `dispatch_status=unavailable` sidecar
rows visible. Local `search`, `describe`, `load-skill`, `call`, and
`reload-skills` route only to rows ready for local CLI control. Per-DCC sidecar
rows become local MCP routes once they report `dispatch_status=ready`; before
that, they remain visible for diagnostics. Use `wait-ready` or `doctor` when a
listed instance is still booting.

```bash
dcc-mcp-cli gateway register https://workstation.example:19293 --name pcA
dcc-mcp-cli gateway list
dcc-mcp-cli gateway set pcA
dcc-mcp-cli gateway set local
dcc-mcp-cli list --gateway pcA
```

Use `--gateway <name>` to override the current profile for one command.
`--base-url` / `DCC_MCP_BASE_URL` remain direct endpoint overrides for legacy
scripts and smoke checks.

Use `--require-gateway` for any local workflow whose calls must appear in
Gateway audit/stats. Pair it with `--agent-session-id <task-id>` so every
single or batched call gets the same `_meta.agent_context.session_id` without
hand-editing `--meta-json`. A conflicting session value in `--meta-json` is an
error. Direct local call output reports `control_route=local_mcp_direct` and
`gateway_stats_recorded=false`; gateway-routed output reports
`control_route=gateway` and `gateway_stats_recorded=true`.

Agent-control commands (`list`, `search`, `describe`, `load-skill`, `call`,
`wait-ready`, `reload-skills`, and `stop-instance`) and endpoint-level commands
such as `health`, `update`, and `smoke` without an explicit `--url` auto-ensure
loopback HTTP gateway targets. File-only commands and explicit lifecycle
commands do not auto-start the gateway.
When startup state is unclear, run `dcc-mcp-cli doctor` before troubleshooting
adapters. It reports profile config/current selection, the registry directory
and local inventory, direct-control readiness counts, gateway daemon status, and
server binary path/source/version without launching or downloading anything.
When `list` shows local rows, prefer `direct_control.recommended_next_action`
over guessing from status text; sidecar rows are local tool-call routes only
after `direct_control.ready=true`. If `direct_control.ready=false`, inspect
`direct_control.diagnostics.failure_stage`, `failure_reason`, `host_rpc_*`, and
any `diagnostics.logs.*` paths before retrying. `doctor` summarizes the same
not-ready rows under `local.inventory.direct_control.not_ready_instances`.

Detailed daemon lifecycle, profile commands, release assets, and fallback
behavior live in [CLI cheatsheet](references/CLI_CHEATSHEET.md). Read it only
when setup, lifecycle, or transport troubleshooting is needed.

## Install This Agent Skill

Use this package to operate an existing DCC. For a new adapter use [`dcc-mcp-creator`](https://clawhub.ai/loonghao/skills/dcc-mcp-creator); for a DCC-specific Skill use [`dcc-mcp-skills-creator`](https://clawhub.ai/loonghao/skills/dcc-mcp-skills-creator).

```bash
openclaw skills install @loonghao/dcc-mcp
npx --yes clawhub@0.23.1 install @loonghao/dcc-mcp
```

The published package is [`@loonghao/dcc-mcp`](https://clawhub.ai/loonghao/skills/dcc-mcp). Install it with the command for the current agent host, start a new agent turn, and invoke `$dcc-mcp` explicitly if automatic routing is uncertain. A checkout may load this directory directly.

Then follow the CLI/MCP preflight above.

`dcc-mcp` supersedes `dcc-cli-gateway`; do not load both names in one agent.

## Critical Rules

| Situation | You MUST |
|-----------|----------|
| **Marketplace/Skill store intent** | Search the official catalog before recommendations or when no exact package ID was supplied; an exact known ID may go directly to consent-gated `marketplace install --reload`; live inventory is not required |
| **Starting any local DCC task** | Run `dcc-mcp-cli list`; it ensures the local gateway, then reads the local FileRegistry |
| **Startup state is ambiguous** | Run `dcc-mcp-cli doctor`; inspect selected profile, registry dir, local inventory, direct-control readiness counts, daemon status, and server binary diagnostics |
| **Starting any remote DCC task** | Select or override a profile with `dcc-mcp-cli gateway set <name>` or `dcc-mcp-cli list --gateway <name>` |
| **Task needs gateway stats or Skill reflection** | Add `--require-gateway --agent-session-id <task-id>` before the first tool call and keep the same task ID for all calls; do not mix direct and measured routes |
| Shell reports `dcc-mcp-cli` command-not-found | Ask permission, then run `python scripts/check_cli.py --ensure-cli --pretty`; the approved helper installs and rechecks health/inventory without another confirmation |
| CLI runs but gateway auto-ensure fails | Run `dcc-mcp-cli doctor`; do not reinstall the CLI or inspect Python-package/server internals |
| Inventory returns `total == 0` | Stop; do not run `search`, `describe`, or `call` |
| Remote gateway unreachable | Stop; explain; ask user permission before troubleshooting |
| User has not agreed to setup | Do not install packages, edit env files, launch GUI apps, or write configs |
| User approved setup | Follow [`references/ZERO_INSTANCES_CLI.md`](references/ZERO_INSTANCES_CLI.md) |
| Timeout, temporary `unreachable`, or DCC restart | Preserve operation IDs and follow the recovery contract in [`references/CLI_CHEATSHEET.md`](references/CLI_CHEATSHEET.md); never blindly replay a mutation or reuse stale slugs |

## Step 0 — Local Inventory First

Run this first when local work begins or a DCC adapter restarts:

```bash
dcc-mcp-cli list
# Only when startup or readiness is unclear:
dcc-mcp-cli doctor
```

Interpret the result:

- `list.total > 0` -> inspect status/dispatch metadata. Local `search`, `describe`, `load-skill`, `call`, and `reload-skills` only route to rows ready for local CLI control; use `wait-ready` or `doctor` for live-but-booting rows, including sidecars that have not reached `dispatch_status=ready`.
- `doctor.profile.selected.mode` / `doctor.local.registry_dir` -> confirms which local/remote mode and registry path the CLI is using before adapter setup.
- Error / timeout -> stop; explain the failure to the user. For remote
  profiles, the CLI cannot auto-start the gateway.

## Step 1 — Select a Live Instance

Run `dcc-mcp-cli list` whenever a DCC starts or stops. Report `total`, counts by `dcc_type`, stale rows, and the chosen instance. If `total == 0`, stop and ask whether the user wants setup guidance; continue only after approval.

## Step 2 — Search Tools

Only run this when inventory shows at least one non-stale target:

```bash
# CLI (primary)
dcc-mcp-cli search --query "create sphere" --dcc-type maya --limit 20

# Python fallback
python scripts/dcc_gateway.py search --query sphere --dcc-type maya --limit 20
```

Copy the returned slug exactly and follow that hit's `next_step`; do not run
separate broad searches for selection, geometry, and scripting unless the
first result proves they are needed. Local and gateway slugs use the same
agent-facing shape:

```text
maya.a1b2c3d4.maya_primitives__create_sphere
```

Never hand-build slugs.

## Step 3 — Follow `next_step`

- `action=call` — call directly; no-schema tools receive this only when compact safety hints are already present.
- `action=describe` — inspect the schema and safety annotations, then call.
- `action=load_skill` — pass the returned arguments unchanged. If the load
  response includes `compact_schema` and `next_step.action=call`, call directly;
  otherwise describe the selected target once.

```bash
# Only when next_step.action=describe
dcc-mcp-cli describe maya.a1b2c3d4.maya_primitives__create_sphere

# Python fallback
python scripts/dcc_gateway.py describe maya.a1b2c3d4.maya_primitives__create_sphere
```

When describe or `compact_schema` is returned, use those exact parameter names
and safety annotations before calling.

## Step 4 — Call a Tool

```bash
# CLI (primary)
dcc-mcp-cli call maya.a1b2c3d4.maya_primitives__create_sphere \
  --require-gateway \
  --agent-session-id task-42 \
  --json '{"radius":2.0}'

# When the workflow reserved this instance, repeat the exact lease owner.
dcc-mcp-cli call maya.a1b2c3d4.maya_primitives__create_sphere \
  --require-gateway \
  --agent-session-id task-42 \
  --json '{"radius":2.0}' \
  --meta-json '{"lease_owner":"workflow-42"}'

# Python fallback
python scripts/dcc_gateway.py call maya.a1b2c3d4.maya_primitives__create_sphere \
  --json '{"radius":2.0}'
```

For asynchronous render/cook tools, add `--wait`; the CLI polls `jobs_get_status` at most once per second until terminal state and writes a 5%-step progress bar plus a 30-second stalled-job heartbeat to stderr while keeping the final result on stdout. Use `--wait-timeout-secs` for longer runs.
The bar uses `progress.current`, `progress.total`, and `progress.message`; do not repeatedly scan output files when typed progress exists.
Native MCP/REST clients may subscribe to `/v1/jobs/{job_id}/events`; otherwise keep the returned `job_id` and use bounded status polling.
Do not create a scheduled task by default. After an explicit cross-session monitoring request, schedule only a one-shot status check for that ID and stop it at terminal state.
During a host reload or gateway restart, keep the ID because status stays routable; `--wait` reports `control_plane_reconnecting` then `wait_recovery` and returns `tracking_status=owner_exited` when the DCC/sidecar owner is gone; never resubmit the render or cook.

Tool-specific fields (`code`, `file_path`, `radius`, and similar) belong inside
the `--json` object. Do not pass them as top-level CLI flags unless the CLI adds
an explicit first-class flag later.

If the selected instance has an active pool lease, every `call` must carry the
same `lease_owner` through `--meta-json`. Missing owner metadata fails with
`instance-leased`; a different owner fails with `lease-owner-mismatch`. Do not
retry either error without the matching workflow owner or a different instance.
Expired leases and instances that were never leased need no owner metadata.
The hidden compatibility lease workflow requires a non-empty owner without
surrounding whitespace on acquire and the same owner on release; ownerless
release never clears an active lease.
The owner is a visible coordination label, not an authentication secret. Lease
enforcement coordinates gateway and local CLI workflows; it does not protect a
DCC adapter endpoint that an untrusted client can reach directly.

For generated scripts, binary descriptors, or other payloads that may exceed a
shell's command-line limit, pass the JSON object through a UTF-8 file or stdin:

```bash
dcc-mcp-cli call godot_project__write_script --json-file payload.json
generate_payload | dcc-mcp-cli call godot_project__write_script --json-file -
```

Use `--json` or `--json-file`, never both. `--json-file -` keeps large payloads
off the process command line, which is especially important on Windows.

See [`references/CLI_CHEATSHEET.md`](references/CLI_CHEATSHEET.md) for command
patterns and common errors.

## Step 5 — Analyze Failures and Report Bugs

Do not guess a root cause or blindly replay a mutation. Preserve `request_id`, `trace_id`, `job_id`, tool slug, instance id, sanitized arguments, error code, and validation result.

```bash
dcc-mcp-cli doctor
dcc-mcp-cli stats --range 24h --status failure --session-id task-42
dcc-mcp-cli search --query "report feedback" --dcc-type maya
dcc-mcp-cli describe <returned-feedback-tool-slug>
dcc-mcp-cli call <returned-feedback-tool-slug> --json \
  '{"tool_name":"maya_geometry__create_sphere","intent":"Create a sphere","attempt":"radius=2.0","blocker":"Radius was ignored","severity":"blocked"}'
```

Use `doctor` for profile, registry, daemon, binary, and readiness failures. For a tool failure, refresh `describe`, compare the schema/annotations with the attempt, inspect failure-only stats, and call `dcc_feedback__report`. Its severity is `blocked`, `workaround_found`, or `suggestion`; it records feedback but does not create an external issue.

For a gateway-routed failure, use the CLI-returned `request_id` to read `/v1/debug/agent-traces/<request_id>` and public-safe `/v1/debug/issue-reports/<request_id>`. The latter supplies a bounded summary and suggested GitHub title/body. Never publish `?mode=raw` without human review; create an external issue only with user authorization.

Route schema/script/Skill defects to the owning package and `dcc-mcp-skills-creator`; dispatch/readiness/install/wiring defects to the adapter and `dcc-mcp-creator`; shared gateway/CLI/protocol defects to `dcc-mcp-core`. Include the smallest reproduction and safe report, not hidden reasoning.

### Review Reusable Friction

```bash
dcc-mcp-cli stats --range 24h --dcc-type maya --session-id task-42
```

Only after acceptance, inspect `stats_coverage`. Gateway SQLite excludes `local_mcp_direct`; `configured_route_recorded=false` cannot support reflection. Re-run through `--require-gateway`; zero calls means missing evidence.

Load `dcc-mcp-skills-creator` and request `review_skill_improvement` with bounded task, stats, validation, and existing-skill summaries. Stats are not root-cause proof; prefer `no_change`, then `update_existing`, and create only for a repeated stable workflow. The review never authorizes out-of-scope changes.

## Updates and Marketplace Maintenance

Use the gateway release manifest for binary checks. `update apply` stages the
CLI for its next launch; a running server must be updated in its own
environment. The Admin Instances panel remains check-only because the gateway
cannot prove an instance's installation root. See the CLI cheatsheet for
platform manifests and server-side update details.

```bash
dcc-mcp-cli update check
dcc-mcp-cli update apply
```

For marketplace Skills, search first when the exact package ID is not known:

```bash
dcc-mcp-cli marketplace search --query "maya rigging" --limit 20
dcc-mcp-cli marketplace inspect <package_name>
dcc-mcp-cli marketplace install <package_name> --dcc maya --reload
```

`--query "maya rigging"` remains supported for scripts. Search and inspect are
read-only; install/update require consent. Inspect is optional when the exact
package ID is already known, and `--dcc` is optional for single-DCC packages.
After updates or installs without `--reload`, run `reload-skills`; then use
`load-skill` only if the adapter did not auto-load it.

Use `install` for adapter plans, never for marketplace Skills:

```bash
dcc-mcp-cli install --dcc-type maya --version 2026
```
Ask before `--execute`, follow the returned `next_steps`, and do not treat
package installation as live registration. If no standard DCC is found, ask for an absolute path and pass `--dcc-path`. If auto-install is disabled, show
the returned policy prompt and hand off to the named deployment owner.

## What This Skill Does Not Use

- Native MCP `tools/list`, `tools/call`, or `resources/read` on the agent host
  (IDE users should use MCP instead of this skill)
- Raw `curl` workflows except when debugging the gateway itself
- Direct Maya/Blender/Houdini scripting

The CLI is the **default agent-facing control plane**. The Python fallback uses
the same gateway REST endpoints only when the CLI is unavailable after a
verified install attempt fails. The gateway still serves MCP for IDE clients in
parallel; choosing this skill does not replace or disable the IDE MCP path.
