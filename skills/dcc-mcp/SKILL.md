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
    compatibility: Cross-platform Windows/macOS/Linux. Prefers dcc-mcp-cli on PATH; can download release asset from GitHub; local profile needs no gateway env. DCC_MCP_BASE_URL is optional for remote/legacy gateway REST fallback.
    version: "0.19.64"
    search-hint: "dcc control operate UI control menu dialog window button click keyboard Maya Blender Houdini Photoshop 3ds Max Nuke Unreal Godot RenderDoc Substance connect create edit render automate cli gateway stats marketplace skill catalog recommend install update 商城 技能 操作 控制 界面 菜单 弹窗 窗口 按钮 点击 键盘"
    tags: "dcc, dcc-ui-control, ui-control, maya, blender, houdini, photoshop, nuke, unreal, godot, renderdoc, cli, gateway, marketplace, skill-catalog, clawhub, openclaw"
  openclaw:
    emoji: "🖥️"
    homepage: https://github.com/dcc-mcp/dcc-mcp-core/blob/main/skills/dcc-mcp/SKILL.md
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

The CLI returns JSON by default. The bundled Python fallback is gateway-REST
only and sends `Accept: application/json` because the gateway REST API itself
now defaults to compact TOON for agent-facing routes.

## Marketplace Intent — Search Before Recommending

Any request to find, compare, recommend, install, or update a DCC-MCP
marketplace Skill must start with the official CLI catalog, even when the user
says “Skill store”, “marketplace”, or “商城” without naming DCC-MCP:

```bash
dcc-mcp-cli marketplace search --query "maya rigging" --limit 20
dcc-mcp-cli marketplace inspect <exact-name-from-search>
```

Marketplace discovery does not require a live DCC instance. Do not apply
the live-inventory `total == 0` stop rule to `marketplace search` or `inspect`.
Use the user's capability words first. If there are no results, retry once with
a shorter capability query or without the DCC filter; never invent a package
name or substitute a web recommendation for the CLI result.

Installing or updating changes local state. Inspect the exact returned package
and obtain user consent before `marketplace install` or `update`; then run
`reload-skills` and, only when needed, `load-skill`. The Python REST fallback
does not implement marketplace commands, so a missing CLI follows the
consent-gated official CLI installation path below.

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
4. Search by the user's intent and target DCC, copy the returned tool slug,
   inspect its schema and annotations, then call it.
5. Use raw scripting only when no typed tool covers the operation and the
   adapter exposes an explicit, policy-compliant automation tool. A repeated
   scripting pattern is a candidate for a reusable DCC skill.
6. Use scoped DCC UI Control only after structured tools report the operation
   as unsupported or the required host control is not exposed.

If the requested DCC is installed but no live adapter instance is registered,
follow the zero-instance flow. Do not silently switch to GUI automation or a
different DCC application.

---

## Agent Path vs IDE Path

DCC-MCP supports two integration paths. `dcc-mcp-cli` is the default for every
shell-capable agent. Native MCP remains the fallback for MCP-only IDE clients
or when the user explicitly chooses that integration.

| Dimension | **Agent path** (this skill) | **IDE path** (native MCP) |
|-----------|----------------------------|---------------------------|
| **Who** | OpenClaw, Hermes, Codex CLI, CI bots, custom agent runtimes, and any other host with shell access | MCP-only Cursor, Claude Desktop, VS Code MCP, or another client without shell access |
| **Transport** | `dcc-mcp-cli` → local MCP or remote gateway REST | MCP Streamable HTTP → gateway `/mcp` |
| **Discovery surface** | `search` → `describe` → `call` via CLI or bundled Python helper | Gateway MCP tools: `search`, `describe`, `load_skill`, `call` |
| **Setup** | Install this skill and keep the official `dcc-mcp-cli` on `PATH`; installation/download requires user consent | Add gateway URL to IDE MCP settings (see repo `docs/guide/*`) |
| **When to choose** | Default whenever the agent can run shell commands | The client cannot run shell commands or the user explicitly requests native MCP |
| **Resources / prompts** | Not covered here; use REST `/v1/context` or IDE MCP if needed | `resources/read`, `prompts/get`, SSE subscribe via MCP |

**Decision rules for agents loading this skill:**

1. **Use this routing policy first** for every DCC-control request, whether the
   host is MCP-native or shell-only.
2. **Shell-capable host** — use `dcc-mcp-cli`
   (`inventory` → `search` → `describe` or `load-skill` → `call`), even when a
   native MCP connector is also available.
3. **MCP-only host** — call the gateway/DCC structured tools directly
   (`inventory` → `search` → `describe` or `load_skill` → `call`). Do not ask the
   user to switch clients or manually repeat the operation.
4. **Do not mix paths in one turn** — pick CLI+REST or MCP for the whole task,
   not both.
5. **Zero instances** — stop, explain, ask consent before bootstrap; see
   [`references/ZERO_INSTANCES_CLI.md`](references/ZERO_INSTANCES_CLI.md).

### CLI installation

If `dcc-mcp-cli` is missing, obtain the user's consent before installing the
latest official release. Exact Linux/macOS and Windows commands, update
semantics, and development fallbacks live in
[`references/CLI_CHEATSHEET.md`](references/CLI_CHEATSHEET.md).

### DCC UI Control fallback

**DCC UI Control** is the public capability name. Its dedicated runtime Skill
is loaded with `dcc-mcp-cli load-skill ui-control`; it is not one of the three
installable agent Skills in this suite. Use it only after the structured DCC
capability is unsupported or cannot reach the required semantic UI:

1. `ui_control__snapshot` with an exact `process_id`, `window_handle`, or
   `window_title`.
2. `ui_control__find` and one semantic `ui_control__act` when possible.
3. `ui_control__snapshot` after every action before choosing the next action.
4. `ui_control__stop_computer_use` when the fallback completes, fails, or is
   abandoned.

Do not transition or retry through another UI/input path after a policy,
authorization, authentication, security, confirmation, `desktop_unavailable`,
or `user_interrupted` result. Those outcomes require the user or environment to
resolve first. Never widen scope to the desktop, reuse coordinates across
snapshots, or resume a user-interrupted session without an explicit request.
Load that runtime Skill for the full target-binding, system-operation,
native-capture, and image-artifact contract.

Internal studios can fork this skill once and reuse the same CLI+REST workflow across
agents without maintaining per-host MCP server lists.

---

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

---

## Connection Order

1. Use `dcc-mcp-cli list` for local inventory, or `dcc-mcp-cli list --gateway <name>` for a remote profile.
2. Use `dcc-mcp-cli` for all subsequent commands when it is on `PATH`.
3. If missing, ask user permission, then download `dcc-mcp-cli` from GitHub Releases.
4. If the download fails, use the bundled Python stdlib REST fallback.

Install via OpenClaw/ClawHub, or point your agent at this `SKILL.md` after cloning
[`dcc-mcp-core/skills/dcc-mcp/`](https://github.com/dcc-mcp/dcc-mcp-core/tree/main/skills/dcc-mcp).

`dcc-mcp` supersedes the former `dcc-cli-gateway` skill slug. Do not install or
load both names in one agent: install `dcc-mcp`, verify it is discoverable, then
remove the old package to avoid duplicate intent routing.

---

## Critical Rules

| Situation | You MUST |
|-----------|----------|
| **Marketplace/Skill store intent** | Run `dcc-mcp-cli marketplace search` first, then inspect the exact returned name before any recommendation or consent-gated mutation; live inventory is not required |
| **Starting any local DCC task** | Run `dcc-mcp-cli list`; it ensures the local gateway, then reads the local FileRegistry |
| **Startup state is ambiguous** | Run `dcc-mcp-cli doctor`; inspect selected profile, registry dir, local inventory, direct-control readiness counts, daemon status, and server binary diagnostics |
| **Starting any remote DCC task** | Select or override a profile with `dcc-mcp-cli gateway set <name>` or `dcc-mcp-cli list --gateway <name>` |
| `dcc-mcp-cli` missing | Ask permission before `--ensure-cli`; fallback Python REST is allowed if download fails |
| CLI auto-ensure fails | Stop; explain the result; do not run agent-control or gateway endpoint commands until the gateway is reachable |
| Inventory returns `total == 0` | Stop; do not run `search`, `describe`, or `call` |
| Remote gateway unreachable | Stop; explain; ask user permission before troubleshooting |
| User has not agreed to setup | Do not install packages, edit env files, launch GUI apps, or write configs |
| User approved setup | Follow [`references/ZERO_INSTANCES_CLI.md`](references/ZERO_INSTANCES_CLI.md) |
| After DCC crash/restart | Re-run `list` and `search`; old slugs may be invalid |

---

## Configuration

Use the local profile unless the user selected a remote gateway. Profile,
fallback, and installation commands live in the
[CLI cheatsheet](references/CLI_CHEATSHEET.md). Do not download, install, or
write configuration without user consent.

---

## Step 0 — Local Inventory First

Run this as the **very first step** every time you begin local work or after a
DCC adapter restarts:

```bash
# Supported adapter identifiers, only when support is unclear
dcc-mcp-cli dcc-types

# Local FileRegistry inventory
dcc-mcp-cli list

# No-launch startup diagnostics when state is unclear
dcc-mcp-cli doctor

# Optional gateway health check
dcc-mcp-cli health
```

Interpret the result:

- `list.total > 0` -> inspect status/dispatch metadata. Local `search`, `describe`, `load-skill`, `call`, and `reload-skills` only route to rows ready for local CLI control; use `wait-ready` or `doctor` for live-but-booting rows, including sidecars that have not reached `dispatch_status=ready`.
- `doctor.profile.selected.mode` / `doctor.local.registry_dir` -> confirms which local/remote mode and registry path the CLI is using before adapter setup.
- `health.status == "ok"` -> gateway is up when you need gateway endpoint/admin/update workflows.
- Error / timeout -> stop; explain the failure to the user. For remote
  profiles, the CLI cannot auto-start the gateway.

---

## Step 1 — Select a Live Instance

Run `dcc-mcp-cli list` whenever a DCC starts or stops. Report `total`, counts by
`dcc_type`, stale rows, and the chosen `instance_id` or `instance_short`. If
`total == 0`, stop and ask whether the user wants setup guidance. Continue only
after explicit approval.

---

## Step 2 — Search Tools

Only run this when inventory shows at least one non-stale target:

```bash
# CLI (primary)
dcc-mcp-cli search --query "create sphere" --dcc-type maya --limit 20

# Python fallback
python scripts/dcc_gateway.py search --query sphere --dcc-type maya --limit 20
```

Copy the returned slug exactly. Local and gateway slugs use the same
agent-facing shape:

```text
maya.a1b2c3d4.maya_primitives__create_sphere
```

Never hand-build slugs.

---

## Step 3 — Describe Schema

```bash
# CLI (primary)
dcc-mcp-cli describe maya.a1b2c3d4.maya_primitives__create_sphere

# Python fallback
python scripts/dcc_gateway.py describe maya.a1b2c3d4.maya_primitives__create_sphere
```

Read `tool.inputSchema` and safety annotations before calling.

---

## Step 4 — Call a Tool

```bash
# CLI (primary)
dcc-mcp-cli call maya.a1b2c3d4.maya_primitives__create_sphere \
  --json '{"radius":2.0}' \
  --meta-json '{"agent_context":{"session_id":"task-42"}}'

# When the workflow reserved this instance, repeat the exact lease owner.
dcc-mcp-cli call maya.a1b2c3d4.maya_primitives__create_sphere \
  --json '{"radius":2.0}' \
  --meta-json '{"lease_owner":"workflow-42","agent_context":{"session_id":"task-42"}}'

# Python fallback
python scripts/dcc_gateway.py call maya.a1b2c3d4.maya_primitives__create_sphere \
  --json '{"radius":2.0}'
```

For a tool declared `execution: async` (or with a positive timeout hint), a
remote REST profile returns a normal JSON envelope immediately with
`output.status="pending"` and `output.job_id`; do not treat HTTP 202 as a
failure or retry the call, because the DCC job is already running.

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

---

## Step 5 — Review Reusable Friction

Only after task acceptance, query narrowly scoped gateway evidence:

```bash
dcc-mcp-cli stats --range 24h --dcc-type maya --session-id task-42
```

Then load `dcc-mcp-skills-creator` and request its
`review_skill_improvement` prompt. Pass only bounded task, stats, validation,
and existing-skill summaries. Treat `total_calls == 0` as no telemetry
evidence, not success. Stats show aggregates, not root cause; prefer
`no_change`, then `update_existing`, and create a skill only for a repeated,
stable workflow. This review does not authorize editing or publishing outside
the task scope.

---

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

For marketplace Skills, preserve the mandatory search-first sequence:

```bash
dcc-mcp-cli marketplace search --query "maya rigging" --limit 20
dcc-mcp-cli marketplace inspect <package_name>
dcc-mcp-cli marketplace install <package_name> --dcc maya
dcc-mcp-cli reload-skills --dcc-type maya
```

`--query "maya rigging"` remains supported for scripts. Search and inspect are
read-only; install/update require consent. After either mutation, run
`reload-skills`, then `load-skill` only if the adapter did not auto-load it.
Package authors use `marketplace pack` and `marketplace publish`; full commands
live in the CLI cheatsheet.

Use `install` for adapter plans, never for marketplace Skills:

```bash
dcc-mcp-cli install --dcc-type maya --version 2026
```

Ask before `--execute`, follow the returned `next_steps`, and do not treat
package installation as live registration. If auto-install is disabled, show
the returned policy prompt and hand off to the named deployment owner.

---

## What This Skill Does Not Use

- Native MCP `tools/list`, `tools/call`, or `resources/read` on the agent host
  (IDE users should use MCP instead of this skill)
- Raw `curl` workflows except when debugging the gateway itself
- Direct Maya/Blender/Houdini scripting

The CLI is the **default agent-facing control plane**. The Python fallback uses
the same gateway REST endpoints only when the CLI is unavailable after a
download attempt fails. The gateway still serves MCP for IDE clients in parallel;
choosing this skill does not replace or disable the IDE MCP path.
