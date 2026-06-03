# Migrating Legacy Embedded Auto-Gateway to a Standalone Daemon

> **[中文版](../../zh/guide/migration/from-embedded-to-daemon)**

This guide is for older deployments, or adapters explicitly launched with
`--legacy-gateway-election`, that still use the embedded first-wins gateway.
Current `dcc-mcp-server` zero-config startup is already daemon-backed: a
per-DCC process ensures the machine-wide `dcc-mcp-server gateway` daemon and
then registers as a backend. The steps below are only needed when you are
retiring a legacy embedded topology or moving to a separately managed daemon /
multi-machine discovery source introduced by epic [#1367][epic].

[epic]: https://github.com/dcc-mcp/dcc-mcp-core/issues/1367

## When should you migrate?

Staying on legacy embedded auto-gateway is reasonable only if:

- You only run DCC tools on a single workstation, and
- All DCC processes (Maya, Blender, Houdini, …) are started on the same OS
  user and share the local `FileRegistry`, and
- You deliberately pass `--legacy-gateway-election` for compatibility with an
  older supervisor.

**Since v0.17, the default runtime mode is already daemon-backed.** The
embedded auto-gateway is now accessed via `--legacy-gateway-election`.
If you are already running the default (`auto` / `serve`) without the
legacy flag, you are using the three-layer architecture and this guide
is a reference for how it works under the hood — no migration needed.

Use explicit `dcc-mcp-server gateway` when:

- You run more than one workstation that should share a single gateway URL.
- The host that issues MCP calls (CI runner, headless agent, render farm
  scheduler) does **not** itself run any DCC.
- A DCC sits behind a NAT or firewall and must be reached from outside the
  LAN — that requires the relay-source path in Phase 3.
- You want to manage gateway uptime independently from any single DCC's
  lifetime (a restart of Maya should not blink the gateway URL for active
  agents).

## Step 0 — Take a legacy baseline snapshot

Before changing a legacy deployment, run the embedded flow once and capture the
state so you have something to compare against and roll back to:

```bash
# Start any DCC adapter (e.g. Maya) with the legacy embedded election path.
dcc-mcp-server --app maya --legacy-gateway-election

# In another terminal, list the live instances the gateway sees:
curl -s http://127.0.0.1:9765/v1/instances | jq '.by_source'
```

Save the output. After the migration the `by_source` counts should still
make sense (`file` may become `http`/`relay`/`mdns` depending on which
topology you adopt).

## Step 1 — Stop competing for the gateway port from legacy adapters

For same-workstation deployments, the simplest migration is to remove
`--legacy-gateway-election` and use the current default `auto` path. Each DCC
process will ensure the same standalone daemon and register as a backend:

```bash
dcc-mcp-server --app maya
dcc-mcp-server --app blender
```

If an external supervisor owns the daemon lifecycle and you only want DCC
processes to publish FileRegistry rows, disable daemon launch/guardian from the
adapter process:

```bash
dcc-mcp-server --app maya --no-ensure-gateway
dcc-mcp-server --app blender --no-ensure-gateway
```

`serve --no-auto-gateway` remains available for per-DCC-only launches: it sets
`gateway_port=0`, so the process never ensures, guards, or binds the gateway
port. With the `gateway-auto` feature enabled it still writes a FileRegistry
service row, which a separately managed same-machine daemon can read.

Roll back to the legacy topology only by re-adding `--legacy-gateway-election`.

## Step 2 — Start the standalone gateway daemon

Run the gateway as its own process. It is feature-gated so its binary
footprint is small (see #1359):

```bash
dcc-mcp-server gateway \
    --host 127.0.0.1 \
    --port 9765 \
    --registry-dir /var/lib/dcc-mcp
```

The daemon hosts **only** the gateway plane — discovery, routing, the
read-only admin UI, audit. It never executes a tool inline; every
`tools/call` is forwarded to the DCC backend that owns the tool. See
[`gateway.md` § Standalone gateway daemon][standalone] for the full
behavior contract.

[standalone]: ../gateway.md#standalone-gateway-daemon-1358

Roll back: stop the daemon and restart the adapters with
`--legacy-gateway-election`. See [Gateway Election → Failover
Diagnostics][failover-diag] for how to inspect that legacy state.

[failover-diag]: ../gateway-election.md#failover-diagnostics-issue-1355

## Step 3 — Pick a discovery source

The daemon exposes four discovery sources that all collapse into the
same `gateway://instances` shape. Pick the one(s) that match your
topology:

| Source | Use when | Configure on |
|--------|----------|--------------|
| `file` | Daemon and DCCs run on the same machine + user. | nothing — `FileRegistry` is automatic |
| `http` | DCC is on another machine, can reach the daemon over HTTPS. | DCC sidecar: `POST /v1/instances/register` + heartbeat |
| `mdns` | Same LAN, zero shared config. | `serve --advertise-mdns` + `gateway --discover-mdns` (build with `--features mdns`) |
| `relay` | DCC is behind NAT / firewall. | `tunnel-agent` → `tunnel-relay`, then `gateway --relay-source ADMIN=PUBLIC` |

Conflict precedence (#1364): `http > relay > mdns > file`. The most
recently re-asserted source for an `instance_id` wins.

## Step 4 — Lock down the daemon

Once a backend can join from outside the local trust boundary, you need
auth. Token-scoped registration and `allowed_dcc` enforcement are
tracked under [#1365][1365]; once that lands the daemon will require a
bearer token on every cross-host source. Until then the daemon is safe
**only** on a trusted local network or behind a reverse proxy that
terminates auth in front of it.

[1365]: https://github.com/dcc-mcp/dcc-mcp-core/issues/1365

## Step 5 — Verify and keep a rollback handy

```bash
# 1. The daemon is the only listener on the gateway port.
curl -s http://127.0.0.1:9765/v1/health

# 2. Every previously-visible DCC still shows up, possibly under a
#    different `source`.
curl -s http://127.0.0.1:9765/v1/instances | jq '.by_source'

# 3. Gateway metadata reports the intended mode:
#    "daemon-backed" for default auto-launch,
#    "failover_disabled_by_adapter" for --no-ensure-gateway, or
#    "gateway_port_not_configured" for --no-auto-gateway / gateway_port=0.
```

Rollback recipe in one block:

```bash
# Stop the daemon.
pkill -f "dcc-mcp-server gateway"

# Restart every DCC adapter with the legacy election flag.
dcc-mcp-server --app maya --legacy-gateway-election
```

## Daemon environment reference

| Variable | Default | Purpose |
|----------|---------|---------|
| `DCC_MCP_GATEWAY_GUARDIAN_INTERVAL` | `5` | Seconds between daemon `/health` probes |
| `DCC_MCP_GATEWAY_GUARDIAN_TIMEOUT` | `0.5` | Per-probe `/health` timeout in seconds |
| `DCC_MCP_GATEWAY_GUARDIAN_FAILURES` | `2` | Consecutive failed probes before re-ensure |
| `DCC_MCP_GATEWAY_LAUNCH_LOCK_STALE_SECS` | `30` | Age after which a stale launch lock is reclaimed by another backend |
| `DCC_MCP_GATEWAY_PERSIST` | `false` | Keep daemon alive when no backends remain |
| `DCC_MCP_GATEWAY_IDLE_TIMEOUT_SECS` | `30` | Grace period (seconds) before daemon shutdown after last backend exits; `0` = never |

## Related reading

- [`docs/guide/gateway.md`](../gateway.md) — run-mode reference, topology
  diagram, discovery payload shape.
- [`docs/guide/gateway-election.md`](../gateway-election.md) — failover
  state machine and diagnostics tool.
- [`docs/guide/tunnel-relay.md`](../tunnel-relay.md) — relay-source setup
  for NAT / cross-subnet topologies.
- [`docs/guide/cli-reference.md`](../cli-reference.md) — `auto` / `serve`
  / `gateway` subcommand flags.
