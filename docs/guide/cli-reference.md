# CLI Reference

This repository ships four operator-facing binaries. This page is the
single source of truth for every flag, every environment variable, and the
five deployment scenarios they cover. Flags on each binary map 1:1 onto an
`DCC_MCP_*` environment variable, so any deployment manifest can drive the
same configuration surface.

`dcc-mcp-cli` and `dcc-mcp-server` are published as raw GitHub Release
assets on every release. The CLI can be installed directly from a URL:

```bash
curl -fsSL https://raw.githubusercontent.com/dcc-mcp/dcc-mcp-core/main/scripts/install-cli.sh | bash

# Windows PowerShell
powershell -c "irm https://raw.githubusercontent.com/dcc-mcp/dcc-mcp-core/main/scripts/install-cli.ps1 | iex"
```

Pin a release by setting `DCC_MCP_VERSION=v0.17.17` or passing
`--version v0.17.17` to the install script.

| Binary | Role | Source |
|---|---|---|
| [`dcc-mcp-cli`](#dcc-mcp-cli) | User/CI control plane for local or remote DCC-MCP REST endpoints. | `crates/dcc-mcp-cli/` |
| [`dcc-mcp-server`](#dcc-mcp-server) | Per-DCC MCP + REST server and machine-wide gateway daemon. | `crates/dcc-mcp-server/` |
| [`dcc-mcp-tunnel-relay`](#dcc-mcp-tunnel-relay) | Public-facing WebSocket relay for the zero-config remote tunnel (#504). | `crates/dcc-mcp-tunnel-relay/` |
| [`dcc-mcp-tunnel-agent`](#dcc-mcp-tunnel-agent) | Local sidecar that registers with the relay and forwards MCP traffic. | `crates/dcc-mcp-tunnel-agent/` |

Development helper binaries (`stub_gen`) are documented in
[`AGENTS.md`](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/AGENTS.md).

---

## `dcc-mcp-cli`

Client-side control plane for DCC-MCP. It is the primary operator and agent UX;
it does not host skills and does not replace the runtime binary
`dcc-mcp-server`.

`dcc-mcp-cli` has two gateway modes:

- `local` (default): read the core default FileRegistry directly and use the
  selected DCC instance's MCP HTTP endpoint for `search`, `describe`,
  `load-skill`, `call`, `wait-ready`, and guarded `stop-instance`.
- named remote profiles: route the same control workflow through the selected
  remote gateway base URL.

Register and select remote profiles with:

```bash
dcc-mcp-cli gateway register https://workstation.example:19293 --name pcA
dcc-mcp-cli gateway list
dcc-mcp-cli gateway set pcA
dcc-mcp-cli gateway set local
```

`--gateway <name>` overrides the current profile for one command. `--base-url`
and `DCC_MCP_BASE_URL` remain supported as direct endpoint overrides for legacy
scripts and smoke checks.

In the default `local` profile, `dcc-mcp-cli list` reads the FileRegistry and
does not require or auto-start a gateway. Local `search`, `describe`,
`load-skill`, `call`, `wait-ready`, and `stop-instance` then resolve the target
instance from that registry and talk to its advertised `mcp_url` / `readyz` /
`safe_stop_url` directly. When the current profile is remote, or when
`--gateway pcA` / `--base-url ...` is supplied, the same commands use the
gateway `/v1/*` surfaces.

`list` is an inventory and diagnostics command: it keeps live `booting` rows
and sidecar rows with `dispatch_status=unavailable` visible so operators can
see startup failures. Local `search`, `describe`, `load-skill`, `call`, and
`reload-skills` only route to direct MCP instances ready for local CLI control
(`status=available` or `busy`, with `dispatch_status=ready` when that metadata
is reported). A per-DCC sidecar row is locally routable once it reports
`dispatch_status=ready`; before that it remains visible as startup diagnostics
and is not used for tool calls. Use `wait-ready` or `doctor` when `list` shows a
live instance that is not yet ready for direct local CLI control. Each local
`list` row includes `direct_control.recommended_next_action` so agents can
distinguish "route this local MCP row" from "wait for sidecar dispatch
readiness". Local rows also include `direct_control.diagnostics`, which folds
sidecar failure metadata into one stable place: `failure_stage`,
`failure_reason`, `host_rpc_*`, gateway guardian/recovery fields, and
stdout/stderr log paths when the DCC supervisor records them in the registry.
`doctor` summarizes not-ready local rows under
`local.inventory.direct_control.not_ready_instances`.

Endpoint-level commands that still need a local gateway (`health`, `update`, and
`smoke` without an explicit `--url`) auto-ensure only loopback HTTP targets
(`http://127.0.0.1:<port>` or `http://localhost:<port>`). Disable this for one
invocation with `--no-auto-gateway`. Commands that operate on local files
(`install`, `marketplace`, `lint`), local instance control commands, and
explicit lifecycle commands (`gateway ...`) do not auto-start the gateway.
Use `dcc-mcp-cli doctor` when startup state is ambiguous: it reports the
current profile config, selected mode, registry directory and inventory, local
direct-control readiness counts, gateway daemon status, and server binary
path/source/version without launching or downloading anything.

```bash
dcc-mcp-cli list
dcc-mcp-cli list --gateway pcA
dcc-mcp-cli doctor
dcc-mcp-cli health
dcc-mcp-cli --no-auto-gateway health
dcc-mcp-cli gateway register https://workstation.example:19293 --name pcA
dcc-mcp-cli gateway list
dcc-mcp-cli gateway set pcA
dcc-mcp-cli gateway set local
dcc-mcp-cli search --query sphere --dcc-type maya --instance-id abc12345
dcc-mcp-cli describe maya.abc12345.create_sphere
dcc-mcp-cli load-skill workflow --dcc-type 3dsmax --instance-id 80321760
dcc-mcp-cli call maya.abc12345.create_sphere --json '{"radius":2}'
dcc-mcp-cli call maya_scene__get_session_info --dcc-type maya --instance-id abc12345 --json '{}'
dcc-mcp-cli wait-ready --dcc-type maya --instance-id abc12345 --require skill_catalog,host_execution_bridge
dcc-mcp-cli stop-instance --dcc-type maya --instance-id abc12345 --expected-owner release-smoke-test
dcc-mcp-cli install --dcc-type maya --version 2026
dcc-mcp-cli install --dcc-type maya --version 2026 --python "C:/Program Files/Autodesk/Maya2026/bin/mayapy.exe"
dcc-mcp-cli install --dcc-type maya --version 2026 --python "C:/Program Files/Autodesk/Maya2026/bin/mayapy.exe" --execute
dcc-mcp-cli marketplace add dcc-mcp/marketplace
dcc-mcp-cli marketplace search --query hunyuan --dcc maya
dcc-mcp-cli marketplace inspect dcc-asset-hunyuan-download
dcc-mcp-cli marketplace install dcc-asset-hunyuan-download --dcc maya
dcc-mcp-cli reload-skills --dcc-type maya
dcc-mcp-cli marketplace list-installed --dcc maya
dcc-mcp-cli marketplace outdated --dcc maya
dcc-mcp-cli marketplace update dcc-mcp-maya-skills --dcc maya
dcc-mcp-cli reload-skills --dcc-type maya
dcc-mcp-cli marketplace update --all
dcc-mcp-cli update check
dcc-mcp-cli update check --binary dcc-mcp-server --current-version 0.18.16
dcc-mcp-cli update apply
dcc-mcp-cli gateway daemon start
dcc-mcp-cli gateway daemon restart
dcc-mcp-cli gateway daemon stop
dcc-mcp-cli gateway daemon status
dcc-mcp-cli lint path/to/skills
```

### Commands

| Command | REST/API contract | Meaning |
|---|---|---|
| `health` | `GET /v1/healthz` | Check the configured endpoint. |
| `doctor [--registry-dir <path>] [--gateway-port <port>]` | local filesystem + gateway probe | Report profile config/current selection, local registry path/inventory, direct-control readiness counts and not-ready diagnostics, gateway daemon status, and server binary diagnostics without auto-starting or downloading services. |
| `list [--gateway <profile>]` | local FileRegistry or `GET /v1/instances` | List live DCC instances. Defaults to local FileRegistry; remote profiles use the gateway. |
| `search [--instance-id <id>]` | local MCP `search_tools` or remote `POST /v1/search` | Search callable capabilities, optionally scoped to a full UUID or unique prefix. |
| `describe <tool-slug>` | local MCP `tools/list` or remote `POST /v1/describe` | Inspect a capability before calling it. |
| `load-skill <skill-name> [--dcc-type <dcc>] [--instance-id <id>]` | local MCP `tools/call load_skill` or remote `POST /v1/load_skill` | Activate a progressive skill and print its registered tools. |
| `call <tool-slug> --json <object>` | local MCP `tools/call` or remote `POST /v1/call` | Invoke one capability. |
| `call <backend-tool> --dcc-type <dcc> --instance-id <id> --json <object>` | local MCP `tools/call` or remote `POST /v1/dcc/{dcc}/instances/{id}/call` | Invoke a backend tool without constructing a dotted gateway slug. |
| `wait-ready [--dcc-type <dcc>] [--instance-id <id>] [--require <bits>]` | local registry + per-instance `/v1/readyz`, or remote gateway inventory + `/v1/readyz` | Wait for smoke-test readiness bits such as `skill_catalog` or `host_execution_bridge`. |
| `reload-skills [--dcc-type <dcc>] [--instance-id <id>]` | local MCP `tools/call dcc_admin__reload_skills`, or remote `POST /v1/dcc/{dcc}/instances/{id}/call` | Ask running adapters to re-scan skill search paths after marketplace installs or path changes. |
| `stop-instance --dcc-type <dcc> --instance-id <id>` | local `safe_stop_url` or remote `POST /v1/dcc/{dcc}/instances/{id}/stop` | Forward a guarded safe-stop request to instances that advertise `safe_stop_url`. |
| `install --dcc-type <dcc> [--version <v>] [--python <path>] [--execute]` | catalog-backed local plan / executor | Resolve the matching adapter and emit an auditable install plan; with `--execute`, run package-install steps with consent, rollback, and package/path verification. Live DCC checks stay in the emitted `next_steps`. |
| `marketplace add <source>` | local source registry | Register a marketplace source (`dcc-mcp/marketplace`, a GitHub `owner/repo`, raw JSON URL, or local catalog file). |
| `marketplace list` | local source registry | List the built-in, configured, and environment-provided marketplace sources. |
| `marketplace search [--query <q>] [--dcc <dcc>] [--source <source>]` | marketplace catalog JSON/YAML | Search skill package entries across configured or explicit sources. |
| `marketplace inspect <name> [--source <source>]` | marketplace catalog JSON/YAML | Print exact entry metadata including version and install fields. |
| `marketplace install <name> [--dcc <dcc>] [--source <source>] [--force]` | marketplace catalog + local filesystem/git | Install a skill package to `~/.dcc-mcp/marketplace/<dcc>/<name>/`. |
| `marketplace list-installed [--dcc <dcc>]` | local installed-state file | List locally installed marketplace packages and their versions/paths. |
| `marketplace uninstall <name> --dcc <dcc>` | local installed-state file + filesystem | Remove an installed marketplace package. |
| `marketplace outdated [NAME...] [--dcc <dcc>]` | marketplace catalog + local installed state | Compare installed versions against latest catalog entries and list packages with newer versions available. |
| `marketplace update [<name>] [--all] [--dcc <dcc>]` | marketplace catalog + git/filesystem + local installed state | Upgrade installed packages to the latest catalog version. For `git` installs, fetches the new ref in place; for other types, re-installs from the catalog. Use `--all` to update every outdated package. |
| `update check [--binary <name>] [--current-version <version>]` | `GET /v1/update/check` | Check the gateway update manifest. Defaults to the CLI binary/version; pass `--binary dcc-mcp-server` plus a server version when checking an instance shown in Admin. |
| `update apply` | `GET /v1/update/check` + download URL | Download and stage the CLI binary for the next CLI launch. It does not update running server instances; use Admin's instance update button or `dcc-mcp-server update apply` in the server environment. |
| `gateway register <url> --name <profile>` | local profile config | Persist a named remote gateway profile. |
| `gateway list` | local profile config | Show configured remote profiles and the active local/remote selection. |
| `gateway set <profile\|local>` | local profile config | Select the active gateway profile. |
| `gateway daemon start [--port <port>]` | local process | Start the local machine-wide gateway daemon. Defaults to `--gateway-idle-timeout-secs 0`, so an explicitly managed daemon stays alive with no backends. |
| `gateway daemon restart [--port <port>]` | local process | Stop the pidfile-tracked daemon, then start it again. The restart's start phase uses the same persistent default as `daemon start`. |
| `gateway daemon stop [--port <port>]` | local process | Stop a running gateway daemon by PID file and verify exit. |
| `gateway daemon status [--port <port>]` | local process | Report gateway daemon health, PID, process liveness, registry dir, PID file, health URL, and CLI version. |
| `gateway ensure/start/stop/status` | local process | Backward-compatible aliases for older scripts; prefer `gateway daemon ...` in user-facing docs. |
| `lint [PATH ...]` | local filesystem validator | Recursively validate SKILL.md packages two levels below each path by default. |

`gateway daemon start` and `gateway daemon restart` are the durable operator
paths. Their default `--gateway-idle-timeout-secs 0` disables idle shutdown;
pass a non-zero timeout only when a script intentionally wants a short-lived
daemon. Automatic loopback gateway ensure for `health`/`smoke` remains scoped to
those endpoint commands and does not affect local `list`/`search`/`call`.

`install` defaults to a planning contract: it resolves catalog entries and
spells out the adapter package / host-plugin / verification steps without
silently modifying DCC plugin folders. The JSON plan also includes
machine-readable `next_steps`: first a `read-install-instructions` step pointing
at the adapter-maintained raw `install.md` runbook when the catalog or GitHub
repo URL provides one, then command arrays for `doctor`, `list`, `wait-ready`,
`search`, marketplace skill `search`/`inspect`/`install`, and `reload-skills`,
plus the manual host-plugin start step. Pass `--python` (or
`DCC_MCP_INSTALL_PYTHON`) when a pip-based adapter must be installed into a DCC
interpreter such as `mayapy`, `hython`, or Blender's bundled Python. Pass
`--execute` to prompt for consent and run executable package-install steps.
Execution rolls back completed steps when a later step fails, uses
`<python> -m pip` for pip installs, verifies pip installs with `pip show`, and
verifies git/zip/path installs by checking that their target path exists and is
not an empty directory. A DCC is considered online only after its host plugin or
sidecar starts, remains alive, and appears in `dcc-mcp-cli list`; the CLI does
not fake gateway registration during install.

Studios with dedicated deployment pipelines can disable automatic install
execution by setting `DCC_MCP_INSTALL_DISABLED=1`. The plan still returns the
adapter metadata and `next_steps`, but `install_policy.auto_install_enabled`
is `false`, `--execute` is skipped, and the agent-facing prompt comes from
`DCC_MCP_INSTALL_DISABLED_PROMPT` (supports `{adapter}`, `{dcc_type}`, and
`{version}` placeholders). Use this for messages such as "Automatic install is
unavailable; contact Pipeline TD to deploy {adapter} for {dcc_type}."

`marketplace` is the CLI-first discovery surface for official and private
skill package catalogs. The built-in source is
`https://raw.githubusercontent.com/dcc-mcp/marketplace/main/marketplace.json`.
Additional sources are persisted under
`~/.dcc-mcp/marketplace/sources.json`, overridden with
`DCC_MCP_MARKETPLACE_SOURCES_FILE`, or supplied ephemerally with
`DCC_MCP_MARKETPLACE_SOURCES` (comma-separated). Installs land in
`~/.dcc-mcp/marketplace/<dcc>/<name>/`, with
`DCC_MCP_MARKETPLACE_INSTALL_ROOT` overriding the root. The current installer
supports `install.type: git`, `install.type: path`, and `install.type: zip`.
Archive installs verify `install.sha256` when present and reject entries that
escape the install root. DCC adapters include
`~/.dcc-mcp/marketplace/<dcc>` in their skill search paths, so installed skills
are discovered on adapter startup or the next
`dcc-mcp-cli reload-skills --dcc-type <dcc>`. After a reload, run
`dcc-mcp-cli load-skill <skill-name> --dcc-type <dcc> --instance-id <id>` when
the adapter has not auto-loaded that skill yet.

`update` compares each installed package version against the latest catalog
entry. For `git`-type installs, if a `.git` directory already exists the
command runs `git fetch && git checkout <ref>` in place; otherwise it
re-clones. The local installed-state file is updated with the new version
metadata. Installed packages with no matching catalog entry (e.g. the
source was removed) are silently skipped.

`dcc-mcp-cli update` is for binary updates exposed by the gateway update
manifest configured with `DCC_MCP_UPDATE_MANIFEST_URL` (or
`GatewayConfig.update_manifest_url`). `update check` is safe for both humans
and agents because it only reads `/v1/update/check`; the CLI auto-ensures the
local gateway before the request. `update apply` stages only the CLI binary.
For server instances, prefer the Admin Instances panel update button, which
calls `POST /admin/api/instances/{instance_id}/update` and stages
`dcc-mcp-server` with restart-required status. When operating from the server
host itself, use `dcc-mcp-server update apply` instead.

`lint` reuses the production `dcc-mcp-skills` validator, so local checks and
runtime loading fail for the same structural problems. CI runs the same command
with explicit repository skill roots via `just lint-skills`.

### CLI installation assets

The installer scripts download one of these GitHub Release assets:

| Platform | Asset |
|---|---|
| Linux x86_64 | `dcc-mcp-cli-linux-x86_64` |
| Windows x86_64 | `dcc-mcp-cli-windows-x86_64.exe` |
| macOS universal2 | `dcc-mcp-cli-macos-universal2` |

Default install locations are `~/.local/bin` on Linux/macOS and
`%LOCALAPPDATA%\dcc-mcp\bin` on Windows. Override with
`DCC_MCP_INSTALL_DIR` or `--install-dir`.

---

## `dcc-mcp-server`

Runtime binary for adapters, sidecars, bridges, and the machine-wide gateway
daemon. It remains scriptable for CI and operations, but the primary user and
agent UX is `dcc-mcp-cli`.

Invoking `dcc-mcp-server` with no subcommand behaves like `dcc-mcp-server auto`:
it ensures a local gateway daemon exists, registers the per-DCC server as a
backend, and keeps a lightweight guardian while the backend is alive so the
daemon can be re-ensured after a crash.

### Run modes

| Command | Role | Gateway behavior |
|---|---|---|
| `dcc-mcp-server` | Implicit `auto`. | Ensures the standalone gateway daemon, then registers as a backend. |
| `dcc-mcp-server auto` | Explicit form of the default behavior. | Same as the no-subcommand path. |
| `dcc-mcp-server serve` | Per-DCC MCP server. | Ensures the standalone gateway daemon, then registers as a backend. |
| `dcc-mcp-server serve --no-auto-gateway` | Per-DCC MCP server only. | Registers/serves tools but never ensures or binds the gateway port. |
| `dcc-mcp-server auto --legacy-gateway-election` | Legacy embedded gateway mode. | The per-DCC process competes for the gateway port directly. |
| `dcc-mcp-server sidecar` | Per-DCC sidecar worker. | Ensures the standalone gateway daemon, registers a `per-dcc-sidecar` row, and dispatches through host RPC. Runtime is implemented by `dcc-mcp-sidecar`. |
| `dcc-mcp-server translate` | External stdio MCP bridge. | Ensures the standalone gateway daemon and registers the bridge as a backend unless `--no-register` is set. |
| `dcc-mcp-server gateway` | Machine-wide gateway daemon. | Hosts discovery, routing, resources/prompts, admin, and audit without running DCC tools inline. |
| `dcc-mcp-server update check/apply` | Server binary update helper. | Reads the gateway update manifest on `127.0.0.1:<gateway-port>` and stages `dcc-mcp-server` for the next server launch. |

`auto` and `serve` share the server flags below. `gateway` has its own smaller
flag surface and rejects server-only flags such as `--app`.

### Core flags

| Flag | Env | Default | Meaning |
|---|---|---|---|
| `--mcp-port` | `DCC_MCP_MCP_PORT` | `0` | MCP Streamable HTTP port. `0` = OS-assigned. |
| `--ws-port` | `DCC_MCP_WS_PORT` | `9001` | WebSocket bridge port for non-Python DCC plugins. |
| `--app` | `DCC_MCP_APP` | `""` | App tag (`"maya"`, `"blender"`, `"photoshop"`, …). Feeds skill discovery + the registry row. |
| `--skill-paths` | — | `[]` | Additional skill search paths (repeatable). |
| `--server-name` | `DCC_MCP_SERVER_NAME` | `"dcc-mcp-server"` | Server name advertised to MCP clients. |
| `--no-bridge` | — | `false` | Disable the WebSocket bridge; MCP HTTP only. |
| `--host` | — | `127.0.0.1` | Host to bind to. |
| `--pid-file` | — | — | Write the server PID to this file while running. |
| `--force` | — | `false` | Overwrite an existing PID file even if it points at a live process. |
| `--shutdown-timeout-secs` | `DCC_MCP_SHUTDOWN_TIMEOUT_SECS` | `10` | Graceful shutdown deadline. |

### Auto-gateway flags (`auto` / `serve`)

| Flag | Env | Default | Meaning |
|---|---|---|---|
| `--gateway-port` | `DCC_MCP_GATEWAY_PORT` | `9765` | Well-known gateway port to ensure/register with. `0` disables gateway ensure/election for this process. |
| `--no-ensure-gateway` | — | `false` | Do not auto-launch the standalone gateway daemon before backend registration. |
| `--legacy-gateway-election` | `DCC_MCP_LEGACY_GATEWAY_ELECTION` | `false` | Restore the old embedded first-wins election path. |
| `--no-admin` | `DCC_MCP_NO_ADMIN` | `false` | Disable the Admin UI on the elected gateway. Admin is enabled by default when a process wins the gateway role. |
| `--admin-path` | `DCC_MCP_ADMIN_PATH` | `/admin` | URL prefix for the Admin UI and its JSON APIs. |
| `--registry-dir` | `DCC_MCP_REGISTRY_DIR` | `<temp>/dcc-mcp-registry` | shared `FileRegistry` directory used by CLI local mode, sidecars, and gateway runners. |
| `--stale-timeout-secs` | `DCC_MCP_STALE_TIMEOUT` | `30` | Seconds without heartbeat before an instance is considered stale. |
| `--app-version` | `DCC_MCP_APP_VERSION` | — | App version (e.g., `"2024.2"`); recorded in the registry. |
| `--scene` | `DCC_MCP_SCENE` | — | Currently-open scene / document; recorded in the registry, used by multi-instance disambiguation. |
| `--heartbeat-secs` | `DCC_MCP_HEARTBEAT_INTERVAL` | `5` | Heartbeat cadence in seconds. `0` disables. |

Admin audit/trace persistence is configured by environment only: set `DCC_MCP_GATEWAY_AUDIT_DIR` to a writable directory to persist `/admin/api/calls` rows in `audit.jsonl` and dispatch traces in `traces.jsonl`; set `DCC_MCP_GATEWAY_AUDIT_MAX_ROWS` (default `5000`) to cap each file.

> **Removed** — `--gateway-tool-exposure` /
> `DCC_MCP_GATEWAY_TOOL_EXPOSURE` are gone. The gateway surface is now
> unconditionally minimal (see `docs/guide/rest-api-surface.md`).
>
> **Removed** — `--gateway-cursor-safe-tool-names` /
> `DCC_MCP_GATEWAY_CURSOR_SAFE_TOOL_NAMES`. Aggregated gateway `prompts/list`
> always emits the cursor-safe `i_<id8>__<escaped>` wire form (#656).

### Standalone gateway flags (`gateway`)

| Flag | Env | Default | Meaning |
|---|---|---|---|
| `--daemon` | `DCC_MCP_DAEMON` | `false` | Respawn the current executable as a detached gateway child and exit the parent. Unix children start in a new session; Windows children use detached process flags. Respawn failures fail before the parent exits. |
| `--restart` | — | `false` | Restart a running gateway daemon. Reads the PID from `--pidfile`, gracefully stops the old process, waits for exit (up to 15 s), then spawns a fresh detached gateway and polls `/health` until ready. Requires `--pidfile`. Handles stale pidfiles (dead process): prints a warning, removes the stale pidfile, and spawns a fresh gateway. |
| `--pidfile PATH` | `DCC_MCP_PIDFILE` | — | Implies daemon mode. The pidfile records the detached child PID and is removed when that child exits cleanly. Pidfile write failures fail before the parent exits. |
| `--gateway-persist` | `DCC_MCP_GATEWAY_PERSIST` | `false` | Keep the gateway daemon alive with no registered backends. |
| `--gateway-idle-timeout-secs` | `DCC_MCP_GATEWAY_IDLE_TIMEOUT_SECS` | `30` | Seconds to wait after the last backend disappears before shutdown. `0` disables idle shutdown. |

Daemon auto-ensure paths pass a bounded idle timeout by default unless
`DCC_MCP_GATEWAY_IDLE_TIMEOUT_SECS` is set. The user-facing
`dcc-mcp-cli gateway daemon start` wrapper passes `0` by default so the
explicitly managed machine-wide daemon does not exit just because no DCC is
currently registered.

### File-logging flags

| Flag | Env | Default | Meaning |
|---|---|---|---|
| `--no-log-file` | `DCC_MCP_NO_LOG_FILE` | `false` | Disable the rotating file logger (stderr logging stays on). |
| `--log-dir` | `DCC_MCP_LOG_DIR` | platform default | Log file directory. |
| `--log-max-size` | `DCC_MCP_LOG_MAX_SIZE` | 10 MiB | Max bytes per log file before size-triggered rotation. |
| `--log-max-files` | `DCC_MCP_LOG_MAX_FILES` | `7` | How many rolled files to retain. |
| `--log-rotation` | `DCC_MCP_LOG_ROTATION` | `"both"` | Rotation policy: `size`, `daily`, `both`. |
| `--log-file-prefix` | `DCC_MCP_LOG_FILE_PREFIX` | `"dcc-mcp"` | Filename prefix. Full filename: `<prefix>.<pid>.<YYYYMMDD>.log`. |
| `--log-retention-days` | `DCC_MCP_LOG_RETENTION_DAYS` | `7` | Age-based retention. `0` disables. |
| `--log-max-total-size-mb` | `DCC_MCP_LOG_MAX_TOTAL_SIZE_MB` | `100` | Total directory cap in MiB. `0` disables. |

### Capture replay/diff

`dcc-mcp-server capture` works on offline traffic capture files produced by
`DCC_MCP_TRAFFIC_CAPTURE=jsonl:<path>` or `DCC_MCP_TRAFFIC_CONFIG=<yaml>`.
It never enables capture itself and does not need a live DCC unless you use
`replay`.

If the YAML config includes an `admin_live` sink, the retained in-memory window
can be downloaded as JSONL from `/admin/api/traffic/export` (or the stable
mirror `/v1/debug/traffic/export`) and then passed to `capture replay` or
`capture diff` like any other capture file.

```bash
# Replay recorded client -> gateway requests against a live gateway MCP endpoint.
dcc-mcp-server capture replay ./captures/run.sqlite \
    --target http://127.0.0.1:9765/mcp \
    --session sess_01HQX \
    --assert outputs-compatible

# Compare two captures frame-by-frame.
dcc-mcp-server capture diff ./captures/before.sqlite ./captures/after.sqlite \
    --before-session sess_before \
    --after-session sess_after
```

Replay assertion modes:

| Mode | Contract |
|---|---|
| `outputs-compatible` | HTTP status and JSON-RPC result/error shape must match the recorded response. |
| `outputs-equal` | HTTP status and response JSON must match exactly. |
| `outputs-ignored` | Requests are sent and counted, but response bodies are not compared. |

Use `--format jsonl` or `--format sqlite` when the filename extension is not
enough for auto-detection. `--rebind-instance-id <id>` rewrites captured
gateway tool slugs such as `maya.old.tool` plus `instance_id` fields so a
recording can be replayed against the current live instance.

### Typical invocations

```bash
# 1) Backwards-compatible auto mode (same as: dcc-mcp-server auto --app maya).
dcc-mcp-server --app maya

# 2) Per-DCC server only, never competing for the shared gateway port.
dcc-mcp-server serve --no-auto-gateway --app maya

# 3) Daemon-backed backend on a workstation with multiple DCCs.
#    Each process ensures the same gateway daemon and registers as a backend.
dcc-mcp-server auto --app maya --server-name maya-shotgun-alpha \
               --scene /shots/ep101/sh0200/shot.ma \
               --log-dir /var/log/dcc-mcp

# 4) Workstation-wide gateway daemon.
dcc-mcp-server gateway --host 127.0.0.1 --port 9765 \
                       --registry-dir /var/lib/dcc-mcp

# 4b) Same gateway as an explicit detached daemon.
dcc-mcp-server gateway --host 127.0.0.1 --port 9765 \
                       --registry-dir /var/lib/dcc-mcp \
                       --daemon --pidfile /var/run/dcc-mcp-gateway.pid

# 5) Bridge an external stdio MCP server behind the same daemon gateway.
dcc-mcp-server translate --stdio "uvx mcp-server-git" \
                         --app-type git --port 0
```

---

## `dcc-mcp-tunnel-relay`

Public-facing WebSocket relay that accepts registrations from local tunnel
agents and forwards multiplexed MCP sessions from remote AI assistants.

Build with `cargo build --bin dcc-mcp-tunnel-relay --features bin`.

| Flag | Env | Default | Meaning |
|---|---|---|---|
| `--jwt-secret-file` | `DCC_MCP_TUNNEL_RELAY_JWT_SECRET_FILE` | **required** | Path to a file containing the HS256 JWT secret. ≥32 bytes in production (`openssl rand -base64 48`). The file is read so the bytes never appear in `ps` output. |
| `--public-host` | `DCC_MCP_TUNNEL_RELAY_PUBLIC_HOST` | `localhost` | Public hostname embedded in minted tunnel URLs (ends up in the JWT `iss` claim). |
| `--base-url` | `DCC_MCP_TUNNEL_RELAY_BASE_URL` | `ws://localhost:9870` | WebSocket base URL; prepended to per-tunnel paths in `RegisterAck.public_url`. |
| `--agent-bind` | `DCC_MCP_TUNNEL_RELAY_AGENT_BIND` | `0.0.0.0:9870` | TCP bind for the agent control plane. |
| `--frontend-bind` | `DCC_MCP_TUNNEL_RELAY_FRONTEND_BIND` | `0.0.0.0:9871` | TCP bind for the remote-client frontend. |
| `--ws-frontend-bind` | `DCC_MCP_TUNNEL_RELAY_WS_FRONTEND_BIND` | — | Optional WebSocket frontend bind (`/tunnel/<id>` upgrade). Omit to disable. |
| `--admin-bind` | `DCC_MCP_TUNNEL_RELAY_ADMIN_BIND` | — | Optional read-only admin endpoint bind (`GET /tunnels`, `GET /healthz`). Omit to disable. |
| `--stale-timeout-secs` | `DCC_MCP_TUNNEL_RELAY_STALE_TIMEOUT_SECS` | `30` | Seconds without heartbeat before a tunnel is evicted from the registry. |
| `--max-tunnels` | `DCC_MCP_TUNNEL_RELAY_MAX_TUNNELS` | `0` | Hard cap on simultaneously-registered tunnels. `0` disables the cap. |

Shutdown: SIGINT / SIGTERM (or Ctrl+C on Windows) drains the accept loops
and waits for live sessions to close.

```bash
dcc-mcp-tunnel-relay \
    --jwt-secret-file /etc/dcc-mcp/tunnel-secret \
    --public-host relay.example.com \
    --base-url wss://relay.example.com \
    --agent-bind 0.0.0.0:9870 \
    --frontend-bind 0.0.0.0:9871 \
    --ws-frontend-bind 0.0.0.0:9880 \
    --admin-bind 127.0.0.1:9877
```

---

## `dcc-mcp-tunnel-agent`

Local sidecar that registers with a relay and bridges per-session traffic
to a local DCC MCP server. Keeps the connection alive across transient
failures with a configurable reconnect policy.

Build with `cargo build --bin dcc-mcp-tunnel-agent --features bin`.

| Flag | Env | Default | Meaning |
|---|---|---|---|
| `--relay-url` | `DCC_MCP_TUNNEL_AGENT_RELAY_URL` | **required** | Relay WebSocket URL (`wss://relay.example.com`). |
| `--token-file` | `DCC_MCP_TUNNEL_AGENT_TOKEN_FILE` | **required** | Path to the bearer JWT file (minted by `dcc_mcp_tunnel_protocol::auth::issue`). |
| `--dcc` | `DCC_MCP_TUNNEL_AGENT_DCC` | **required** | DCC tag this agent identifies with; must be in the JWT's `allowed_dcc` list. |
| `--local-target` | `DCC_MCP_TUNNEL_AGENT_LOCAL_TARGET` | **required** | Local MCP HTTP server address (`host:port`) to bridge to. |
| `--heartbeat-secs` | `DCC_MCP_TUNNEL_AGENT_HEARTBEAT_SECS` | `10` | Heartbeat cadence. Stay comfortably under the relay's `--stale-timeout-secs`. |
| `--reconnect-policy` | `DCC_MCP_TUNNEL_AGENT_RECONNECT_POLICY` | `exponential` | `constant` or `exponential`. |
| `--reconnect-initial-secs` | `DCC_MCP_TUNNEL_AGENT_RECONNECT_INITIAL_SECS` | `2` | Exponential: first-retry delay. |
| `--reconnect-max-secs` | `DCC_MCP_TUNNEL_AGENT_RECONNECT_MAX_SECS` | `60` | Exponential: hard cap on retry delay. |
| `--reconnect-constant-secs` | `DCC_MCP_TUNNEL_AGENT_RECONNECT_CONSTANT_SECS` | `5` | Constant: flat delay. |
| `--capabilities` | `DCC_MCP_TUNNEL_AGENT_CAPABILITIES` | `[]` | Comma-separated capability tags forwarded to remote clients via `/tunnels`. |

A non-retryable `Rejected` error (bad JWT, DCC-type mismatch) exits the
process with a non-zero code so supervisors don't restart-loop on a
misconfiguration.

```bash
dcc-mcp-tunnel-agent \
    --relay-url wss://relay.example.com \
    --token-file ~/.config/dcc-mcp/tunnel.jwt \
    --dcc maya \
    --local-target 127.0.0.1:8765 \
    --heartbeat-secs 10 \
    --reconnect-policy exponential \
    --reconnect-initial-secs 2 \
    --reconnect-max-secs 60
```

---

## Deployment scenarios

### Scenario 1 — Embedded in a DCC host

The Maya / Blender / Houdini plug-in loads `dcc_mcp_core` into the host's
Python interpreter and calls `create_skill_server()` directly. No external
binary involved. Most end-user deployments look like this.

See `examples/host_adapter_template.py` for the plug-in skeleton.

### Scenario 2 — Standalone per-DCC server

One `dcc-mcp-server` process per workstation launched by the DCC supervisor
or by a user autostart. Covers headless studios running things like
`mayapy` batch or a Python-only renderer that still wants to expose
capabilities via MCP + REST.

```bash
dcc-mcp-server --app maya --scene /shots/ep101/sh0200/shot.ma
```

### Scenario 3 — Gateway aggregating multiple DCC servers

Multiple `dcc-mcp-server` processes on the same workstation. The first one
binds the gateway port `9765` and indexes the others. Clients connect to
`127.0.0.1:9765/mcp` (or `/v1/*`), use MCP `search` / `describe` for
discovery, then execute through REST `/v1/call` or `/v1/call_batch` to reach
any DCC through one endpoint.

Example manifests: `examples/compose/gateway-ha/` and
`examples/k8s/gateway-ha/`.

### Scenario 4 — Remote relay + tunnel agent

The public relay runs on an operator-owned host; each artist workstation
runs an agent that registers with it. SaaS AI clients (Claude.ai, Cursor
desktop behind an enterprise firewall, etc.) connect to the relay's
frontend and get forwarded to the workstation's local MCP server.

```bash
# On the relay host (public internet):
dcc-mcp-tunnel-relay \
    --jwt-secret-file /etc/dcc-mcp/tunnel-secret \
    --public-host relay.example.com \
    --base-url wss://relay.example.com

# On the artist's workstation:
dcc-mcp-tunnel-agent \
    --relay-url wss://relay.example.com \
    --token-file ~/.config/dcc-mcp/tunnel.jwt \
    --dcc maya --local-target 127.0.0.1:8765
```

Mint JWTs with `dcc_mcp_tunnel_protocol::auth::issue`; scope them per
artist / per DCC via the `allowed_dcc` claim.

### Scenario 5 — CI / test harness

Integration tests spin up an in-process `McpHttpServer` and hit its
`/v1/*` endpoints directly. No external binary; no gateway.

See `crates/dcc-mcp-skill-rest/src/tests.rs` and
`crates/dcc-mcp-http/tests/http/` for reference patterns.

---

## Related reading

- [REST API surface](rest-api-surface.md) — the `/v1/*` contract.
- [Gateway diagnostics](gateway-diagnostics.md) — how to read logs + metrics when multiple servers contend for the gateway.
