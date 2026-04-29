# Remote MCP Tunnel Relay

Zero-config remote access to a DCC's local MCP server, without opening
inbound firewall holes on the workstation. Tracked in issue #504.

## When to use it

Reach for the relay when you need an off-machine agent (cloud LLM,
orchestrator on another host, web client) to talk to a DCC running
behind NAT or a corporate firewall. Inside a single LAN keep using
`McpHttpConfig(host="0.0.0.0", port=8765)` directly — it's one fewer hop.

## Architecture

```
┌──────────────┐      WSS / TCP       ┌────────────┐      TCP      ┌─────────────┐
│ Local Agent  │ ───── Register ────► │ Relay      │ ◄──── select  │ Remote MCP  │
│ (in DCC)     │ ◄─── OpenSession ─── │ (public)   │   tunnel id   │ Client      │
│              │ ◄─── Data ─────────► │  registry  │ ────────────► │             │
│              │ ──── Heartbeat ─────► │  + sweeper │               │             │
└──────┬───────┘                       └────────────┘               └─────────────┘
       │ TCP
       ▼
┌──────────────┐
│ DCC HTTP MCP │
│ (localhost)  │
└──────────────┘
```

Three crates collaborate:

| Crate | Role |
|---|---|
| `dcc-mcp-tunnel-protocol` | Frame format (msgpack), JWT auth, codec |
| `dcc-mcp-tunnel-relay` | Public-facing server: agent + frontend listeners, registry, eviction |
| `dcc-mcp-tunnel-agent` | Local sidecar: registers, multiplexes per-session bytes to the local DCC |

## Minimal end-to-end example (Rust)

```rust
use std::time::Duration;
use dcc_mcp_tunnel_protocol::{auth, TunnelClaims};
use dcc_mcp_tunnel_relay::{RelayConfig, RelayServer};
use dcc_mcp_tunnel_agent::{AgentConfig, run_once};

# async fn demo() -> anyhow::Result<()> {
let secret = b"swap-me-with-a-real-32B-secret-please";

// 1. Operator stands up the relay (e.g. on a public VM).
let relay = RelayServer::start(
    RelayConfig {
        jwt_secret: secret.to_vec(),
        public_host: "relay.example.com".into(),
        base_url: "wss://relay.example.com".into(),
        stale_timeout: Duration::from_secs(45),
        max_tunnels: 0,
    },
    "0.0.0.0:9001".parse()?, // agent listener
    "0.0.0.0:9002".parse()?, // frontend listener
).await?;

// 2. Operator mints a per-DCC JWT and ships it to the workstation.
let token = auth::issue(&TunnelClaims {
    sub: "studio-bob".into(), iat: 0, exp: u64::MAX,
    iss: "studio-issuer".into(),
    allowed_dcc: vec!["maya".into()],
}, secret)?;

// 3. Agent runs in the DCC process (or a sibling sidecar).
let cfg = AgentConfig::new(
    "relay.example.com:9001",
    &token,
    "maya",
    "127.0.0.1:8765", // local DCC MCP HTTP server
);
let registered = run_once(cfg).await?;
println!("public URL: {:?}", registered.public_url);
# Ok(()) }
```

## Frontend transports

Two ways for a remote client to attach to a registered tunnel.

### 1. Plain TCP (raw byte stream)

Connect to the relay's frontend port and send a 2-byte length-prefixed
tunnel id as the very first payload, then start exchanging raw MCP
traffic:

```text
[u16 BE: tunnel_id_len][tunnel_id_bytes][... raw MCP traffic ...]
```

Helper exposed for tests + SDKs:

```rust
use dcc_mcp_tunnel_relay::data::write_select_tunnel;
write_select_tunnel(&mut tcp_stream, &tunnel_id).await?;
```

### 2. WebSocket (browser- and proxy-friendly)

Enable the WS frontend by populating
[`OptionalBinds::ws_frontend`](https://docs.rs/dcc-mcp-tunnel-relay) on
the relay, then connect to:

```text
ws://<host>:<ws_port>/tunnel/<tunnel_id>
```

Each binary WS message becomes one MCP payload in either direction.
Text frames are ignored (the wire is binary). For TLS, terminate at a
reverse proxy (`nginx` / `caddy` / cloud LB) — the relay itself speaks
plain HTTP/1.1, mirroring how `dcc-mcp-http` is deployed.

## Admin endpoint (`/tunnels`)

When [`OptionalBinds::admin`](https://docs.rs/dcc-mcp-tunnel-relay) is
populated, the relay exposes a read-only HTTP surface on a separate
port:

| Path | Returns |
|---|---|
| `GET /tunnels` | JSON array of [`TunnelSummary`] rows (one per live tunnel) |
| `GET /healthz` | `200 OK` `"ok"` while the process is up |

Example:

```bash
curl -s http://relay.example.com:9003/tunnels | jq
# [{
#   "tunnel_id": "01J…",
#   "dcc": "maya",
#   "capabilities": ["scene.read"],
#   "agent_version": "dcc-mcp-tunnel-agent/0.14",
#   "registered_at_ms_ago": 31204,
#   "last_heartbeat_ms_ago": 1450,
#   "session_count": 2
# }]
```

The endpoint is mutation-free; firewall it to your operator network
because it leaks the live tunnel id list.

## Agent reconnect & back-off

For long-lived agents use
[`run_with_reconnect`](https://docs.rs/dcc-mcp-tunnel-agent) instead of
[`run_once`]. It honours
[`AgentConfig::reconnect`](https://docs.rs/dcc-mcp-tunnel-agent), which
defaults to exponential back-off (1 s → 60 s, doubling). A successful
registration resets the delay to `initial`; a `RegisterAck { ok: false }`
returns [`ReconnectExit::Fatal`] without retrying so a misconfigured
JWT fails fast.

```rust
use tokio::sync::watch;
let (shutdown_tx, shutdown_rx) = watch::channel(false);
let task = tokio::spawn(dcc_mcp_tunnel_agent::run_with_reconnect(cfg, shutdown_rx));
// ... later ...
shutdown_tx.send(true)?;
let _ = task.await;
```

## Authentication & scoping

Every agent registration carries a JWT signed with `RelayConfig::jwt_secret`.
The token's `allowed_dcc` claim **caps** which DCC tags the agent may
register under — the relay rejects mismatches with `ErrorCode::DccNotAllowed`.
Tokens are expected to be short-lived (minutes to hours); rotation is
the operator's job.

## Stale tunnel eviction

The relay sweeps the registry every 15 s by default. Any tunnel whose
last heartbeat is older than `RelayConfig::stale_timeout` is dropped,
which closes its outbound queue and tears down per-tunnel tasks. Active
sessions on an evicted tunnel see a TCP RST.

## Capability matrix

| Capability | Status |
|---|---|
| Plain TCP transport (agent + frontend) | ✅ shipped |
| JWT auth + DCC scoping | ✅ shipped |
| 1:N session multiplexing per tunnel | ✅ shipped |
| Periodic eviction | ✅ shipped |
| WebSocket frontend (`ws://` — pair with reverse-proxy TLS for `wss://`) | ✅ shipped |
| `/tunnels` + `/healthz` admin endpoint | ✅ shipped |
| Reconnect-with-back-off on the agent | ✅ shipped |
| HTTP+SSE frontend with `/dcc/<name>/<tunnel_id>` routing | follow-up |
| Built-in TLS termination on the relay itself | follow-up — defer to ops |
| Production benchmark (latency p50/p99 vs direct TCP) | follow-up |

## See also

- `crates/dcc-mcp-tunnel-relay/tests/e2e.rs` — runnable example covering
  successful round-trip + token rejection.
- [`docs/guide/agents-reference.md`](agents-reference.md) for trap rules
  and conventions.
