# ADR 012: Use OS-assigned ports for DCC instances

## Status

Accepted

## Context

Embedded DCC adapters historically copied the same fixed instance port. Two
applications, or two instances of one application, then raced for that port.
Per-adapter fallback numbers only move the collision and require clients to
know product-specific constants.

The shared HTTP server already supports binding `127.0.0.1:0`, exposes the
actual address on `McpServerHandle`, and registers that address in
FileRegistry after `bind()` succeeds. The CLI and gateway discover instances
from that registry, so a fixed per-instance port is not a discovery contract.

The gateway is different: it is the stable local control-plane endpoint used
by configured clients and first-wins election. Across the existing public API,
`gateway_port=0` means disabled.

## Decision

DCC instance HTTP servers default to port `0`, letting the OS assign an
available loopback port atomically. Startup must publish only the actual port
returned by `listener.local_addr()`; code must not probe a free port and bind
it later.

`DccServerOptions.from_env()` resolves instance ports in this order:

1. an explicit `port` argument, including explicit `0`;
2. `DCC_MCP_<DCC>_PORT`; and
3. `0`.

Adapter constructors and factory wrappers use `port=None` when the caller did
not provide an override. They must not use truthiness expressions such as
`port or DEFAULT_PORT`, because those discard explicit `0`.

The gateway continues to default to port `9765`. Changing its bootstrap and
election semantics requires a separate superseding ADR; `gateway_port=0`
continues to disable it.

## Consequences

### Positive

- Multiple DCCs and multiple same-type instances start without coordinating
  endpoint ports.
- The OS owns allocation, avoiding bind-after-probe races.
- CLI and gateway discovery remain the single routing contract.
- Operators can still request a stable direct endpoint with an argument or
  environment variable.

### Negative

- Clients that bypass FileRegistry and the gateway must read the handle/log or
  configure a fixed port.
- Existing adapters must remove their local fixed defaults to adopt the
  standard.

### Neutral

- The gateway remains a stable control-plane endpoint on `9765`.
- Binding remains loopback-only by default.

## Non-functional requirements

- **Reliability:** registration contains the listener's actual non-zero port.
- **Concurrency:** two same-type instances started without a port receive
  distinct endpoints and are both discoverable.
- **Security:** the default bind address remains `127.0.0.1`.
- **Compatibility:** explicit integer ports and DCC-specific environment
  overrides continue to work; explicit `0` is preserved.

## Failure modes and mitigations

| Failure mode | Mitigation |
| --- | --- |
| Adapter replaces explicit `0` with a fixed default | Core option precedence test and creator guide |
| Registry advertises `0` or a stale guess | Register only after bind using `local_addr()` |
| Two instances receive the same endpoint | Kernel-held listeners plus concurrent integration test |
| A direct client cannot predict the endpoint | Discover through CLI/gateway or set an explicit port |
| Gateway bootstrap becomes undiscoverable | Keep the stable `9765` control-plane default |

## Alternatives considered

### Reserve a different fixed port for every DCC

Rejected. It requires a global allocation table, still collides between two
instances of one DCC, and makes clients depend on adapter constants.

### Probe a free port before server startup

Rejected. Closing the probe socket before the real bind creates a TOCTOU race.

### Randomize the gateway port too

Rejected for this decision. The gateway is the bootstrap endpoint and port
`0` currently means disabled; changing both contracts together would make the
instance-port migration larger and less safe.

## References

- `McpHttpServer::start` in `crates/dcc-mcp-http/src/server/mod.rs`
- `DccServerOptions` in `python/dcc_mcp_core/_server/options.py`
- `skills/dcc-mcp-creator/references/ADAPTER_WORKFLOW.md`
