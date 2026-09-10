# ADR 036: Vendor-neutral durable memory provider boundary

Status: Accepted

## Context

Core already owns bounded ephemeral, working, and long-term memory through
`MemoryStore` and `MemoryRecorder`. External memory services need a stable
integration boundary without coupling Core to one vendor, blocking a DCC host
thread, or giving remembered content authority to invoke tools.

## Decision

Core exposes a small `MemoryProvider` protocol with `recall`, `remember`,
`forget`, `health`, and `close`. Its DTOs contain JSON-safe inert data only.
Providers receive no server, dispatcher, tool registry, approval object, or
callable action.

`NoopMemoryProvider` is the default privacy-preserving behavior. An opt-in
`LocalMemoryProvider` supplies a minimal SQLite implementation. DCC runtimes
must use `MemoryCoordinator`, which submits all provider I/O to bounded worker
threads and completes returned futures with a timeout. A timed-out provider
cannot delay the DCC main thread; callers decide whether to ignore, retry, or
surface the failure.

Third-party packages register factories in the
`dcc_mcp.memory_providers` Python entry-point group. Core does not import or
depend on MemCode or another service SDK.

Existing lifecycle hooks remain observation and policy boundaries.
`AFTER_TOOL_CALL` may produce a candidate `MemoryRecord`, but applications must
apply their own consent, sensitivity, and retention policy before calling
`remember`. Recalled records are context hints only and never grant permission,
execute actions, or bypass tool policy.

## Consequences

- Providers are replaceable and independently releasable.
- Remote latency and failures stay outside DCC host threads.
- Local persistence is explicit rather than enabled by importing Core.
- Provider processes may continue work after a timeout; implementations should
  use their own bounded network timeouts and idempotent record identifiers.
- Semantic ranking, consent UI, and vendor-specific authentication remain
  outside this minimum contract.

## Alternatives considered

- Bind Core directly to one hosted memory SDK: rejected because credentials,
  release cadence, and data policy would leak into every DCC adapter.
- Extend `LifecycleHooks` to perform remote I/O synchronously: rejected because
  hooks execute at latency-sensitive host boundaries.
- Put executable callbacks in recalled records: rejected because memory is
  context, not an authority or automation surface.
