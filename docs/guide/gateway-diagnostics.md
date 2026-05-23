# Gateway Contention & Diagnostics

When multiple `dcc-mcp-server` processes run on one workstation (or you
scale a gateway deployment across pods), they compete for the gateway
role, maintain a shared service registry, probe each other for liveness,
and evict dead peers. This page is the operator's playbook: how each
mechanism is surfaced in logs, metrics, and gateway-native MCP resources, and
how to debug the five recurring failure modes.

---

## Status matrix

The gateway aggregates instances by their `ServiceStatus`. Operators see
these values in the `gateway://instances` MCP resource, `GET /v1/readyz`,
and the `/metrics` Prometheus export.

| Status | What it means | Who sets it | How to recover |
|---|---|---|---|
| `Available` / `ok` | All readiness bits are green; the instance is routable. | Per-DCC `ReadinessProbe` returning `ready`. | — |
| `Booting` | The DCC host is alive but at least one readiness bit is red (process up, dispatcher not ready, or DCC not ready). | `probe_mcp_readiness` decoded `GET /v1/readyz → 503`. | Wait; transient. The gateway **keeps** the registry row so it doesn't churn, but won't route traffic. |
| `Unreachable` | The gateway's TCP probe couldn't answer `/v1/readyz` **or** `/health`. | Gateway health-check loop after 2 consecutive misses. | Investigate the DCC process; after 3 consecutive misses the row is auto-deregistered. |
| `ShuttingDown` | The instance called `deregister` and is winding down live sessions. | Graceful shutdown path. | Wait for it to disappear. |
| `stale` (surface-only) | `last_heartbeat` is older than `stale_timeout`. | Eviction sweeper. | The row will be removed by the next sweep; if stale forever, the process likely crashed without deregistering. Bump `DCC_MCP_STALE_TIMEOUT` only if you know why. |
| `ghost` (internal) | No owner process holds the sentinel lock / PID file. | `FileRegistry::read_alive` on every read. | Auto-pruned; no action. |

---

## Election (three-tier comparison)

Only one process can bind the gateway port at a time; the rest stand by.
When a newer/better adapter shows up, the current gateway yields
cooperatively (issue #718). The comparison uses three tiers, in order:

1. **`crate_version`** — the `dcc_mcp_core` version baked into the
   binary. A 0.14.28 challenger beats a 0.14.17 resident.
2. **`adapter_version`** — tie-break #1. A real DCC adapter
   (`dcc_mcp_maya 0.3.0`) beats a resident that has no adapter version.
3. **`adapter_dcc`** — tie-break #2. A real DCC (`adapter_dcc = "maya"`)
   beats a generic standalone (`adapter_dcc = "unknown"` or missing).

Fields live on the `__gateway__` sentinel row in the `FileRegistry`.
Inspect them by reading `gateway://instances`:

```jsonc
{
  "dcc_type": "__gateway__",     // sentinel row, NEVER routable
  "version": "0.14.28",          // crate_version
  "adapter_version": "0.3.0",    // adapter_version
  "adapter_dcc": "maya",         // adapter_dcc
  "host": "127.0.0.1",
  "port": 9765
}
```

### What you'll see in logs

| Event | Template | Level |
|---|---|---|
| Winner bound the port | `Won gateway election` (with `version`) | `INFO` |
| Challenger waiting | `Challenger: port still taken (attempt N/M)` | `DEBUG` |
| Cooperative yield accepted | `Cooperative yield accepted — waiting for port to free up` | `INFO` |
| Optional cooperative yield fallback | `Cooperative yield optional capability unavailable (...) — polling for port` | `DEBUG` |
| Yield probe skipped for same-or-older challenger | `Skipping cooperative yield probe because challenger is not newer than the current gateway` | `DEBUG` |
| Newer sentinel detected | `Gateway: newer-version sentinel detected — initiating voluntary yield` | `INFO` |

---

## Heartbeat, staleness, and ghost eviction

Three complementary liveness mechanisms:

1. **Heartbeat** (`--heartbeat-secs`, default 5) — each instance
   `touch`es its row every interval. `flush_to_file` does an atomic
   temp-file + rename so concurrent readers never see a half-written
   row (issue #554). On Windows the write is guarded by `LockFileEx` to
   survive read-write overlap.

2. **Stale sweep** (`--stale-timeout-secs`, default 30) — rows whose
   `last_heartbeat` is older than the timeout are surfaced with
   `status: "stale"` and evicted on the next sweep.

3. **Ghost eviction** (#748 + #719) — every `read_alive()` call probes
   the owner process: either the sentinel lock file is acquirable
   (meaning the previous holder is dead) or, fallback, `sysinfo` reports
   the `pid` is no longer running. Rows without a `pid` field are kept
   alive (fail-open contract, #227).

### What you'll see in logs

| Template | Level | When |
|---|---|---|
| `registering service` (with `dcc_type`, `instance_id`, `host`, `port`) | `INFO` | Instance registered. |
| `deregistered service` | `INFO` | Graceful shutdown. |
| `removed stale service` | `INFO` | Stale sweep evicted an instance. |
| `removed ghost entry (owner sentinel/PID is dead)` | `INFO` | Owner process crashed without deregistering. |
| `FileRegistry hot-reloaded from disk` | `TRACE` | mtime-based reload fired. |
| `Gateway: evicted N stale instance(s)` | `INFO` | Periodic sweeper run. |
| `Gateway: reaped N ghost entry/entries` | `INFO` | Periodic sweeper run. |
| `Gateway: pre-subscribe dead-PID sweep reaped ...` | `INFO` | Startup hygiene (#556). |

---

## TCP probe loop

Every 30 s the gateway probes each live backend with `GET /v1/readyz`
(5 s timeout), falling back to `GET /health` for pre-#660 backends. The
outcome maps to `ProbeOutcome::{Ready, Booting, Unreachable}`.

Failure escalation:

- **1 failure** — WARN `Health check failed` with `consecutive_failures=1`.
- **2 failures** — row marked `Unreachable` and filtered out of fan-out.
- **3 failures** — row auto-deregistered; INFO `Auto-deregistered after 3 consecutive health-check failures`.

Startup probe: before the gateway subscribes to any backend, it TCP-
connects each registered instance with a 3-second timeout and evicts
unreachable ones (so you don't burn reconnect budget on a crashed
instance whose registry row survived a reboot).

---

## Gateway-native diagnostics resources

Gateway diagnostics are read-only MCP resources. They are not advertised as
`tools/list` entries, so agents fetch only the diagnostic view they need:

| URI | Use when |
|---|---|
| `gateway://diagnostics/process` | You need gateway process metadata plus live/stale/unhealthy instance counts. Add `?dcc_type=maya` to filter rows. |
| `gateway://diagnostics/audit` | You need pending-call and resource-subscription counts. Backend audit history remains per-instance. |
| `gateway://diagnostics/metrics` | You need the gateway-local tool count, live backend count, timeout settings, and `publishes_backend_tools=false`. |

Example MCP read:

```json
{"jsonrpc":"2.0","id":1,"method":"resources/read","params":{"uri":"gateway://diagnostics/process"}}
```

---

## Prometheus metrics

Build with `cargo build --features prometheus` and mount `GET /metrics`.
The metrics server refreshes counts every 5 seconds:

- `dcc_mcp_instances_total{status="active"}` — count of `Available` rows.
- `dcc_mcp_instances_total{status="stale"}` — count of rows past `stale_timeout`.
- `dcc_mcp_tools_total{dcc_type="maya"}` — visible tool count per DCC.
- `dcc_mcp_request_duration_seconds` — histogram of request latency.
- `dcc_mcp_requests_failed_total{method, error}` — per-method failure counter.

---

## Bare troubleshooting recipes

### Scenario 1 — "One DCC server is missing from `tools/list`"

Remember: the gateway `tools/list` only contains the discover + dispatch
primitives. Per-tool tools live on `search_tools` / `describe_tool`.
What's missing is probably the **instance**, not its tools.

```bash
# Via the gateway-native MCP resource (any MCP client can run this)
# → Returns every row with its status; each entry carries `mcp_url`.
resources/read uri=gateway://instances
# Optional URI query: gateway://instances?include_dead=true to see
# rows whose owning process exited.

# Via gateway REST
curl -s http://127.0.0.1:9765/v1/context | jq .
```

Diagnose by status:

- `stale` → heartbeat older than `stale_timeout`. Likely the process died.
- `booting` → `GET /v1/readyz` on that instance returned 503. The DCC host is still starting.
- `unreachable` → probe failed. Check the instance's own logs; will auto-deregister after 3 misses.
- not in the list at all → the process never registered. Check `DCC_MCP_REGISTRY_DIR` and `FileRegistry` permissions.

### Scenario 2 — Ghost row never deregisters

```bash
# List everything, including rows the gateway has filtered out:
resources/read uri=gateway://instances?include_dead=true
```

If you see a row with `pid` pointing at a long-dead process, the
sentinel lock file should have been released on process exit and the
next `read_alive` should evict it automatically. Force it by restarting
the gateway (its startup-probe pass will call `read_alive`). If that
still doesn't evict it, check the `locks/` directory under
`DCC_MCP_REGISTRY_DIR` — a leftover `<dcc_type>-<instance_id>.lock`
whose owner is dead but can't be unlocked usually points at a stale
Windows handle; manually deleting the lock file + `services.json` row
is safe.

### Scenario 3 — `tools/call` returned "Unknown gateway tool"

Since v0.15 the gateway no longer exposes per-tool backend actions via
`tools/list`. Any tool name the gateway doesn't recognise — including
backend-qualified `<skill>__<action>` / `i_<id8>__<escaped>` /
`<id8>__<tool>` forms — now returns the redirect message:

> Unknown gateway tool 'X'. The gateway MCP surface is intentionally
> minimal — it only exposes discovery + dispatch primitives. Use
> `search_tools` to find backend capabilities, `describe_tool` to get a
> schema, and `call_tool` to invoke one by slug.

Fix: update the caller to the new flow — `search_tools` → `describe_tool`
→ `call_tool`. Or use REST `POST /v1/call` directly with a `tool_slug`.

### Scenario 4 — Gateway auto-deregistered my server but it's still running

The TCP probe missed 3 consecutive times. Root causes, in order of
likelihood:

1. **Firewall** — does the gateway-host actually reach the instance's
   `mcp_port`? `curl -s http://<host>:<port>/v1/readyz` from the
   gateway host.
2. **Probe timeout too tight** — the default is 5 s. A scene open that
   blocks the HTTP thread can miss it. Either make `/v1/readyz` a cheap,
   non-blocking endpoint (the default does this) or raise the probe
   interval.
3. **Wrong endpoint** — pre-#660 servers only answer `GET /health`. The
   gateway falls back automatically; if you've patched the health path
   to something else, update the patch.

After you fix the root cause, the instance will re-register on its next
heartbeat tick (no manual intervention needed).

### Scenario 5 — Election flapping / two instances claim the same DCC

Happens when two processes registered the same `dcc_type` but have
different `instance_id`. The gateway keeps them distinct (the `<id8>`
prefix in tool slugs disambiguates) — that's by design, not a bug.
What's **not** by design is two rows with the same `(host, port)` — that
means two processes bound the same port, which shouldn't be possible.
Check for:

- A crashed-then-restarted process whose old row is ghost — wait for
  `read_alive` to evict it (usually within 30 s).
- A misconfigured autostart that launched the same DCC twice.

The election itself is cooperative: the current gateway yields on a
newer sentinel, it doesn't race. If you see flapping in the `__gateway__`
sentinel row's version field, check system clock drift (two machines
claiming to be "newer" than each other is almost always a time-sync
problem).

---

## Debug recipes cheat-sheet

```bash
# List every known instance, live and dead.
curl -s http://127.0.0.1:9765/mcp \
     -H 'content-type: application/json' \
     -d '{"jsonrpc":"2.0","id":1,"method":"resources/read","params":{"uri":"gateway://instances?include_dead=true"}}' \
  | jq .

# Probe an instance by hand.
curl -v http://127.0.0.1:18812/v1/readyz

# Check the gateway's own metrics (needs prometheus feature).
curl -s http://127.0.0.1:9765/metrics | grep dcc_mcp_

# Inspect the on-disk registry.
ls -la "$DCC_MCP_REGISTRY_DIR"
cat "$DCC_MCP_REGISTRY_DIR/services.json" | jq .

# Tail the gateway log.
tail -F "$DCC_MCP_LOG_DIR/dcc-mcp.*.log" | grep -E 'Gateway|ghost|stale|Health'
```

---

## Related reading

- [REST API surface](rest-api-surface.md) — `/v1/readyz`, error kinds, envelope parity.
- [CLI reference](cli-reference.md) — every flag and env var on `dcc-mcp-server`.
- [AGENTS.md](https://github.com/loonghao/dcc-mcp-core/blob/main/AGENTS.md) — full decision table for the public API.
