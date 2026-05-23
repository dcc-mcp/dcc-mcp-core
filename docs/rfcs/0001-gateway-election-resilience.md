# RFC 0001 - Gateway Election Resilience

**Status**: Draft
**Target repo**: `dcc-mcp-core`
**Authors**: dcc-mcp-core contributors
**Date**: 2026-05-23
**Related code**: `dcc_mcp_gateway::gateway::runner`, `dcc_mcp_transport::discovery::file_registry`, `dcc_mcp_http::server`, `dcc_mcp_http_server::session`

---

## Summary

Today, when a second DCC instance starts and wins the gateway election, the
process that was previously serving port 9765 yields, the port briefly
closes, and the new process binds it. Long-lived MCP clients (Cursor,
VS Code, custom agents) **lose their connection mid-session** with no
in-band recovery signal, and a stateless backend has no way to resume.

This RFC proposes a set of independently shippable changes to `dcc-mcp-core`
that make gateway transitions either **invisible to clients** or **cleanly
recoverable**, without adding new processes, new packaging artifacts, or
DCC-specific code paths.

## Motivation - two scenarios that today share one mechanism

Election today is **version-driven preemption**: whichever adapter has the
higher `dcc_mcp_core` version wins, and the previous owner cooperatively
yields. This single mechanism is asked to serve two very different
scenarios with opposite client-facing requirements:

### Scenario 1 - startup of a co-existing DCC (`disconnect == bug`)

A studio user already has Maya open with an active agent session. They
start a second Maya for a parallel scene. The second Maya's adapter
finds the current gateway "outranked" and triggers a handover, taking
down the first user's MCP session as collateral damage:

```text
2026-05-22T19:36:08  INFO  Registered in FileRegistry instance=ff70b208-...
2026-05-22T19:36:08  INFO  We outrank the current gateway - entering challenger mode
                              own=0.17.21 own_adapter_dcc=None
                              gateway=0.3.7 gateway_adapter_dcc=Some("maya")
2026-05-22T19:36:08  INFO  Cooperative yield accepted - waiting for port to free up
2026-05-22T19:36:08  INFO  Won gateway election version=0.3.7
2026-05-22T19:36:08  INFO  gateway SSE: backend in stateless mode
                              - no MCP-Session-Id / __session_id
2026-05-22T19:36:08  INFO  gateway SSE: stateless MCP backend
                              - SSE subscription unavailable; parked
```

The MCP client (Cursor) sees only `"Server is not ready"` thereafter.
**This is the bug.**

### Scenario 2 - owner adapter crashes (`failover == feature`)

A Maya holding the gateway dies hard - segfault, OOM, force-kill, host
sleep. Without something to take over, port 9765 has no listener and the
whole studio's agent fleet is offline until someone notices. Today the
crash is "rescued" only because **any** newer DCC startup will preempt
and rebind the port - i.e. failover happens as a side effect of the
preemption rule that breaks Scenario 1.

**These two scenarios need different decisions and different signals.**
Disconnect-on-coexist is purely the wrong default; disconnect-on-crash-
recovery is the correct trade-off but is currently triggered by the
wrong condition (version outranks) instead of the right one (peer
unresponsive).

## Hard constraints

These constraints are typical of
any in-DCC Python ecosystem:

1. **No new processes.** The current "every DCC adapter ships its own
   embedded HTTP server, and one of them happens to also host the gateway"
   model deploys cleanly through rez / thm / per-host plugin managers.
   Introducing a separate `gateway` daemon, sidecar proxy, systemd unit,
   or Windows service breaks studio packaging and asks operators to manage
   a new lifecycle.
2. **rez packaging must stay flat.** Any new artifact (extra rez package,
   extra entry point, extra environment variable) multiplies the number
   of packages a DCC `package.py` has to `requires`. Prefer changes that
   live inside `dcc-mcp-core` and propagate via existing dependencies.
3. **Multi-DCC by construction.** Maya, Blender, 3ds Max, Houdini,
   Photoshop, ZBrush, Unreal, Unity - and the long-tail of studio
   integrations on top - all share the gateway. Every change must be
   implemented in `dcc-mcp-core` and inherited; no per-adapter changes
   should be required to opt in to resilience.
4. **Backward compatible.** Old `dcc-mcp-*` adapters and old MCP clients
   must keep working unchanged. Resilience features are negotiated.

## Non-goals

- Hot binary upgrade of an in-flight DCC adapter (the DCC process itself
  is still ephemeral).
- High-availability gateway across hosts (everything here is on
  localhost; remote LAN clients keep using the existing 0.0.0.0 listener).
- Reimplementing the MCP wire protocol.

---

## Design

Four independent components. Each is shippable on its own and brings
visible value; together they remove the disconnect class entirely.

### A. Liveness-driven election with automatic failover

The right decision for an arriving adapter is **not** "am I higher
version", it is **"is the current owner actually serving traffic right
now?"**. Co-existence and failover are both correctly answered by that
single question.

**Mechanism: lease + heartbeat + active probe.**

The gateway owner publishes a lease in `FileRegistry`:

```jsonc
// services.json (extended)
{
  "gateway_lease": {
    "instance_id": "1f363976-dcf9-4e35-aa5a-b854b47b1ec2",
    "endpoint":    "http://127.0.0.1:9765/mcp",
    "version":     "0.17.21",
    "renewed_unix_secs": 1779478211,
    "ttl_secs":         5
  }
}
```

The owner renews `renewed_unix_secs` every `ttl_secs / 3` seconds (1.6 s
default) - same write path the registry already uses for adapter
heartbeats.

**Arriving adapter decision tree (replaces the current
`version_outranks` branch):**

1. Read `gateway_lease`.
2. **If lease is fresh** (`now - renewed <= ttl_secs`):
   - **Active probe**: `GET http://<endpoint>/health` with a 2 s
     timeout.
   - Probe **200 OK** -> owner is alive and healthy. **Skip election.**
     Register only as an instance and return; the existing gateway will
     route to us via its FileRegistry watcher (already implemented).
   - Probe **fails / times out / 5xx** -> owner is hung. Continue to
     step 4 (failover path).
3. **If lease is stale** (`now - renewed > ttl_secs`):
   - Owner missed heartbeats - likely crashed. Still active-probe once
     (cheap) to avoid racing a temporary GC pause; on failure continue.
4. **Failover path** (formerly the "preemption" path):
   - Atomically CAS the `gateway_lease` to ourselves (FileRegistry
     already serializes writes via `services.lock`). If CAS loses, some
     other arrival won the race - re-read and goto 1.
   - Wait `socket_takeover_grace_ms` (default 500) for the dead owner's
     OS socket to release (some platforms hold TIME_WAIT).
   - Bind 9765 and start serving.
   - Log clearly: `"Promoted to gateway after liveness failure of
     <prev_instance> (last_renewed=...s ago, probe=<err>)"`.

**Version-driven preemption** is *not* part of this default path. It
remains available as an explicit opt-in for the rare rolling-upgrade
case:

```yaml
# gateway_policy.yaml - defaults are conservative
election:
  default: liveness_driven    # Do not preempt healthy owners; take over failed owners.
  lease_ttl_secs: 5
  lease_renew_interval_secs: 1.6
  probe_timeout_secs: 2
  socket_takeover_grace_ms: 500

  # Opt-in: rolling upgrades. Off by default to protect interactive sessions.
  allow_version_preempt:
    enabled: false
    when:
      component: dcc_mcp_core
      bump: minor             # 0.17 -> 0.18 may preempt; 0.17.21 -> 0.17.22 may not
```

**Why this satisfies both scenarios:**

- **Co-existing DCC starts.** Today: always preempts -> client drop.
  Liveness-driven: probe passes -> silent register, no drop.
- **Owner crashes.** Today: rescued only if a newer DCC happens to
  start. Liveness-driven: detected within `ttl_secs`, *any* peer adapter
  can promote.
- **Owner hung** (Maya UI deadlock, long GC, kernel pause). Today:
  same as crash, rescue is accidental. Liveness-driven: probe fails ->
  controlled failover with explicit `Promoted` log line.
- **Genuine rolling upgrade.** Today: always preempts. Liveness-driven:
  opt-in via `allow_version_preempt` (off by default).

### A.event - Pre-shutdown handoff event (graceful-exit fast path)

Lease+heartbeat above gives ~`ttl_secs` MTTR for crashes. For the very
common case of an *intentional* exit (user closes Maya, `kill -TERM`,
planned upgrade, idle scheduler eviction) the owner has full opportunity
to **actively announce** before going down. This collapses MTTR from
seconds to tens of milliseconds and avoids any window where 9765 is
unowned.

**Mechanism: a single record in `services.json` + a single SSE
notification, both atomic, both fire-and-forget.**

When the gateway owner decides to shut down (or receives a shutdown
signal), before closing its listener it performs **one write** to
`FileRegistry` and **one broadcast** on its SSE stream:

```jsonc
// services.json - gateway_handoff is a transient record, GC'd by the
// next successor on promotion or by the registry sweep after `until`.
{
  "gateway_handoff": {
    "from_instance_id": "1f363976-...",
    "endpoint":         "http://127.0.0.1:9765/mcp",
    "reason":           "graceful_shutdown",
                        // | "explicit_release" | "scheduled_upgrade"
    "issued_unix_secs":  1779478215.0,
    "deadline_unix_secs": 1779478220.0,   // owner promises to stay up
                                          // until this point (5s grace)
    "in_flight_calls":   2,
    "subscribed_clients": 3,
    "suggested_successor": "4a3c0197-..."   // optional - see A.standby
  }
}
```

```jsonc
// SSE notification on every active /mcp subscription
{
  "jsonrpc": "2.0",
  "method":  "notifications/gateway/handoff",
  "params": {
    "from":     "1f363976-...",
    "deadline_unix_secs": 1779478220.0,
    "reason":   "graceful_shutdown",
    "endpoint_after_handoff_will_be_same": true
  }
}
```

**Two consumers, both already wired:**

- **Peer adapters** subscribe to FileRegistry change events today (used
  for instance discovery) - the new `gateway_handoff` field arrives via
  the same callback. The first peer to observe it CAS's the
  `gateway_lease` to itself, binds the port (the outgoing owner is still
  holding it until `deadline`, so the bind succeeds via `SO_REUSEPORT`
  on Linux, or via the cooperative-handoff window on Windows), and
  starts serving. Once the new owner reports `bound`, the outgoing
  owner releases. Total client-visible gap: typically &lt; 50 ms.
- **MCP clients** subscribed to `/mcp` SSE receive
  `notifications/gateway/handoff` and learn the deadline. Smart clients
  hold their request queue for the grace window then retry in place
  (since `endpoint_after_handoff_will_be_same=true` for the
  "everyone shares 9765" deployment); naive clients fall through to
  reconnect-on-error, no worse than today.

**Interaction with the heartbeat path (A) - they are dual:**

- **Graceful shutdown / SIGTERM** -> detected by A.event (this section),
  MTTR ~50 ms.
- **Hard crash, kill -9, host sleep** -> detected by A (lease expiry +
  probe), MTTR ~`ttl_secs`.
- **Slow hang (UI thread deadlock)** -> detected by A (probe fails
  before lease expires), MTTR ~probe timeout.
- **Planned upgrade with version bump** -> A.event when the outgoing
  owner cooperates, plus opt-in `allow_version_preempt` as fallback;
  MTTR ~50 ms.

The two mechanisms write the **same** `gateway_lease` afterwards, so
peers don't need to know which path triggered the change - they just
re-read the lease.

**Public API (so studio code can request a handoff explicitly):**

```python
# dcc_mcp_core.gateway
def request_release(reason: str = "explicit_release",
                    grace_secs: float = 5.0,
                    suggested_successor: str | None = None) -> dict:
    """Announce a handoff and return the result.

    Useful when the embedding DCC tool wants to gracefully drop its
    gateway role - e.g. a `before_quit` hook in Maya / Blender that
    fires before plugin unload.
    """
```

**Constraints check**: [ok] no new process (uses existing FileRegistry
write + existing SSE channel) ; [ok] flat rez (one symbol added to
`dcc-mcp-core`) ; [ok] multi-DCC (announce path is DCC-agnostic; only
`reason` strings differ per integration) ; [ok] backward compatible
(handoff is a hint - peers without the watcher still failover via the
heartbeat path; clients without the notification handler fall through
to reconnect).

**SIGTERM hookup per host**: dcc-mcp-core registers a single signal
handler / atexit hook. Per-adapter integration is one line:

```python
# dcc_mcp_maya: register on plugin load
gateway.install_graceful_shutdown_hook(
    fire_on=["maya.OpenMaya.MSceneMessage.kBeforeQuit",
             "SIGTERM", "SIGINT", "atexit"])
```

Blender / 3ds Max / Houdini ship analogous one-liners pointed at their
own `before_quit` events.

**Constraints check**: [ok] no new process ; [ok] flat rez (gateway_policy.yaml
ships inside `dcc-mcp-core` with conservative defaults; studios may
override at runtime) ; [ok] multi-DCC (decision is `dcc_type`-agnostic) ; [ok]
backward compatible (any pre-RFC adapter falls back to today's
version-driven path because it doesn't know how to write the lease;
new gateway treats missing-lease as "stale" and takes over after probe
fails, matching today's behavior).

### B. Graceful drain on election (when preemption *does* happen)

When preemption is intentionally requested (`allow_preempt_when`), the
outgoing gateway must hand over instead of vanishing:

```text
state machine:
  Active  -- peer asserts higher rank --->  Draining
  Draining (T_grace = 5s):
     * stop accepting new SSE subscribers / POSTs
     * finish in-flight tool calls (or fail them with retriable error)
     * broadcast notification to all subscribers:
         {"jsonrpc":"2.0",
          "method":"notifications/gateway/draining",
          "params":{
             "next_endpoint":"http://127.0.0.1:54521/mcp",
             "next_endpoint_stable":"http://127.0.0.1:9765/mcp",
             "grace_ms":4500,
             "reason":"preempted_by",
             "peer":{"version":"0.18.0","instance_id":"..."}}}
     * after grace: close listener with HTTP/2 GOAWAY equivalent
                    (Connection: close + SSE event: close)
  Closed   -- peer ready ---> peer binds 9765
```

**Wire format**: extends MCP `notifications/*` namespace under
`notifications/gateway/...`. Clients that don't understand it ignore the
notification (standard JSON-RPC behavior) and fall through to plain
reconnect - they're no worse off than today.

**Constraints check**: [ok] protocol-only, no process ; [ok] multi-DCC ; [ok]
compatible (unknown notifications dropped silently).

### C. Stable session id with backend-agnostic resume

**Today.** Two relevant log lines:

```text
session created: fc4c2da2-1b72-401a-89e3-300e784785a8
gateway SSE: backend in stateless mode - no MCP-Session-Id / __session_id
```

The session id exists internally but is not propagated through the
gateway, so a client that reconnects after a drop starts fresh - all
loaded skills, tool subscriptions, pending tasks are gone.

**Proposal**:

1. **Always propagate** `MCP-Session-Id` end-to-end. Gateway adds it on
   first `initialize` if the backend didn't, and stores
   `session_id -> {instance_id, last_active, capability_snapshot}` in a
   small SQLite file under `dcc_mcp_transport::discovery` (next to
   `services.lock`). One file, no service.
2. **Resume path**. When a client reconnects with a known
   `MCP-Session-Id`:
   - If the backing DCC instance is still alive -> reattach.
   - If gone -> return MCP error `session_resumed_on_new_instance` with
     a `replay_hint` listing which skills were loaded, so the client
     can `load_skill` them again in one batch.
3. **GC**. Sessions older than `session_ttl_secs` (default 3600) are
   evicted by the FileRegistry sweep that already runs.

This is also the canonical answer to MCP spec 2025-06-18's "Streamable
HTTP with resumable sessions" - so it doubles as **spec compliance
work**, not just election fix.

**Constraints check**: [ok] adds one SQLite file, no daemon ; [ok] DCC-agnostic
(lives entirely in `dcc-mcp-core`) ; [ok] optional from the client side
(clients without `MCP-Session-Id` get current behavior).

### D. First-class resource for liveness/election state

**Today.** `gateway://instances` exists but is queried on demand. The
gateway capability declares `resources.subscribe=true` (good!) but the
internal hook from election events -> `notifications/resources/updated`
is missing, so clients can't passively follow.

**Proposal**: wire two existing-but-unused signals into the resource
subscription mechanism:

| Resource URI            | Push trigger                                                  |
| ----------------------- | ------------------------------------------------------------- |
| `gateway://instances`   | adapter register / deregister / readiness change              |
| `gateway://election`    | state machine transitions (A/B/Active/Draining/Closed)        |
| `gateway://endpoint`    | when the stable endpoint a client should connect to changes   |

Smart clients subscribe once and receive push notifications. Combined
with **A**, virtually no client ever has to handle a hard disconnect.

**Constraints check**: [ok] piggybacks on existing
`resources/subscribe` plumbing ; [ok] ignored by clients that don't
subscribe ; [ok] DCC-agnostic.

---

## Phasing & per-phase delivery value

Each phase ships and is useful on its own:

- **P0** - gateway lease + heartbeat write path (A, foundation).
  ~150 lines. Adds the data structures; election behavior unchanged.
  No client-visible effect; pre-req for everything below.
- **P1** - switch arrival decision from `version_outranks` to
  `liveness_driven` (A, behavior). ~80 lines.
  Visible effect: "start a 2nd Maya without dropping the 1st Maya's
  agents" *and* "owner crash -> automatic failover within `ttl_secs`".
  Solves the original Cursor disconnect.
- **P1.5** - handoff event (A.event): adds the `gateway_handoff` record, the `notifications/gateway/handoff` SSE message, a `request_release()` public API, and the per-adapter graceful-shutdown hook. ~250 lines. Visible effect: graceful Maya exit and planned upgrade switch in &lt; 50 ms instead of `ttl_secs`. Pairs naturally with P1 and shares its data structures.
- **P2** - `gateway_policy.yaml` + `allow_version_preempt` opt-in
  (A, policy). ~200 lines.
  Studios can declare rolling-upgrade policy without code change.
- **P3** - graceful drain protocol for opt-in preemption / planned
  shutdown (B). ~400 lines.
  Generalises P1.5 to in-flight tool calls (lets the outgoing owner
  finish a long `run_check` before releasing) and adds the formal
  `Draining` state machine entry.
- **P4** - stable session id + SQLite resume store (C). ~600 lines.
  MCP 2025-06-18 compliance + zero-skill-reload after disconnect.
- **P5** - resource subscriptions for election events (D). ~150 lines.
  Observability tools (Admin UI, MCP inspectors) follow election live.

**Future work (separate RFC)** - *Standby pre-election*: have peers
periodically vote a "next in line" successor that pre-warms (cache
endpoint, pre-bind to a temporary port, pre-load common skills). On
A.event or A failover the successor promotes in single-digit ms with
zero election-race. This is etcd/zookeeper-style HA and brings real
value at studio scale; deferred from this RFC to keep the surface area
manageable.

P0+P1 together delete the original Cursor disconnect *and* give the
fleet automatic gateway failover after crashes. Adding P1.5 cuts the
graceful-exit handoff from seconds to ~50 ms. The remaining phases
harden the upgrade path and make the protocol fully resumable.

## Implementation notes for the constraints

- **No new process**: Phase 0-4 add code only inside the existing gateway
  process. The SQLite session store (Phase 3) is a file managed by the
  gateway, not a service.
- **rez packaging**: every change ships inside the existing
  `dcc_mcp_core` rez package. Adapter packages (`dcc_mcp_maya`,
  `dcc_mcp_blender`, future `dcc_mcp_houdini`, ...) keep their current
  `requires = ["dcc_mcp_core-0.17+"]` - the upper version range bumps
  alone activate the new behavior.
- **Multi-DCC**: nothing in the design references Maya / Blender / etc.
  Adapter shipping order is irrelevant; whichever DCC the user happens
  to start first owns the gateway, others register as instances.
- **Backward compatibility matrix**:

  | Client                       | Old gateway              | New gateway                              |
  | ---------------------------- | ------------------------ | ---------------------------------------- |
  | Old (no `MCP-Session-Id`)    | works (today)            | works (sessions auto-issued but unused)  |
  | New (sends `MCP-Session-Id`) | works (header ignored)   | resumable across drops                   |

## Open questions

1. **Lease TTL vs. failover MTTR**. Default `ttl_secs=5` /
   `renew_interval=1.6` means worst-case 5 s of "no one serves 9765"
   between a hard crash and a peer promoting. Acceptable for interactive
   agent use, possibly too long for render farm. Should `ttl_secs` be a
   first-class env var (`DCC_MCP_GATEWAY_LEASE_TTL_SECS`) for studios
   to tune per deployment?

2. **Probe semantics on partial outage**. If `GET /health` returns 200
   but `/v1/readyz` would say "dispatcher dead" (Maya UI thread frozen),
   the lease holder is *technically* alive but useless. Should the
   probe be `/v1/readyz` instead of `/health`, accepting longer
   timeouts? Or two-phase: probe `/health` first, only escalate to
   `/v1/readyz` if a client recently reported a tool timeout?

3. **Split-brain on `services.lock` failure**. If two adapters both
   CAS the lease at the same millisecond and the file lock is buggy,
   both could bind 9765 -> bind fails on one, but lease says "mine" on
   that loser. Defense: after a successful bind, re-verify lease
   ownership and roll back if lost.

4. **Election policy storage**. Should `gateway_policy.yaml` live next to
   `services.json` in `%TEMP%/dcc-mcp-registry/`, or follow studio
   convention (`%APPDATA%/dcc-mcp/`)? Both are off the rez install root
   so admins can pin policy without rebuilding adapters.

5. **`MCP-Session-Id` and DCC instance death**. If a DCC dies mid-session,
   should `resume` return an error (current direction) or silently re-
   bind to a peer DCC of the same type? The latter is convenient for
   farm/headless but dangerous for interactive sessions where scene
   state matters. Suggest: error by default, opt-in re-bind via
   `session.rebind_policy=any_alive_peer`.

6. **Notification namespace**. `notifications/gateway/*` is not in the
   MCP spec. Confirm OK to extend, or use a vendor prefix like
   `notifications/x-dcc-mcp/gateway/*` to make non-standard nature
   explicit and prevent collision with future spec additions.

7. **Drain grace timing**. 5 s feels right for human-interactive cases
   but may be wrong for long-running async tools (`run_check`,
   `capture_ui`, render submits). Should drain timeout be derived from
   `max(tool.timeout_hint_secs across in-flight)` rather than fixed?

8. **Handoff successor selection**. The simple version of A.event lets
   *any* peer race to CAS the lease after the announcement. With many
   peers this is a thundering herd. Should we require the outgoing
   owner to nominate a `suggested_successor` (e.g. "newest healthy
   instance whose dcc_type equals mine"), and have non-suggested peers
   wait `successor_grace_ms` (default 200) before joining the race?
   Compromise between simplicity (no leader-election sub-protocol) and
   thundering-herd avoidance.

9. **Cross-platform port handoff**. SO_REUSEPORT lets two processes
   share a listening socket on Linux & macOS, making the handoff
   literally zero-gap. Windows requires the outgoing owner to release
   first. Should A.event hard-code a per-OS code path, or should the
   protocol treat the bind-overlap window as best-effort and rely on
   client retry to bridge the 1-10 ms Windows gap?

## Out-of-scope follow-ups

- **Sidecar `gateway-front` proxy on a fixed port**. Considered and
  rejected for this RFC because it violates constraint #1 (extra
  process). Worth revisiting **only** if A/B/C/D combined still leave
  observable disconnect classes - current analysis says they don't.
- **Cross-host gateway federation**. Today everything is localhost; LAN
  federation would belong in a separate RFC after this one lands.
