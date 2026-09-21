# Request for Comments

This directory holds design proposals that are still in the RFC stage. RFCs
are intentionally more exploratory than ADRs: they capture problem framing,
constraints, phased designs, alternatives, and open questions before the
project accepts or rejects a direction.

Each RFC should be independently reviewable and should keep these constraints
explicit:

1. No new required processes for existing gateway or adapter deployments.
2. Flat packaging: downstream adapters inherit behavior through their existing
   `dcc-mcp-core` dependency.
3. Multi-DCC by construction: core behavior must not assume Maya, Blender, or
   any single host.
4. Backward compatible by default: new behavior should be additive, opt-in, or
   safely negotiated.

## Active RFCs

| ID | Title | Solves | Status |
| --- | --- | --- | --- |
| [0001](./0001-gateway-election-resilience.md) | Gateway Election Resilience | MCP clients dropping when a co-existing DCC starts; automatic failover when the gateway owner crashes; graceful handoff when the owner exits cleanly | Draft |
| [0002](./0002-event-bus-and-webhooks.md) | Event Bus & Webhooks | Downstream DCC integrators need lifecycle, tool, and skill hooks without forking the server | Draft |
| [0003](./0003-traffic-interception-and-replay.md) | Traffic Interception & Agent Debugging | Skill authors need opt-in protocol capture, replay, and diff tooling for empirical agent iteration | Draft |
| [0004](./0004-modeling-spec-gate.md) | Modeling Spec Gate & Acceptance Contract | Modeling defects survive every machine stage and surface only at lookdev or compositing; the existing spec evaluator is untyped and never invoked | Draft |
| [0005](./0005-modeling-capability-ranking-and-first-experiment.md) | Modeling Capability Ranking & First Experiment | Five modeling-quality capabilities compete for the same slot, and none of them can currently be shown to work | Draft |

## Dependency graph

```text
0001 (election)

0002 (event bus)
   |
   +-- depended on by --> 0003 (traffic interception)
                          traffic frames consume the EventBus

0004 (spec gate) -- no code dependency; touches only the
                   verification schema family and the
                   declaration lint

0005 (eval set) -- no code dependency; consumes the existing
                  dcc_mcp_core.verification primitives and the
                  ADR-019 experiment-event model
```

0001 and 0002 can land in either order. 0003 is gated on 0002 P0, the
`EventBus` primitive.

0004 is independent of 0001 through 0003: it touches only the verification
schema family and the declaration lint, and adds no runtime dependency on
the event bus or on gateway election.

0005 is independent of 0001 through 0004 and can land in any order relative
to them. It is listed last only because it is the newest. It shares the
verification surface with 0004 but adds no dependency on it: the spec gate
declares what a modeling result must satisfy, while the eval set measures
whether current code satisfies it.

## Recommended landing order

1. 0001 P0+P1: switch election from version-driven preemption to
   liveness-driven failover.
2. 0002 P0+P1: add the `EventBus` primitive and initial `tool.*` /
   `skill.*` emit points.
3. 0003 P0: add the `traffic.frame` event and JSONL sink.
4. 0001 P1.5: add pre-shutdown gateway handoff.
5. 0002 P3: add webhook delivery.
6. 0003 P2+P3: add replay and diff CLIs.

Accepted decisions that become project policy should be moved or summarized in
`../adr/` once the implementation direction is no longer exploratory.
