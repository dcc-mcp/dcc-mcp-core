# ADR-018: Instance Status Vocabulary Unification & Actionability

## Status

Proposed

## Context

`dcc-mcp-core` exposes instance status through three surfaces — gateway resource
(`gateway://instances`), CLI `doctor`, and CLI `call`/`wait-ready` — but each
surface uses a different vocabulary and schema:

| Surface | status values | dispatch_status | retryable | recommended_next_action |
| --- | --- | --- | --- | --- |
| Gateway `compact_instance_json` | `available`, `busy`, `unreachable`, `booting`, `stale` (derived) | `ready`, `pending` (ad-hoc metadata) | ✗ | ✗ |
| Gateway `entry_to_json` (verbose) | same + `shutting_down` | `ready`, `not_reported` (metadata) | ✗ | ✗ |
| CLI `direct_control_report` | From `ServiceStatus` raw | `ready`, `unavailable`, `not_reported` | ✗ | ✓ (hardcoded `if/else`) |
| CLI `doctor` | Through `direct_control` block | Ad-hoc key path | ✗ | ✓ (drifted from CLI call) |

This creates four problems:

1. **Agent confusion**: An agent reading `gateway://instances` sees `dispatch.ready: true` but
   `status: "booting"` — should it route or wait? Without `retryable` and
   `recommended_next_action`, every downstream consumer re-derives the answer.

2. **Discovery health ≠ dispatch availability**: PIP-2725 introduced `list_ok` +
   `index_health` honesty fields so agents can distinguish "the registry listed
   successfully" from "these instances can accept tool calls." But each surface
   encodes this separation differently.

3. **No retry signal**: When an instance is `status=booting, dispatch_status=pending`,
   the agent cannot know whether to poll, restart, or escalate. Today the logic
   is duplicated across gateway, CLI, and every adapter.

4. **Drift between surfaces**: `direct_control_report` has
   `recommended_next_action`; `compact_instance_json` does not. `doctor` reads
   `dispatch_status` from a different key path than `call`. Every new surface
   reinvents the mapping.

Parent: PIP-2881 (RFC: CLI interaction contract & workflow consistency).
Preceding research: PIP-2880 (Unity CLI survey, Steve Jobs, 2026-07-24).

## Decision summary

1. **Unified status vocabulary**: Two orthogonal axes:
   - `status` (liveness): `available` | `busy` | `booting` | `unavailable`
   - `dispatch_status` (readiness): `ready` | `pending` | `failed` | `unknown`

2. **Every (status, dispatch_status) pairing carries `retryable: bool | null` and
   `recommended_next_action: string`.** The mapping is a pure function of the
   pairing; no surface re-derives it.

3. **All three surfaces — `gateway://instances`, CLI `doctor`, CLI `call`/`wait-ready`
   — emit the same `InstanceStatus` schema.** Consumers learn one shape.

4. **Discovery health stays separate.** `list_ok`, `index_health`, `stale`, and
   `evicted_dead` remain top-level honesty fields; they are never folded into
   per-instance status.

## Status vocabulary

### `status` (liveness — from `ServiceEntry.status` / heartbeat staleness)

| Value | ServiceStatus(es) | Semantics |
| --- | --- | --- |
| `available` | `ServiceStatus::Available` | Accepting connections, not leased |
| `busy` | `ServiceStatus::Busy` | Has an active lease or in-flight call |
| `booting` | `ServiceStatus::Booting` | Alive but DCC host still initializing |
| `unavailable` | `ServiceStatus::Unreachable`, `::ShuttingDown`, `::Stale`, or stale-by-heartbeat | Not routable |

The `status` field is a **surface projection**, not a new storage enum.
`ServiceStatus` variants persist unchanged in `services.json` and the
`ServiceEntry` struct. The projection happens at serialization time so
the on-disk format and Rust-internal types stay backward-compatible.

Staleness is a cross-cutting concern: `status: "unavailable"` + `stale: true`
means heartbeat timeout; `status: "unavailable"` + `stale: false` means the
instance explicitly reported an unroutable state.

### `dispatch_status` (readiness — from `entry.metadata["dispatch_status"]`)

| Value | Metadata value | Semantics |
| --- | --- | --- |
| `ready` | `"ready"` | Dispatcher confirmed; `mcp_url` present; ready for `tools/call` |
| `pending` | `"pending"` | Dispatcher initializing; may become ready |
| `failed` | `"failed"` | Dispatcher reported a terminal failure; see `failure_stage` / `failure_reason` |
| `unknown` | absent / any other value | No dispatch status reported; treat as not-ready |

## `retryable` + `recommended_next_action` mapping

The mapping is a deterministic function `f(status, dispatch_status, stale)`:

| status | dispatch_status | stale | retryable | recommended_next_action |
| --- | --- | --- | --- | --- |
| `available` | `ready` | false | `null` | Instance is ready for dispatch. |
| `available` | `pending` | false | `true` | Wait for dispatch_status=ready. Run `dcc-mcp-cli wait-ready`. |
| `available` | `failed` | false | `true` | Inspect dispatch failure (failure_stage, failure_reason). Restart DCC or wait for sidecar recovery. |
| `available` | `unknown` | false | `false` | Instance is not reporting dispatch status. Verify the dispatcher/sidecar is running. |
| `busy` | `ready` | false | `true` | Instance is busy with an active lease. Wait for the current job to complete or select another instance. |
| `busy` | `pending` | false | `true` | Instance is busy and dispatch is not ready. Wait for the current job and dispatcher to resolve. |
| `busy` | `failed` | false | `true` | Instance is busy and dispatch failed. Cancel the current job (if owned) and restart the DCC host. |
| `busy` | `unknown` | false | `false` | Instance is busy with unknown dispatch status. Verify dispatcher health and lease ownership. |
| `booting` | `ready` | false | `true` | Instance is still booting but dispatcher reports ready. Wait for status=available. |
| `booting` | `pending` | false | `true` | Instance is booting. Wait for DCC host initialization to complete. |
| `booting` | `failed` | false | `false` | Instance booting but dispatch failed. Check DCC startup logs and restart. |
| `booting` | `unknown` | false | `true` | Instance is booting. Wait for status=available and dispatch_status reporting. |
| `unavailable` | any | true | `true` | Instance heartbeat is stale. Verify the DCC host process is still running. Restart if dead. |
| `unavailable` | any (not stale) | false | `false` | Instance is unavailable. Check if the DCC host process is running. Restart if it exited. |

`retryable: null` means "not applicable" — the instance is ready, no retry
decision needed.

`retryable: true` = the caller should poll/wait and retry. `retryable: false` =
the caller should not retry without human or operator intervention (restart,
reconfigure, investigate logs).

## Unified `InstanceStatus` schema

Every surface emits the same sub-object. In Rust, this is a free function
`compute_instance_status(entry: &ServiceEntry, stale: bool) -> InstanceStatus`
that returns:

```rust
/// Projected status for agent-facing surfaces (ADR-018).
#[derive(Debug, Clone, Serialize)]
pub struct InstanceStatus {
    /// Liveness: "available" | "busy" | "booting" | "unavailable"
    pub status: &'static str,
    /// Readiness: "ready" | "pending" | "failed" | "unknown"
    pub dispatch_status: &'static str,
    /// true=poll/retry, false=intervention needed, null=not applicable
    pub retryable: Option<bool>,
    /// Human-readable next action for the agent
    pub recommended_next_action: &'static str,
}
```

In JSON output (compact and verbose):

```json
{
  "status": "available",
  "dispatch_status": "ready",
  "retryable": null,
  "recommended_next_action": "Instance is ready for dispatch."
}
```

### Integration into existing surfaces

| Surface | Where | Change |
| --- | --- | --- |
| Gateway `compact_instance_json` | Top-level `instance_status` key | Add `InstanceStatus` block; keep existing `status`, `stale`, `dispatch` for backward compat (deprecate after 2 releases) |
| Gateway `entry_to_json` (verbose) | Top-level `instance_status` key | Same; existing `status`, `stale`, `dispatch` preserved |
| CLI `direct_control_report` | `direct_control` block | Add `instance_status` sub-key; existing fields preserved |
| CLI `doctor` | Per-instance in `local.inventory` | Use the same `compute_instance_status` function |

Backward compatibility: existing consumers reading `status` (string),
`stale` (bool), and `dispatch.ready` (bool/null) continue to work. The new
`instance_status` block is additive. After two releases, the deprecated
fields are removed and `instance_status` becomes the canonical location.

## Discovery health ≠ dispatch availability

These remain separate, top-level honesty signals:

| Field | Location | Semantics |
| --- | --- | --- |
| `list_ok` | `gateway://instances` response | Registry read succeeded |
| `index_health` | `gateway://instances` response | `"healthy"` / `"degraded"` / `"empty"` |
| `total` / `capped` / `limit` / `offset` | `gateway://instances` response | Pagination metadata |
| `stale_count` / `evicted_dead` | `gateway://instances` response | Registry health counters |

An instance with `instance_status.status: "available"` and
`instance_status.dispatch_status: "ready"` is routable **regardless** of
`index_health`. An agent that needs to assess fleet-level health reads
`index_health`; an agent that needs to route a single call reads
`instance_status`. No field serves both purposes.

## Requirements

### Functional

1. `compute_instance_status(entry, stale) -> InstanceStatus` is a pure function
   with no side effects.
2. `gateway://instances` compact and verbose responses both include the
   `instance_status` block on every instance.
3. CLI `doctor` uses the same `compute_instance_status` function for local
   registry rows.
4. CLI `call` / `wait-ready` use `instance_status` when reporting why an
   instance cannot be used.
5. The mapping table in this ADR is the single source of truth; no surface
   hardcodes different `retryable` or `recommended_next_action` values.

### Non-functional

- `compute_instance_status` must not allocate on the heap (use `&'static str`).
- The function lives in `dcc-mcp-transport` so CLI, gateway, and server crates
  all depend on it without pulling in HTTP/gateway deps.
- Existing tests for `compact_instance_json`, `direct_control_report`, and
  `doctor` must pass with the additive fields present.
- Python 3.7 adapters are consumers of the JSON output only; no Python-side
  changes required.

## Implementation phases

### Phase 1: Transport types (`dcc-mcp-transport`)

- Add `DispatchStatus` enum to `discovery::types`
- Add `InstanceStatus` struct with `compute_instance_status` free function
- Add the mapping table as a private constant lookup
- Unit tests for all 16+ pairings

### Phase 2: Gateway output (`dcc-mcp-gateway`)

- `compact_instance_json`: add `instance_status` block
- `entry_to_json` / `dispatch_json`: add `instance_status` block
- Keep existing `status`, `stale`, `dispatch` fields unchanged

### Phase 3: CLI surfaces (`dcc-mcp-cli`)

- `direct_control_report`: add `instance_status` sub-key
- `doctor`: use `compute_instance_status` for per-instance status
- `instance_selection`: surface `instance_status` in error messages

### Phase 4: Deprecation (2 releases later)

- Remove deprecated `status`, `stale`, `dispatch` fields from gateway output
- Remove `direct_control` redundant fields
- `instance_status` becomes the only status location

## Reuse before adding code

| Need | Existing owner | Change |
| --- | --- | --- |
| `ServiceStatus` enum | `dcc_mcp_transport::discovery::types` | Add `DispatchStatus` enum alongside it; do not modify `ServiceStatus` |
| `dispatch_status` metadata | `ServiceEntry.metadata["dispatch_status"]` | Formalize values; add `DispatchStatus::from_entry()` constructor |
| `compact_instance_json` | `dcc_mcp_gateway::native_resources::instances` | Call `compute_instance_status` and embed the result |
| `direct_control_report` | `dcc_mcp_cli::application::local_instance` | Call `compute_instance_status`; keep `direct_control` block for backward compat |
| `doctor` | `dcc_mcp_cli::application::doctor` | Use `compute_instance_status` for per-instance readiness |

No new crate, database table, or service is needed.

## Consequences

### Positive

- Agents read one schema (`instance_status`) across all surfaces.
- `retryable` and `recommended_next_action` are never re-derived by consumers.
- Discovery health and dispatch availability stay separate and unambiguous.
- The mapping table is auditable and testable as a single source of truth.

### Negative

- Two-release backward-compat window means duplicate fields temporarily.
- `ServiceStatus` display names change on the wire (`shutting_down` →
  `unavailable`); consumers relying on exact string matching may break.

### Neutral

- `ServiceStatus` enum variants are unchanged internally; only the surface
  projection is new.
- Python adapters consume the JSON output only and are unaffected.

## Alternatives rejected

- **Add `retryable`/`recommended_next_action` to `ServiceEntry` struct**: These are
  derived values, not stored state. Storing them would duplicate the mapping
  logic and risk drift between storage and computation.
- **Keep status and dispatch_status as ad-hoc metadata strings**: The current
  approach works but every consumer re-derives readiness, leading to the
  inconsistency documented in the Context section.
- **Fold `index_health` into per-instance status**: Discovery fleet health is a
  different concern from per-instance routability. Conflating them would force
  agents to read per-instance fields to assess fleet health, and vice versa.
- **Remove existing `status`/`dispatch` fields immediately**: Would break every
  downstream consumer. The two-release deprecation window is the safer path.

## Product defaults

1. `instance_status` appears in `gateway://instances` compact output by default
   (no opt-in flag).
2. CLI `doctor` and `call` output use `instance_status` when run with
   `--output json`; `--output human` shows a summary line.
3. Admin UI instance detail view renders `instance_status` with a color-coded
   status badge and the action text as a tooltip.
