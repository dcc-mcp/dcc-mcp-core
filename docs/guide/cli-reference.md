# CLI Reference

This repository ships four operator-facing binaries. This page is the
single source of truth for every flag, every environment variable, and the
five deployment scenarios they cover. Flags on each binary map 1:1 onto an
`DCC_MCP_*` environment variable, so any deployment manifest can drive the
same configuration surface.

`dcc-mcp-cli` and `dcc-mcp-server` are published as raw GitHub Release
assets on every release. The CLI can be installed directly from a URL:

```bash
curl -fsSL https://raw.githubusercontent.com/loonghao/dcc-mcp-core/main/scripts/install-cli.sh | bash

# Windows PowerShell
powershell -c "irm https://raw.githubusercontent.com/loonghao/dcc-mcp-core/main/scripts/install-cli.ps1 | iex"
```

Pin a release by setting `DCC_MCP_VERSION=v0.17.17` or passing
`--version v0.17.17` to the install script.

| Binary | Role | Source |
|---|---|---|
| [`dcc-mcp-cli`](#dcc-mcp-cli) | User/CI control plane for local or remote DCC-MCP REST endpoints. | `crates/dcc-mcp-cli/` |
| [`dcc-mcp-server`](#dcc-mcp-server) | Per-DCC MCP + REST server, with an integrated auto-gateway. | `crates/dcc-mcp-server/` |
| [`dcc-mcp-tunnel-relay`](#dcc-mcp-tunnel-relay) | Public-facing WebSocket relay for the zero-config remote tunnel (#504). | `crates/dcc-mcp-tunnel-relay/` |
| [`dcc-mcp-tunnel-agent`](#dcc-mcp-tunnel-agent) | Local sidecar that registers with the relay and forwards MCP traffic. | `crates/dcc-mcp-tunnel-agent/` |

Development helper binaries (`stub_gen`) are documented in
[`AGENTS.md`](https://github.com/loonghao/dcc-mcp-core/blob/main/AGENTS.md).

---

## `dcc-mcp-cli`

Client-side control plane for DCC-MCP. It does not host skills and does not
replace `dcc-mcp-server`; it knows how to talk to a local or remote gateway /
per-DCC REST endpoint and how to build auditable installation plans.

The default endpoint is `http://127.0.0.1:9765`, override it with
`--base-url` or `DCC_MCP_BASE_URL`.

```bash
dcc-mcp-cli list
dcc-mcp-cli health
dcc-mcp-cli search --query sphere --dcc-type maya --instance-id abc12345
dcc-mcp-cli describe maya.abc12345.create_sphere
dcc-mcp-cli load-skill workflow --dcc-type 3dsmax --instance-id 80321760
dcc-mcp-cli call maya.abc12345.create_sphere --json '{"radius":2}'
dcc-mcp-cli call maya_scene__get_session_info --dcc-type maya --instance-id abc12345 --json '{}'
dcc-mcp-cli wait-ready --dcc-type maya --instance-id abc12345 --require skill_catalog,host_execution_bridge
dcc-mcp-cli stop-instance --dcc-type maya --instance-id abc12345 --expected-owner release-smoke-test
dcc-mcp-cli install --dcc-type maya --version 2026
dcc-mcp-cli lint path/to/skills
```

### Commands

| Command | REST/API contract | Meaning |
|---|---|---|
| `health` | `GET /v1/healthz` | Check the configured endpoint. |
| `list` | `GET /v1/instances` | List live DCC instances from the gateway. |
| `search [--instance-id <id>]` | `POST /v1/search` | Search callable capabilities, optionally scoped to a full UUID or unique prefix. |
| `describe <tool-slug>` | `POST /v1/describe` | Inspect a capability before calling it. |
| `load-skill <skill-name> [--dcc-type <dcc>] [--instance-id <id>]` | `POST /v1/load_skill` | Activate a progressive skill and print its registered tools. |
| `call <tool-slug> --json <object>` | `POST /v1/call` | Invoke one capability. |
| `call <backend-tool> --dcc-type <dcc> --instance-id <id> --json <object>` | `POST /v1/dcc/{dcc}/instances/{id}/call` | Invoke a backend tool without constructing a dotted gateway slug. |
| `wait-ready [--dcc-type <dcc>] [--instance-id <id>] [--require <bits>]` | `GET /v1/instances` + per-instance `/v1/readyz` | Wait for smoke-test readiness bits such as `skill_catalog` or `host_execution_bridge`. |
| `stop-instance --dcc-type <dcc> --instance-id <id>` | `POST /v1/dcc/{dcc}/instances/{id}/stop` | Forward a guarded safe-stop request to instances that advertise `safe_stop_url`. |
| `install --dcc-type <dcc> [--version <v>]` | catalog-backed local plan | Resolve the matching adapter and emit an auditable install plan. |
| `lint [PATH ...]` | local filesystem validator | Recursively validate SKILL.md packages two levels below each path by default. |

`install` intentionally starts as a planning contract: it resolves catalog
entries and spells out the runtime / adapter / verification steps without
silently modifying DCC plugin folders. DCC-specific installers can attach to
that contract incrementally.

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

Standalone server runner with explicit run modes for per-DCC MCP servers and
the machine-wide gateway daemon. Invoking `dcc-mcp-server` with no subcommand
is still backwards compatible: it behaves exactly like `dcc-mcp-server auto`.

### Run modes

| Command | Role | Gateway behavior |
|---|---|---|
| `dcc-mcp-server` | Backwards-compatible implicit `auto`. | Starts a per-DCC MCP server and participates in first-wins gateway election. |
| `dcc-mcp-server auto` | Explicit form of the default behavior. | Same as the no-subcommand path. |
| `dcc-mcp-server serve` | Per-DCC MCP server. | Participates in first-wins gateway election unless told otherwise. |
| `dcc-mcp-server serve --no-auto-gateway` | Per-DCC MCP server only. | Registers/serves tools but never tries to bind the gateway port. |
| `dcc-mcp-server gateway` | Machine-wide gateway daemon. | Hosts discovery, routing, resources/prompts, admin, and audit without running DCC tools inline. |

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
| `--gateway-port` | `DCC_MCP_GATEWAY_PORT` | `9765` | Well-known port to compete for. `0` disables the gateway role entirely and therefore disables admin too. |
| `--no-admin` | `DCC_MCP_NO_ADMIN` | `false` | Disable the read-only Admin UI on the elected gateway. Admin is enabled by default when a process wins the gateway role. |
| `--admin-path` | `DCC_MCP_ADMIN_PATH` | `/admin` | URL prefix for the read-only Admin UI and its JSON APIs. |
| `--registry-dir` | `DCC_MCP_REGISTRY_DIR` | platform temp dir | shared `FileRegistry` directory. |
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

# 3) Gateway-winner on a workstation with multiple DCCs.
#    First terminal wins the gateway port, subsequent ones register as plain instances.
dcc-mcp-server auto --app maya --server-name maya-shotgun-alpha \
               --scene /shots/ep101/sh0200/shot.ma \
               --log-dir /var/log/dcc-mcp

# 4) Workstation-wide gateway daemon.
dcc-mcp-server gateway --host 127.0.0.1 --port 9765 \
                       --registry-dir /var/lib/dcc-mcp
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
