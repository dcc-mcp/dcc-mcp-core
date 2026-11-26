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
  tools are checked. OpenClaw
  and other shell agents use dccmcp inventory/search/describe/call. The
  compatibility alias dcc-mcp-cli remains supported;
  MCP-native IDEs use the gateway MCP surface. Not for tasks unrelated to DCC
  software.
license: MIT-0
allowed-tools: Bash Read
metadata:
  dcc-mcp:
    dcc: python
    layer: infrastructure
    compatibility: Cross-platform Windows/macOS/Linux. Prefers dccmcp on PATH; can download release asset from GitHub; local profile needs no gateway env. DCC_MCP_BASE_URL is optional for remote/legacy gateway REST fallback.
    version: "0.21.0"  # x-release-please-version
    search-hint: "dcc control operate UI control menu dialog window button click keyboard Maya Blender Houdini Photoshop 3ds Max Nuke Unreal Godot RenderDoc Substance connect create edit render automate cli gateway stats 操作 控制 界面 菜单 弹窗 窗口 按钮 点击 键盘"
    tags: "dcc, dcc-ui-control, ui-control, maya, blender, houdini, photoshop, nuke, unreal, godot, renderdoc, cli, gateway, clawhub, openclaw"
  openclaw:
    emoji: "🖥️"
    homepage: https://github.com/dcc-mcp/dcc-mcp-core/blob/main/skills/dcc-mcp/SKILL.md
---

# DCCMCP — Default DCC Control

The primary agent command is `dccmcp`. Existing installations can continue to
use the compatibility alias `dcc-mcp-cli`.

The CLI is maintained and released from
[`dcc-mcp/dcc-mcp-cli`](https://github.com/dcc-mcp/dcc-mcp-cli). The core
repository provides the shared MCP client, gateway, adapter runtime, and this
skill package.

> **Route DCC intent here first.** MCP-native agents call the structured gateway
> tools directly; shell-only agents use `dccmcp` — no MCP connector
> required.


Existing installations can use the compatibility alias `dcc-mcp-cli`. These
commands remain supported during migration.

```bash
dcc-mcp-cli dcc-types
dcc-mcp-cli stats --range 24h --dcc-type maya --session-id task-42
```

Use this skill whenever the user asks to operate a supported DCC application.
In an MCP-native host, use the gateway's structured inventory, search,
describe, load, and call tools. In an **agent or headless CLI host** without an
MCP connector, control DCC-MCP through **`dccmcp`**. The CLI uses local
FileRegistry + direct per-DCC MCP in the built-in `local` profile, and gateway
REST (`/v1/search`, `/v1/describe`, `/v1/call`) for named remote profiles.

The CLI returns JSON by default. The bundled Python fallback is gateway-REST
only and sends `Accept: application/json` because the gateway REST API itself
now defaults to compact TOON for agent-facing routes.

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
2. If host support is unclear, run `dccmcp discover` and
   `dccmcp dcc-types`; use the exact catalog `dcc_type` value instead of
   guessing aliases.
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

DCC-MCP supports two integration paths. `dccmcp` is the default for every
shell-capable agent. Native MCP remains the fallback for MCP-only IDE clients
or when the user explicitly chooses that integration.

| Dimension | **Agent path** (this skill) | **IDE path** (native MCP) |
|-----------|----------------------------|---------------------------|
| **Who** | OpenClaw, Hermes, Codex CLI, CI bots, custom agent runtimes, and any other host with shell access | MCP-only Cursor, Claude Desktop, VS Code MCP, or another client without shell access |
| **Transport** | `dccmcp` → local MCP or remote gateway REST | MCP Streamable HTTP → gateway `/mcp` |
| **Discovery surface** | `search` → `describe` → `call` via CLI or bundled Python helper | Gateway MCP tools: `search`, `describe`, `load_skill`, `call` |
| **Setup** | Install this skill and keep the official `dccmcp` on `PATH`; installation/download requires user consent | Add gateway URL to IDE MCP settings (see repo `docs/guide/*`) |
| **When to choose** | Default whenever the agent can run shell commands | The client cannot run shell commands or the user explicitly requests native MCP |
| **Resources / prompts** | Not covered here; use REST `/v1/context` or IDE MCP if needed | `resources/read`, `prompts/get`, SSE subscribe via MCP |

**Decision rules for agents loading this skill:**

1. **Use this routing policy first** for every DCC-control request, whether the
   host is MCP-native or shell-only.
2. **Shell-capable host** — use `dccmcp`
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

If both `dccmcp` and its `dcc-mcp-cli` compatibility alias are missing, obtain the user's consent before installing the
latest official release:

```bash
# Linux/macOS
curl -fsSL https://raw.githubusercontent.com/dcc-mcp/dcc-mcp-cli/main/scripts/install-cli.sh | sh

# Windows PowerShell
powershell -ExecutionPolicy Bypass -c "irm https://raw.githubusercontent.com/dcc-mcp/dcc-mcp-cli/main/scripts/install-cli.ps1 | iex"
```

After installation, use `dccmcp update` for the normal self-update flow. It
checks the release manifest, downloads the platform asset, verifies its
SHA-256 digest, and stages the binary for the next CLI launch. Use
`dccmcp update check` for a read-only check or `dccmcp update apply` when an
explicit apply action is clearer in an operator runbook. Set
`DCC_MCP_CLI_AUTO_UPDATE=true`, or pass `--auto-update`, to perform the same
best-effort check and staging before an ordinary CLI command. A staged update
is reused instead of downloaded again. Automatic update failures are reported
as warnings and do not block the current command. This updates the CLI only;
it does not update a running `dcc-mcp-server`.

Before creating an adapter install plan, run the read-only host discovery
command. It checks PATH, configured search roots, common application folders,
and mounted Windows volumes without launching a DCC.

```bash
dccmcp discover
dccmcp discover --dcc-type maya
dccmcp discover --dcc-type maya --path "D:/Autodesk"
```

`dccmcp install --dcc-type <dcc>` includes matching local installations in
`detected_installations` and automatically selects a detected host Python when
the plan does not receive `--python`. Treat this as a setup hint. Start the
host plugin and confirm a live registry row with `dccmcp list` before
calling tools.

Set `DCC_MCP_DCC_SEARCH_PATHS` to a platform-separated list of additional
studio or mounted-volume roots when an installation is outside the standard
folders.

Release downloads are verified with SHA-256 sidecars. Keep
`DCC_MCP_ALLOW_UNSIGNED_DOWNLOADS` unset unless a trusted legacy mirror has no
sidecar and the operator has explicitly accepted that exception.

### DCC UI Control fallback

**DCC UI Control** is the public capability name and `ui-control` is its stable
CLI command. DCCMCP is the control-plane layer. The project-owned **DCC-CUA**
capability layer is the execution boundary behind that route. It owns exact
window binding, screenshots, UI Automation, native input, visible safety
effects, user interruption, and audit records. The `app_ui__*` names below are
legacy runtime tool identifiers and MCP compatibility names. Do not call the
feature generic “Computer Use” in agent-facing text.

Do not choose UI Control first. Search, describe, and call the structured DCC
skill, host API, or adapter script that owns the operation. If the operation is
reported as unsupported, no suitable tool exists, or semantic UI Automation
cannot reach the required control, make an agent-directed transition to DCC UI
Control:

1. `app_ui__snapshot` with an exact `process_id`, `window_handle`, or
   `window_title`.
2. `app_ui__find` and a semantic `app_ui__act` when possible; otherwise one
   screenshot-coordinate `app_ui__act` using that snapshot.
3. `app_ui__snapshot` after every action before choosing the next action.
4. `app_ui__stop_computer_use` when the fallback completes, fails, or is
   abandoned so the visible effects and input owner are released.

Shell agents should use the stable CLI wrapper instead of hand-building legacy
tool slugs. Before the first observation or input action, the DCC-CUA host
must report `provider=dcc-cua`, its runtime version, and the exact target PID
and HWND. No UI action is valid without all four bindings. This requirement
also applies when an explicit DCC-CUA request targets a browser or another
non-DCC application through the project `ui-control` route.

```bash
dccmcp ui-control snapshot --instance-id <id> \
  --json '{"session_id":"menu","process_id":1234}'
dccmcp ui-control find --instance-id <id> \
  --json '{"session_id":"menu","label":"Settings","role":"menu_item"}'
dccmcp ui-control act --instance-id <id> \
  --json '{"session_id":"menu","control_id":"settings","action":"click","snapshot_id":"<snapshot_id>"}'
dccmcp ui-control snapshot --instance-id <id> \
  --json '{"session_id":"menu","process_id":1234}'
dccmcp ui-control stop --instance-id <id> \
  --json '{"session_id":"menu"}'
```

Use `ui-control wait` for condition-based waits. Every subcommand accepts
`--dcc-type`, `--json-file`, `--meta-json`, and `--timeout-secs` with the same
meaning as `call`.

DCCMCP does not implement desktop input itself. The CLI validates and routes
the request, while the DCC-CUA host performs the scoped operation. The only
valid sequence is `snapshot -> find -> one act -> snapshot -> stop`. A
`policy_disabled`, `permission_denied`, `authentication`, `confirmation`,
`desktop_unavailable`, or `user_interrupted` result stops the workflow. Do not
silently switch to a generic Computer Use provider, retry after
`Ctrl+Alt+Esc`, or change the target session.

Do not transition or retry through another UI/input path after a policy,
authorization, authentication, security, confirmation, `desktop_unavailable`,
or `user_interrupted` result. Those outcomes require the user or environment to
resolve the boundary first.

Never widen the scope to the desktop or reuse coordinates across snapshots.
Native pointer or keyboard fallback requires one exact `process_id` or
`window_handle` already bound by the adapter/operator through
`DCC_MCP_APP_UI_UIA_PROCESS_ID` or `DCC_MCP_APP_UI_UIA_WINDOW_HANDLE`; request
scope can only narrow that trusted target. Title-only and process-name scopes
are observation-only.
If the user presses Ctrl+Alt+Esc and the tool returns `user_interrupted`, stop without
retrying, changing `session_id`, or starting a new session. Only call
`app_ui__snapshot(resume_computer_use=true)` after the user explicitly asks to
resume DCC UI Control.

For an exact PID/HWND, `app_ui__snapshot` automatically uses native window
capture if Windows UIA enumeration fails or times out; treat the returned tree
as image-only and continue with one bounded native action.
On the CLI+REST path, rich images are materialized into a bounded local
`artifact_path`; use the host agent's local image viewer on that absolute path
instead of expecting base64 JSON to render in the terminal.

Internal studios can fork this skill once and reuse the same CLI+REST workflow across
agents without maintaining per-host MCP server lists.

---

## Gateway Profiles And Local-First Inventory

`dccmcp` has a built-in `local` profile. In local mode, agent-control
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
dccmcp gateway register https://workstation.example:19293 --name pcA
dccmcp gateway list
dccmcp gateway set pcA
dccmcp gateway set local
dccmcp list --gateway pcA
```

Use `--gateway <name>` to override the current profile for one command.
`--base-url` / `DCC_MCP_BASE_URL` remain direct endpoint overrides for legacy
scripts and smoke checks.

Agent-control commands (`list`, `search`, `describe`, `load-skill`, `call`,
`wait-ready`, `reload-skills`, and `stop-instance`) and endpoint-level commands
such as `health`, `update`, and `smoke` without an explicit `--url` auto-ensure
loopback HTTP gateway targets. File-only commands and explicit lifecycle
commands do not auto-start the gateway.
When startup state is unclear, run `dccmcp doctor` before troubleshooting
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

1. Use `dccmcp list` for local inventory, or `dccmcp list --gateway <name>` for a remote profile.
2. Use `dccmcp` for all subsequent commands when it is on `PATH`.
3. If missing, ask user permission, then download `dccmcp` from GitHub Releases.
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
| **Starting any local DCC task** | Run `dccmcp list`; it ensures the local gateway, then reads the local FileRegistry |
| **Startup state is ambiguous** | Run `dccmcp doctor`; inspect selected profile, registry dir, local inventory, direct-control readiness counts, daemon status, and server binary diagnostics |
| **Starting any remote DCC task** | Select or override a profile with `dccmcp gateway set <name>` or `dccmcp list --gateway <name>` |
| `dccmcp` missing | Ask permission before `--ensure-cli`; fallback Python REST is allowed if download fails |
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
# Installed host applications and host Python interpreters, without launching DCCs
dccmcp discover

# Supported adapter identifiers, only when support is unclear
dccmcp dcc-types

# Local FileRegistry inventory
dccmcp list

# No-launch startup diagnostics when state is unclear
dccmcp doctor

# Optional gateway health check
dccmcp health
```

Interpret the result:

- `list.total > 0` -> inspect status/dispatch metadata. Local `search`, `describe`, `load-skill`, `call`, and `reload-skills` only route to rows ready for local CLI control; use `wait-ready` or `doctor` for live-but-booting rows, including sidecars that have not reached `dispatch_status=ready`.
- `doctor.profile.selected.mode` / `doctor.local.registry_dir` -> confirms which local/remote mode and registry path the CLI is using before adapter setup.
- `health.status == "ok"` -> gateway is up when you need gateway endpoint/admin/update workflows.
- Error / timeout -> stop; explain the failure to the user. For remote
  profiles, the CLI cannot auto-start the gateway.

---

## Step 1 — Select a Live Instance

Run `dccmcp list` whenever a DCC starts or stops. Report `total`, counts by
`dcc_type`, stale rows, and the chosen `instance_id` or `instance_short`. If
`total == 0`, stop and ask whether the user wants setup guidance. Continue only
after explicit approval.

---

## Step 2 — Search Tools

Only run this when inventory shows at least one non-stale target:

```bash
# CLI (primary)
dccmcp search --query sphere --dcc-type maya --limit 20

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
dccmcp describe maya.a1b2c3d4.maya_primitives__create_sphere

# Python fallback
python scripts/dcc_gateway.py describe maya.a1b2c3d4.maya_primitives__create_sphere
```

Read `tool.inputSchema` and safety annotations before calling.

---

## Step 4 — Call a Tool

```bash
# CLI (primary)
dccmcp call maya.a1b2c3d4.maya_primitives__create_sphere \
  --json '{"radius":2.0}' \
  --meta-json '{"agent_context":{"session_id":"task-42"}}'

# When the workflow reserved this instance, repeat the exact lease owner.
dccmcp call maya.a1b2c3d4.maya_primitives__create_sphere \
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
dccmcp call godot_project__write_script --json-file payload.json
generate_payload | dccmcp call godot_project__write_script --json-file -
```

Use `--json` or `--json-file`, never both. `--json-file -` keeps large payloads
off the process command line, which is especially important on Windows.

See [`references/CLI_CHEATSHEET.md`](references/CLI_CHEATSHEET.md) for command
patterns and common errors.

---

## Step 5 — Review Reusable Friction

Only after task acceptance, query narrowly scoped gateway evidence:

```bash
dccmcp stats --range 24h --dcc-type maya --session-id task-42
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

Use the gateway update manifest for binary checks:

Official release builds use the platform-specific manifest from the latest
GitHub release by default. Set `DCC_MCP_UPDATE_MANIFEST_URL` only to override
that source for a studio mirror or pinned deployment.

```bash
# Normal self-update: check, download, verify, and stage for the next launch.
dccmcp update

# Read-only check or explicit staging.
dccmcp update check
dccmcp update apply

# Best-effort automatic update before this command.
dccmcp --auto-update list

# Check a server/instance version shown in the admin panel.
dccmcp update check --binary dcc-mcp-server --current-version 0.18.16

# Stage a CLI binary update for the next CLI launch.
dccmcp update apply
```

`dccmcp update` and `dccmcp update apply` only stage the CLI binary. To
update a running server binary, run the server-side command in that server
environment:

```bash
dcc-mcp-server update check
dcc-mcp-server update apply
```

Use marketplace commands for skills:

```bash
dccmcp marketplace search --query rigging --dcc maya --limit 20
dccmcp marketplace inspect <package_name>
dccmcp marketplace install <package_name> --dcc maya
dccmcp reload-skills --dcc-type maya
dccmcp marketplace outdated --dcc maya
dccmcp marketplace update <package_name> --dcc maya
dccmcp reload-skills --dcc-type maya
```

Host-neutral and multi-DCC packages are installed once under the shared
`~/.dcc-mcp/marketplace/any` root. DCC adapters search that root before the
host-specific marketplace root, and `--dcc all` addresses the shared root.

Use marketplace release commands for package authors and CI:

```bash
dccmcp marketplace pack ./my-skill --out dist/
dccmcp marketplace publish ./my-skill \
  --catalog ./marketplace.json \
  --install-url https://github.com/<owner>/<repo>/releases/download/v0.1.0/my-skill.zip \
  --sha256 sha256:<digest>
```

After installing or updating skills, first run
`dccmcp reload-skills --dcc-type <dcc>` so running adapters re-scan the
marketplace skill path. Then use `dccmcp load-skill` for a live instance
when the adapter has not auto-loaded the skill yet.

Use `install` for adapter plans, not marketplace skills:

```bash
dccmcp install --dcc-type maya --version 2026
dccmcp install --dcc-type maya --version 2026 --python "C:/Program Files/Autodesk/Maya2026/bin/mayapy.exe"
dccmcp install --dcc-type maya --version 2026 --python "C:/Program Files/Autodesk/Maya2026/bin/mayapy.exe" --execute
```

Agents must ask before using `--execute`. The executor prompts for consent,
rolls back completed steps if a later step fails, verifies pip packages with
`pip show`, and verifies git/zip/path installs by checking their target path.
Package install is not online registration: the DCC plugin or sidecar must
start and remain alive before `dccmcp list` shows an instance. Treat the
install JSON `next_steps` array as the authoritative machine-readable follow-up
sequence. If it includes `read-install-instructions`, read that adapter
repository's raw `install.md` first; it owns host-specific setup. Then
start/enable the host plugin, run `doctor`, confirm `list`, wait for readiness,
search/call tools, and use marketplace `search`, `inspect`, `install`, then
`reload-skills` for optional community skill packages.

If `install_policy.auto_install_enabled` is `false`, do not retry with
`--execute`. Show the returned `install_policy.prompt` to the user and hand off
to the named Pipeline TD / studio deployment path. Studios set this through
`DCC_MCP_INSTALL_DISABLED=1` and `DCC_MCP_INSTALL_DISABLED_PROMPT`.

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
