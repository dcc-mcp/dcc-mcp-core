# ADR 018: Instance Status Vocabulary Unification & Actionability

## Status

Proposed — 2026-07-25

## Context

Today `ServiceStatus` (transport-level: `Available`, `Busy`, `Booting`, `Unreachable`, `ShuttingDown`, `Stale`) is stored on `ServiceEntry`, while `dispatch_status` ("ready", "unavailable", or absent) lives in `ServiceEntry.metadata` as a free-form string. The CLI `direct_control_report()` and gateway `dispatch_json()` both read `dispatch_status` from metadata and synthesize their own `ready` booleans, `recommended_next_action` strings, and reason labels independently. This duplication creates drift: the CLI and gateway can disagree on whether an instance is actionable, and callers (agents, scripts, admin UI) must interpret raw strings and booleans scattered across the output.

ADR 018 defines a unified `InstanceStatus` type in `dcc-mcp-transport` that every surface — transport types, gateway output, CLI doctor — uses as the single source of truth.

## Decision

### 1. Unified `DispatchStatus` enum

Add to `dcc-mcp-transport`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DispatchStatus {
    Ready,
    Pending,
    Failed,
    Unknown,
}
```

This replaces the free-form `"ready"` / `"unavailable"` / absent metadata pattern.

### 2. Unified `InstanceStatus` struct

Add to `dcc-mcp-transport`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceStatus {
    /// Transport-level connection state (from ServiceStatus).
    pub status: ServiceStatus,
    /// Application-level dispatch readiness.
    pub dispatch_status: DispatchStatus,
    /// Whether the current state is safe to retry.
    pub retryable: bool,
    /// Human + machine-readable next step for operators/agents.
    pub recommended_next_action: String,
}
```

### 3. Status pairing table

| `ServiceStatus` | `DispatchStatus` | `retryable` | `recommended_next_action` |
|---|---|---|---|
| `Available` | `Ready` | true | "Instance is available for dispatch." |
| `Available` | `Pending` | true | "Wait for instance to report dispatch_status=ready." |
| `Available` | `Failed` | false | "Inspect instance failure stage/reason; the backend may need a restart." |
| `Available` | `Unknown` | true | "Dispatch status not yet reported; try a direct MCP call." |
| `Busy` | `Ready` | true | "Instance is busy; retry after current job completes." |
| `Busy` | `Pending` | true | "Instance is busy and not yet dispatch-ready; wait and retry." |
| `Busy` | `Failed` | false | "Instance is busy but dispatch has failed; inspect failure details." |
| `Busy` | `Unknown` | true | "Instance is busy; retry later." |
| `Booting` | `Pending` | true | "Instance is booting; wait for readiness and retry." |
| `Booting` | `Unknown` | true | "Instance is booting; wait for readiness and retry." |
| `Unreachable` | `Unknown` | false | "Instance is unreachable; check logs and restart if needed." |
| `ShuttingDown` | `Unknown` | false | "Instance is shutting down; wait for a new instance." |
| `Stale` | `Unknown` | false | "Instance is stale; it will be removed by the registry." |

### 4. Consistent output across surfaces

- **Gateway `entry_to_json()`**: top-level `"status"` becomes the full `InstanceStatus` JSON block (status, dispatch_status, retryable, recommended_next_action). The old `"dispatch"` sub-object is removed after a deprecation window.
- **CLI `direct_control_report()`**: uses the same `InstanceStatus` derivation from `ServiceEntry` instead of synthesizing its own.
- **Gateway `gateway://instances` resource**: compact projection includes the `InstanceStatus` fields.

### 5. Discovery health ≠ dispatch availability

`ServiceStatus::Available` means the transport is healthy (TCP port open, MCP server responding). `DispatchStatus::Ready` means the application is ready to accept tool calls (DCC initialized, skill catalog loaded, dispatcher running). These are exposed separately so agents can distinguish "server is up but DCC is still loading" from "everything is ready."

## Consequences

- **Positive**: Single source of truth for instance status; no more string matching on metadata; consistent output across all surfaces.
- **Positive**: Agents get `retryable` and `recommended_next_action` in every instance listing without needing to synthesize them.
- **Negative**: Breaking change to the gateway JSON output shape; existing consumers must update.
- **Migration**: Old `"dispatch"` block is kept alongside the new `"status"` block for one release cycle, then removed.

## References

- Parent: PIP-2881 (P0 Instance State Actionability)
- Child issues: PIP-2900 (Phase 1: transport types), PIP-2901 (Phase 2: gateway output), PIP-2902 (Phase 3: CLI doctor)
