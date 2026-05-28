# Migrating from Embedded Auto-Gateway to a Standalone Daemon

> **[中文版](../../zh/guide/migration/from-embedded-to-daemon)**

This guide walks you through moving a working zero-config single-workstation
setup (embedded auto-gateway) to one of the supported multi-process / multi-
machine topologies introduced by epic [#1367][epic]. The default zero-config
flow is **not** going away — every step here is opt-in.

[epic]: https://github.com/loonghao/dcc-mcp-core/issues/1367

## When should you migrate?

Stay on the embedded auto-gateway if:

- You only run DCC tools on a single workstation, and
- All DCC processes (Maya, Blender, Houdini, …) are started on the same OS
  user and share the local `FileRegistry`.

Migrate to a standalone gateway daemon when **any** of the following is
true:

- You run more than one workstation that should share a single gateway URL.
- The host that issues MCP calls (CI runner, headless agent, render farm
  scheduler) does **not** itself run any DCC.
- A DCC sits behind a NAT or firewall and must be reached from outside the
  LAN — that requires the relay-source path in Phase 3.
- You want to manage gateway uptime independently from any single DCC's
  lifetime (a restart of Maya should not blink the gateway URL for active
  agents).

## Step 0 — Take a baseline snapshot

Before changing anything, run the embedded flow once and capture the
state so you have something to compare against and roll back to:

```bash
# Start any DCC adapter (e.g. Maya) normally so the embedded auto-gateway
# binds the well-known port.
dcc-mcp-server --app maya

# In another terminal, list the live instances the gateway sees:
curl -s http://127.0.0.1:9765/v1/instances | jq '.by_source'
```

Save the output. After the migration the `by_source` counts should still
make sense (`file` may become `http`/`relay`/`mdns` depending on which
topology you adopt).

## Step 1 — Stop competing for the gateway port from embedded adapters

The first reversible change is to tell every DCC adapter to **never** bid
for the gateway port. This lets you operate the gateway daemon out of
band while keeping the same registry, scenes, and skill paths.

```bash
# Every DCC sidecar / plugin host launcher gets the new flag:
dcc-mcp-server serve --no-auto-gateway --app maya
dcc-mcp-server serve --no-auto-gateway --app blender
```

`serve --no-auto-gateway` is a strict subset of `auto` — it never tries
to bind `--gateway-port`. The DCC still registers itself with whichever
gateway is reachable (whether embedded in a peer, or standalone).

Roll back: drop the `--no-auto-gateway` flag and the adapter rejoins
first-wins election.

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

Roll back: stop the daemon. Embedded adapters will detect the missing
gateway sentinel on their next election tick and (if they did **not**
have `--no-auto-gateway`) take over. See [Gateway Election → Failover
Diagnostics][failover-diag] for how to inspect that state.

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

[1365]: https://github.com/loonghao/dcc-mcp-core/issues/1365

## Step 5 — Verify and keep a rollback handy

```bash
# 1. The daemon is the only listener on the gateway port.
curl -s http://127.0.0.1:9765/v1/health

# 2. Every previously-visible DCC still shows up, possibly under a
#    different `source`.
curl -s http://127.0.0.1:9765/v1/instances | jq '.by_source'

# 3. The failover diagnostic on each embedded adapter reports
#    "failover_disabled_by_adapter" (because of --no-auto-gateway) or
#    "gateway_port_not_configured" — both are stable, expected states.
```

Rollback recipe in one block:

```bash
# Stop the daemon.
pkill -f "dcc-mcp-server gateway"

# Restart every DCC adapter without --no-auto-gateway. The first one up
# wins the gateway port and the topology is back to single-workstation
# embedded auto-gateway.
dcc-mcp-server --app maya
```

## Related reading

- [`docs/guide/gateway.md`](../gateway.md) — run-mode reference, topology
  diagram, discovery payload shape.
- [`docs/guide/gateway-election.md`](../gateway-election.md) — failover
  state machine and diagnostics tool.
- [`docs/guide/tunnel-relay.md`](../tunnel-relay.md) — relay-source setup
  for NAT / cross-subnet topologies.
- [`docs/guide/cli-reference.md`](../cli-reference.md) — `auto` / `serve`
  / `gateway` subcommand flags.
