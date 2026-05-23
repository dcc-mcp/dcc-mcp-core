# RFC 0002 - Event Bus & Webhooks

**Status**: Draft
**Target repo**: `dcc-mcp-core` (with surface in `dcc-mcp-server`)
**Authors**: dcc-mcp-core contributors
**Date**: 2026-05-23
**Related**: RFC 0001 (gateway election); RFC 0003 (traffic interception - reuses the EventBus from this RFC)

---

## Summary

Today, downstream DCC integrators (Maya shelf tools, character rigging
tools, future Blender / Houdini / 3ds Max / Photoshop tools, and other
studio plugins) can only observe and extend the
dcc-mcp-server by either (a) re-reading log files or (b) forking the
server. There is no first-class way to **hook** into the lifecycle of
the gateway, an adapter, a skill, or a tool call.

This RFC proposes a single in-process **`EventBus`** in `dcc-mcp-core`,
exposed via two compatible surfaces:

1. **In-process Python subscriptions** for downstream Python code (Maya
   plugins, studio tools, automated test fixtures) that runs alongside
   the adapter.
2. **Out-of-process HTTP webhooks** for sidecar tools that live in a
   different process / language / host (CI gates, dashboards,
   notification bots, external monitoring).

Both surfaces emit the **same event taxonomy and schema**, so a hook
written for one consumer keeps working when moved to the other.

## Motivation

Real downstream integration use cases that today require log-scraping or
patches to upstream code:

- **Per-call audit log into studio observability.** Every tool call
  produced by an artist's agent session should land in the studio
  metrics pipeline (latency, success rate, which `dcc_type`, which
  skill, which user). Today there's no callback to fan this out without
  parsing `dcc_mcp_http::trace::on_response` log lines.
- **Skill load gating.** A studio policy may forbid loading `experimental-maya-tools`
  on a render farm node. Without a `skill/loading` event we can't
  decline before the skill enters the catalog.
- **Custom progress UI on tool dispatch.** A studio Maya panel wants
  a small toast when an agent dispatches a long tool ("agent is
  exporting your shot"). Today the panel polls the admin task list.
- **Alert when an agent calls a destructive tool.** A webhook on
  `tool/dispatched` where `annotations.destructive_hint=true`
  takes minutes to wire if the event exists; today it's not feasible
  without a proxy in front of the gateway.
- **CI gate on adapter promotion.** When a new gateway is promoted in
  a CI environment a webhook to the CI service lets it run a smoke
  test and tear the env down.

The unifying observation: dcc-mcp-server **already raises every
interesting state change internally** (we can see them in the trace
logs) - it just doesn't expose them as a first-class extension point.

## Constraints

Same hard constraints as RFC 0001:

1. **No new processes.** The EventBus lives inside the existing
   gateway / adapter processes. Webhook delivery is an outbound HTTP
   client task, not a new daemon.
2. **Flat rez packaging.** Hooks ship inside `dcc-mcp-core`; downstream
   adapters and studio tools inherit via existing `requires`.
3. **Multi-DCC by construction.** Event taxonomy and schema are
   DCC-agnostic. Per-DCC details live in `event.attributes`.
4. **Backward compatible.** No event subscription is required; with no
   subscribers the bus is a near-zero-cost pass-through.

## Non-goals

- High-throughput log shipping (>10k events/sec). Studios that need that
  should feed the EventBus into a real broker (Kafka, NATS) via a
  webhook - out of scope for the core bus.
- Replacing structured logging. Logs stay; events are an orthogonal
  *programmatic* channel.
- Replacing OpenTelemetry. Events are higher-level domain semantics
  ("skill loaded", "tool dispatched"); OT spans remain the source of
  truth for distributed tracing. An OT exporter for events is a Phase-3
  follow-up.

---

## Design

### 1. Event taxonomy

Events are addressed by hierarchical dotted names. Subscribers can
register for an exact name or a `prefix.*` wildcard.

| Namespace        | Examples                                                                                              | Lifecycle of payload     |
| ---------------- | ----------------------------------------------------------------------------------------------------- | ------------------------ |
| `gateway.*`      | `gateway.started`, `gateway.stopping`, `gateway.election_changed`, `gateway.handoff_announced`        | Process / cluster level  |
| `instance.*`     | `instance.registered`, `instance.deregistered`, `instance.health_changed`                             | Per-DCC adapter          |
| `skill.*`        | `skill.discovered`, `skill.loading`, `skill.loaded`, `skill.unloaded`, `skill.validation_failed`      | Per skill                |
| `tool.*`         | `tool.dispatched`, `tool.completed`, `tool.failed`, `tool.cancelled`, `tool.timed_out`                | Per tool call            |
| `resource.*`     | `resource.read`, `resource.subscribed`, `resource.changed`                                            | Per MCP resource URI     |
| `session.*`      | `session.created`, `session.resumed`, `session.expired` (depends on RFC 0001's session work)          | Per MCP session          |
| `client.*`       | `client.connected`, `client.disconnected`, `client.initialize`                                        | Per connection           |
| `policy.*`       | `policy.skill_denied`, `policy.tool_denied`                                                           | Per enforcement decision |

Studios may emit their own events under a `studio.<name>.*` prefix; the
core bus does not interpret them but delivers them to any subscriber.

### 2. Event schema

Every event is a single JSON object with a stable envelope:

```jsonc
{
  "schema_version": 1,
  "name":           "tool.completed",
  "id":             "ev_01HQX...",        // ULID, sortable by time
  "timestamp_ns":   1779478215123456789,
  "source": {
    "instance_id":  "1f363976-...",
    "dcc_type":     "maya",
    "adapter_version": "0.3.7",
    "host":         "WORKSTATION-01",
    "pid":          68616
  },
  "correlation": {
    "session_id":   "fc4c2da2-...",       // present iff client-driven
    "request_id":   "cd4aacff-...",       // MCP request id
    "trace_id":     "abc123...",          // OT trace id when available
    "span_id":      "def456..."
  },
  "attributes": {
    // event-specific payload, snake_case keys, JSON primitives only
    "tool_slug":    "maya.1f363976.maya_pipeline__bootstrap_project",
    "skill_name":   "maya-pipeline-dev",
    "duration_ms":  142,
    "result_success": true
  }
}
```

`schema_version` bumps only on breaking changes; additive attribute
additions are non-breaking.

### 3. Subscription surfaces

#### 3a. In-process Python

```python
from dcc_mcp_core.events import bus, EventFilter

@bus.on("tool.completed")
def record_metric(event: dict) -> None:
    studio_metrics.observe(
        name=event["attributes"]["tool_slug"],
        latency_ms=event["attributes"]["duration_ms"],
        ok=event["attributes"]["result_success"],
    )

# Wildcards + filters
@bus.on("skill.*", filter=EventFilter(dcc_type="maya"))
def audit_skill_lifecycle(event): ...

# Decline-on-skill-loading hook (synchronous, blocking - see section 6)
@bus.before("skill.loading")
def gate_skill(event):
    if event["attributes"]["skill_name"] == "experimental-maya-tools" and is_farm_node():
        return bus.Veto("experimental-maya-tools not allowed on farm nodes")
    return None
```

Two flavors of subscriber:

- **`on(name)`** - asynchronous, fire-and-forget, no return value.
  Subscriber crash is isolated (logged + bus continues).
- **`before(name)`** - synchronous, may return a `Veto` to abort the
  triggering action. Only allowed on a small set of "vetoable" event
  names (defined per event below); other names raise at registration.

Subscriptions live for the lifetime of the process; an explicit
`bus.unsubscribe(handler)` is provided.

#### 3b. Out-of-process HTTP webhook

Studios configure webhooks declaratively in a YAML file picked up by
the gateway on startup (and reloaded on file change):

```yaml
# webhooks.yaml - sits next to gateway_policy.yaml (see RFC 0001)
webhooks:
  - name: studio-metrics
    url: https://example.com/dcc-mcp/events
    events:
      - tool.completed
      - tool.failed
    headers:
      Authorization: "Bearer ${DCC_MCP_WEBHOOK_TOKEN}"
    delivery:
      attempts: 3
      backoff_ms: [200, 1000, 5000]
      timeout_ms: 2000
    filters:
      - attributes.skill_name: "maya-*"

  - name: destructive-tool-alert
    url: https://hooks.example.com/services/...
    events:
      - tool.dispatched
    filters:
      - attributes.annotations.destructive_hint: true
    payload_template: |
      { "text": "Agent in {{source.dcc_type}} called destructive tool {{attributes.tool_slug}} (session {{correlation.session_id}})" }
```

Webhook delivery runs on an internal Tokio task pool, bounded queue
length, and applies per-webhook backoff. Failed deliveries are logged
and emitted on the bus as a `webhook.delivery_failed` event (which
itself can have subscribers but **cannot** trigger webhook delivery -
loop-break by construction).

### 4. Delivery guarantees

- **In-process `on(...)` subscribers**: best-effort, fire-and-forget.
  Subscriber exceptions are caught and reported on
  `bus.subscriber_failed`. Order within a single emit point is preserved
  per-subscriber; order across emit points is not guaranteed.
- **In-process `before(...)` subscribers**: synchronous, returns a
  decision that the emitting site must honor. Strict registration-order.
- **Out-of-process webhooks**: at-least-once with bounded retry. If all
  retries exhaust, the event is dropped and `webhook.delivery_failed`
  is emitted with the original event id (so it can be replayed from a
  durable sink, see RFC 0003).

### 5. Performance budget

- With **0 subscribers** on a given event name: cost is a single
  hashmap lookup + one branch - measurable but not material to the
  hot path of a tool call. The bus must benchmark this and pass a
  microbenchmark gate before each release.
- With **N in-process subscribers**: O(N) calls. The emitter is
  responsible for not emitting in tight loops; bus is not.
- Webhook fan-out is fully async; emitting tool path is **not** blocked
  by webhook delivery latency.

### 6. Vetoable events (`before(...)` subscribers)

Only these events accept a veto, because their emission site has a
well-defined "do nothing instead" branch:

- `skill.loading` -> reject skill load
- `tool.dispatched` (before the host call) -> reject the tool call with
  a structured error
- `resource.subscribed` -> reject subscription
- `client.initialize` -> reject the new client (e.g. policy match
  failure)

Adding more vetoable events requires changes at the emission site, so
the list is hand-curated rather than open.

---

## Phasing

- **P0** - `EventBus` core, in-process `on()` API, taxonomy for
  `gateway.*` and `instance.*`. ~250 lines. No subscribers shipped yet;
  enables downstream experimentation.
- **P1** - Emit points for `skill.*` and `tool.*` (the most-requested
  category). ~200 lines.
- **P2** - `before()` API + vetoable-event whitelist. ~150 lines.
- **P3** - Webhook delivery worker + `webhooks.yaml` loader.
  ~400 lines.
- **P4** - OpenTelemetry exporter for the event stream (events become
  span events on the current OT span). ~150 lines. Optional, can land
  any time after P0.

## Backward compatibility

- The bus exists from P0 but emits nothing if no events are wired yet.
- Downstream adapters / clients that don't import `dcc_mcp_core.events`
  see no behavior change.
- Webhook config absent => no outbound HTTP.
- Schema changes to **`attributes`** are additive; **envelope** changes
  bump `schema_version` and the bus refuses subscribers with a
  declared `min_schema_version` higher than runtime.

## Open questions

1. **Event payload size cap**. Should we hard-cap `attributes` size
   (e.g. 4 KiB) to keep webhook payloads sane, with overflow indicated
   by an `attributes_truncated: true` flag? RFC 0003 will want the
   *full* payload - does it bypass the cap because it reads from a
   different sink, or does the bus carry full payloads and webhook
   delivery alone applies the cap?

2. **Static dispatch for the zero-subscriber hot path**. Should we
   generate per-event-name dispatcher functions at build time so the
   "no subscribers" check is a constant-folded `false` instead of a
   hashmap lookup? Worth it only if benchmarks show the hashmap is
   visible in profiles.

3. **Per-DCC namespaces vs flat namespaces**. Today the proposal uses
   one flat namespace. Alternative: `maya.tool.completed` vs
   `tool.completed` with `source.dcc_type=maya`. The flat version is
   simpler; the namespaced version composes better with subscription
   wildcards. Suggest: stay flat, rely on `source.dcc_type` for
   filtering.

4. **Webhook authentication for inbound (i.e. webhook *receivers* that
   need to know the request came from us)**. Should we sign the HTTP
   body with an HMAC keyed off a per-webhook secret? Common pattern
   (GitHub, Stripe). Defer to P3 if scope creeps.

5. **In-process subscribers crossing Python <-> Rust boundary**. The
   core is Rust with a Python facade. Should we keep the subscriber
   list in Rust and call into Python via PyO3, or proxy to a pure-
   Python EventBus with a one-way Rust->Python bridge? The former is
   faster but adds GIL coordination; the latter is simpler but adds
   one IPC hop. Suggest: prototype both, decide on benchmark.

## Acknowledgements

Originated from the downstream Maya integration work after
realising that downstream studio code (per-call metrics, skill
gating, destructive-tool alerts) all needed the same primitive that
didn't exist in `dcc-mcp-core`.
