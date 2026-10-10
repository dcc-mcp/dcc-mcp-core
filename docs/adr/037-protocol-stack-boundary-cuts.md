# ADR 037: Cut the DCC Coupling Out of the MCP Protocol Stack

## Status

Proposed — Cut 1 and Cut 3 implemented, Cut 2 not started.

| Cut | Status | Evidence |
| --- | --- | --- |
| 1 — invert `http-types` → `gateway-core` | Done | `GatewayPolicy` moved; policy evaluation made generic over `PolicySubject` so the config crate never names `CapabilityRecord`; `gateway-core` re-exports it |
| 3 — extract `dcc-mcp-adapters` | Done | New crate owns the adapter surface; `dcc-mcp-protocols` re-exports it; `dcc-mcp-usd` repointed; protocols drops `parking_lot` |
| 2 — split `dcc-mcp-transport` | Not started | Deferred: `discovery/` has 120 consumer files and `TransportError` must be split first |

Every cut keeps `src/lib.rs` byte-identical, so the Python 3.7 wheel
composition is unchanged.

## Context

ADR-027 assigns one canonical owner to every cross-crate protocol type, and the
workspace has largely followed it. What ADR-027 does not prevent is a crate
whose *canonical concern is generic* from also carrying DCC-specific payload
types, or from depending downward on a DCC-shaped crate. Three concrete
instances exist today:

1. `dcc-mcp-protocols` owns "MCP tool, resource, prompt, and adapter models"
   (ADR-027). Its generic MCP types (`types_tools.rs`, `types_resources.rs`,
   `types_prompts.rs`, `error_envelope.rs`) live beside a ~2 000-line DCC
   adapter surface: `src/adapters/types.rs` (492 lines: `DccInfo`, `SceneInfo`,
   `DccCapabilities`, `CaptureResult`, `SceneNode`, `FrameRange`, …),
   `src/adapters/traits.rs` (519 lines: 12 `Dcc*` traits), and
   `src/python/data/` (1 049 lines of PyO3 `PyDccInfo` / `PySceneInfo` /
   `PyCaptureResult` …). Those three pulls are also the only reason the crate
   depends on `parking_lot`, `uuid`, and `pyo3`.

2. `dcc-mcp-http-types` is described as "wire-level value types for the DCC MCP
   HTTP server — no axum/tokio/reqwest", yet it depends on
   `dcc-mcp-gateway-core` for exactly one symbol, `GatewayPolicy`
   (`src/config/gateway.rs:5`, `src/config/aggregate.rs:9`). The arrow points
   from a transport-neutral configuration crate into the gateway domain crate.

3. `dcc-mcp-transport` owns "IPC/network transport mechanics", but ~4 700 of
   its 10 838 lines are DCC session-registry vocabulary: `discovery/types.rs`
   (1 475) defines `ServiceEntry` with `dcc_type`, `adapter_dcc`, `scene`,
   `documents`, and `host_pid`, plus the constant
   `GATEWAY_SENTINEL_DCC_TYPE = "__gateway__"` whose own doc comment admits the
   compromise — *"Defined at the transport layer so lower layers … can
   special-case it without depending on `dcc-mcp-http`"* (`discovery/types.rs:118`).

The driver for fixing this is a pending question, not an active migration: we
may eventually publish the protocol stack as its own crate/repository so that
MCP clients outside this workspace can reuse it. That migration is only cheap
if the stack stops reaching into DCC concepts first. Today the cost is paid
whether or not the split ever happens, because the DCC types inflate the
dependency ceiling of the crates every consumer must build.

Two constraints bound the work:

- **Python 3.7 red line (until 2026-12-31).** `src/lib.rs:70` registers 19
  sibling crates into the single `_core` PyO3 module, so the wheel needs the
  whole workspace. `compatibility/python.json` and seven CI workflows gate py37
  wheels, a pinned py37 test toolchain, and a full duplicate test-suite lane.
- **release-please.** `.release-please-manifest.json` drives a single version
  across 17 `extra-files`, and the workspace publishes **no** crate to
  crates.io today (zero `cargo publish` anywhere).

## Decision

Keep every crate in place and change no directory structure. Remove the three
DCC couplings one at a time, each as an independently mergeable commit. Per
ADR-030, published crate identities are preserved: these are internal
source-layout moves, not renames.

**Ordering amendment.** The ADR originally ordered the cuts by estimated cost
as Cut 1 → Cut 2 → Cut 3. Measurement during implementation reversed Cut 2 and
Cut 3: `dcc-mcp-transport`'s `discovery/` has **120** external consumer files
(gateway 70, CLI 18, sidecar 9, …), while the DCC adapter surface has **one**
real consumer (`dcc-mcp-usd`). Cut 2 is also blocked behind splitting
`TransportError` (513 lines), which mixes `UnsupportedDccLinkVersion` with
`ServiceNotFound` and `UnsupportedServiceEntrySchemaVersion`. The work
therefore proceeds Cut 1 → Cut 3 → Cut 2.

**Cut 1 — Invert `dcc-mcp-http-types` → `dcc-mcp-gateway-core`.**
Move `GatewayPolicy` from `dcc-mcp-gateway-core/src/policy.rs` into
`dcc-mcp-http-types`, and let `dcc-mcp-gateway-core` depend on
`dcc-mcp-http-types`. `GatewayPolicy` is a pure serde value type
(`read_only`, `allowed_dcc_types`, `allowed_skill_names`, …) with no I/O; its
own module doc already states that "transport layers decide how to serialize
those denials". There is no cycle to break — `dcc-mcp-gateway-core` does not
depend on `dcc-mcp-http-types` today. This is a one-line `Cargo.toml` change
plus two `use` lines.

`GatewayPolicyDenial` / `GatewayPolicyDenyReason` / `GatewayPolicyOperation`
stay in `dcc-mcp-gateway-core`: they carry denial semantics the gateway
evaluates, not operator configuration. Because `GatewayPolicy` keeps its
`allowed_dcc_types` field and `GatewayPolicyDenyReason` keeps its
`DccAllowlist` variant, DCC vocabulary does land in the "transport-neutral"
crate. We accept that: ADR-027 assigns *transport-neutral HTTP configuration
DTOs* to this crate, and an operator-written allowlist is configuration.

**Cut 2 — Split DCC-specific modules out of `dcc-mcp-transport`.**
Separate the generic transport mechanics — `connector.rs`, `listener/`,
`event_bridge.rs`, `error.rs` — from the DCC session-registry surface —
`discovery/` and `dcc_link.rs`. The generic half stays in
`dcc-mcp-transport`; the DCC half moves to a new `dcc-mcp-service-discovery`
crate that depends on it.

Even the "generic" half is not fully clean and the ADR says so:
`ipc/mod.rs` takes `dcc_type: &str` in `default_pipe_name` (`:87`),
`default_unix_socket` (`:94`), `default_local` (`:103`), and
`IpcConfig::pipe_path` / `socket_path` / `address_for` (`:378`, `:383`, `:389`).
These are generic mechanisms with a DCC-named parameter, so the split renames
the parameter to `service` rather than moving the code.

`dcc_link.rs` is the borderline case. Its frame format (ADR-028) is generic and
`DccLinkFrame`, `DccLinkType`, `IpcChannelAdapter`, and `SocketServerAdapter`
have **zero** use sites outside `dcc-mcp-transport` — only the Python bindings
re-export them. It moves with `discovery/` because its type-tag vocabulary and
name are DCC-shaped, not because it is functionally coupled.

**Cut 3 — Extract DCC adapter types out of `dcc-mcp-protocols`.**
Move `src/adapters/`, `src/python/data/`, `src/python/enums.rs`,
`src/python/scene_node.rs`, and the `#[cfg(test)]` `src/mock/` adapter into a
new `dcc-mcp-adapters` crate. This **amends ADR-027's ownership table**: the
row "MCP tool, resource, prompt, and adapter models → `dcc-mcp-protocols`" is
narrowed to "MCP tool, resource, and prompt models → `dcc-mcp-protocols`", and
a new row assigns "DCC adapter models, traits, and their PyO3 projections" to
`dcc-mcp-adapters`.

Two constraints discovered during implementation:

- `dcc-mcp-adapters` must **not** depend on `dcc-mcp-protocols`. Protocols
  re-exports adapters for backwards compatibility, so a dependency back would
  form a cycle. Adapters converts *into* MCP types at the boundary that knows
  both layers.
- The `mock/` module moves with the adapters because it implements the
  `DccAdapter` traits, and it is the reason `parking_lot` leaves the protocol
  crate.

Its blast radius is the smallest of the three: outside `dcc-mcp-protocols`,
only `dcc-mcp-usd` uses these types (`SceneInfo`, `SceneStatistics`,
`DccSceneInfo`, `DccResult`). `DccInfo`, `DccCapabilities`, `DccAdapter`, and
every `Py*` type have **zero** external use sites. `dcc-mcp-capture` matches
the moved type names in grep but has no dependency on `dcc-mcp-protocols`; its
`CaptureResult` is a separate type.

**Not in scope.** `dcc-mcp-jsonrpc`, `dcc-mcp-wire`, `dcc-mcp-models`, and
`dcc-mcp-naming` need no change. `dcc-mcp-naming` is confirmed a true leaf
(zero internal `dcc-mcp-*` dependencies) apart from DCC *policy* constants such
as `DEFAULT_DCC`, which are naming vocabulary rather than a dependency edge.

## Consequences

**Positive**

- The generic protocol stack (`jsonrpc`, `protocols`, `wire`, `http-types`,
  `naming`, and the generic half of `transport`) becomes independently
  publishable without first untangling DCC concepts.
- `dcc-mcp-protocols` drops `parking_lot`, `uuid`, and optional `pyo3`,
  lowering the build ceiling for its six dependents.
- The `GATEWAY_SENTINEL_DCC_TYPE` self-documented layering compromise
  (`discovery/types.rs:118`) disappears rather than being re-justified.
- Reverse-dependency churn stays bounded: Cut 1 touches 1 `Cargo.toml`, Cut 2
  up to 6, Cut 3 up to 6 — and in each case only 1–2 crates actually consume
  the moved types (`dcc-mcp-usd` for Cut 3; `dcc-mcp-cli` and `dcc-mcp-gateway`
  for Cut 2's discovery half).

**Negative / accepted**

- Two new crates (`dcc-mcp-adapters`, `dcc-mcp-service-discovery`) enter the
  workspace; the aggregate crate and root `Cargo.toml` `python-bindings` /
  `stub-gen` feature lists (`:268`, `:321`) must be re-pointed at whichever
  crate hosts the PyO3 projections.
- Cut 1 puts `allowed_dcc_types` and the `GatewayPolicy` name into
  `dcc-mcp-http-types`. Reviewers who read "transport-neutral" strictly will
  object; the answer is that operator-facing configuration is the crate's
  assigned concern.
- Cut 3 amends an accepted ADR. Until it merges, `dcc-mcp-protocols` remains
  the documented owner of adapter models.

**Explicitly deferred**

- Physical extraction into a separate repository. ADR-027 already notes that
  "crate consolidation is not required … a future merge needs separate
  dependency and compatibility evidence"; the same bar applies in reverse to a
  split.
- crates.io publishing. No `cargo publish` exists anywhere in this workspace
  today, so a publish lane would be built from nothing, and two release-please
  pipelines would then need manual ordering coordination.
- Any change that touches the py37 build chain before the 2026-12-31 red line.
  All three cuts are intra-workspace source moves; `src/lib.rs` keeps
  registering the same 19 modules, so the wheel's composition is unchanged.

## Alternatives Considered

- **Split the repository now.** Rejected: the py37 red line is under three
  months away and the release-please 17-file propagation would need per-crate
  rework plus a crates.io lane that does not exist. Doing the cuts first makes
  the split cheap later without paying the release-pipeline risk now.
- **Leave the couplings and document them.** Rejected: `discovery/types.rs:118`
  shows the pattern self-propagating — each new convenience placement is
  justified by the last one.
- **Move `GatewayPolicy` usage behind a trait or generic in
  `dcc-mcp-http-types`.** Rejected: `GatewayPolicy` does not touch
  `CapabilityRecord` in its own definition, only in evaluation methods, so a
  type parameter would add indirection without removing the dependency.
- **Merge `dcc-mcp-wire` and `dcc-mcp-jsonrpc` while we are here.** Rejected:
  ADR-027 explicitly requires separate dependency and compatibility evidence
  for that, and it is orthogonal to removing DCC coupling.
- **Extract `bridge.rs` as the DCC cut in `dcc-mcp-protocols`.** Rejected on
  evidence: `src/bridge.rs` contains no DCC types and no pyo3 — it is a generic
  WebSocket JSON-RPC 2.0 bridge protocol. The DCC surface is
  `src/adapters/` and `src/python/data/`.

## Related

- ADR-027 — the ownership table this decision amends (Cut 3).
- ADR-028 — the `DccLinkFrame` contract that moves with Cut 2.
- ADR-030 — published crate identities survive internal source moves.
- ADR-010 / ADR-033 / ADR-034 — the 2026-07-28 wire contracts that make the
  protocol stack the part worth isolating.
- ADR-011 — the Python 3.7 contract that defers physical extraction.
