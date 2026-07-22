# Architecture Decision Records (ADR)

This directory captures the non-reversible architectural decisions that shape
`dcc-mcp-core`. Each record is a short document written at the time the
decision was made, preserved as-is so that future contributors can understand
the trade-offs that were considered.

Format: [MADR-style](https://adr.github.io/madr/) — Status / Context /
Decision / Consequences / Alternatives considered.

| #   | Title                                                                                          | Status   |
| --- | ---------------------------------------------------------------------------------------------- | -------- |
| 001 | *(reserved — not yet written)*                                                                 | —        |
| 002 | [DCC Main-Thread Affinity](./002-dcc-main-thread-affinity.md)                                  | Accepted |
| 003 | [Thin Harness Skill Pattern](./003-thin-harness-skill-pattern.md)                              | Accepted |
| 009 | [Migrate MCP Transport to rmcp SDK](./009-rmcp-migration.md)                                   | Accepted |
| 010 | [MCP 2026-07-28 Dual Protocol Migration Strategy](./010-mcp-2026-07-28-dual-protocol-migration.md) | Proposed |
| 011 | [Python 3.7 LTS Compatibility Contract](./011-python-37-lts-compatibility-contract.md)        | Accepted |
| 012 | [Use OS-assigned ports for DCC instances](./012-os-assigned-dcc-instance-ports.md)             | Accepted |
| 013 | [Persist tool-call analytics locally and export studio telemetry through OTLP](./013-persistent-tool-call-analytics.md) | Accepted |
| 014 | [Isolate DCC UI Control behind a native session host](./014-isolate-ui-control-host.md)         | Accepted |
| 015 | [Bound Windows system configuration to operator grants](./015-bounded-ui-control-system-operations.md) | Accepted |
| 016 | [Unify application automation under UI Control naming](./016-unify-ui-control-naming.md) | Accepted |
| 017 | [Codex-style Record & Replay with visual closed-loop execution](./017-codex-record-replay-visual-closed-loop.md) | Accepted |

> Numbering is strictly sequential and never reused. ADR 001 is reserved for
> the first historical record; filling it in is tracked separately from any
> individual feature PR.
