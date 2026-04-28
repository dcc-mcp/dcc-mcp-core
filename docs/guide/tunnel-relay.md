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

## Frontend client wire format

Until the HTTP / WSS frontends land (PR 4 of #504), remote clients use
plain TCP and a 2-byte length-prefixed tunnel id as the very first
payload:

```text
[u16 BE: tunnel_id_len][tunnel_id_bytes][... raw MCP traffic ...]
```

Helper exposed for tests + SDKs:

```rust
use dcc_mcp_tunnel_relay::data::write_select_tunnel;
write_select_tunnel(&mut tcp_stream, &tunnel_id).await?;
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

## What's not (yet) in the MVP

| Capability | Status |
|---|---|
| Plain TCP transport (agent + frontend) | ✅ MVP |
| JWT auth + DCC scoping | ✅ MVP |
| 1:N session multiplexing per tunnel | ✅ MVP |
| Periodic eviction | ✅ MVP |
| WebSocket Secure transport (browser-friendly) | follow-up |
| HTTP+SSE frontend with `/dcc/<name>/<tunnel_id>` routing | follow-up |
| `/tunnels` listing endpoint + admin metrics | follow-up |
| Reconnect-with-back-off on the agent | follow-up |

The MVP is good enough to validate the protocol end-to-end and to
unblock dependent work (issue #504 acceptance criteria 1-5).

## See also

- `crates/dcc-mcp-tunnel-relay/tests/e2e.rs` — runnable example covering
  successful round-trip + token rejection.
- [`docs/guide/agents-reference.md`](agents-reference.md) for trap rules
  and conventions.
