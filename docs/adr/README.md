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

> Numbering is strictly sequential and never reused. ADR 001 is reserved for
> the first historical record; filling it in is tracked separately from any
> individual feature PR.
