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
| 014 | [Isolate DCC UI Control behind a native session host](./014-isolate-ui-control-host.md)         | Superseded by ADR-020 |
| 015 | [Bound Windows system configuration to operator grants](./015-bounded-ui-control-system-operations.md) | Superseded by ADR-020 |
| 016 | [Unify application automation under UI Control naming](./016-unify-ui-control-naming.md) | Partly superseded by ADR-020 |
| 017 | [Codex-style Record & Replay with visual closed-loop execution](./017-codex-record-replay-visual-closed-loop.md) | Accepted |
| 018 | [Instance Status Vocabulary Unification & Actionability](./018-cli-interaction-contract-workflow-consistency.md) | Proposed |
| 019 | [Build reproducible agent experiments on the session timeline](./019-reproducible-agent-experiments.md) | Proposed |
| 020 | [Consume application control through standalone dcc-cua](./020-external-cua-runtime.md) | Accepted |
| 021 | [Use content-addressed revisions for cross-DCC asset sync](./021-content-addressed-asset-sync.md) | Accepted |
| 022 | [Canonical Tool Result Envelope](./022-canonical-tool-result-envelope.md) | Accepted |
| 023 | [Installation-Bound Binary Updates](./023-installation-bound-binary-updates.md) | Accepted |
| 024 | [Immutable Marketplace Install Sources](./024-immutable-marketplace-install-sources.md) | Accepted |
| 025 | [Pinned Pip Adapter Artifacts](./025-pinned-pip-adapter-artifacts.md) | Accepted |
| 026 | [Verify Official Release Metadata with Sigstore](./026-sigstore-release-metadata.md) | Accepted |
| 027 | [Assign One Owner to Every Cross-Crate Protocol Type](./027-protocol-type-ownership.md) | Accepted |
| 028 | [Version DCC-Link Frames with a Tagged Header](./028-version-dcc-link-frames.md) | Accepted |

> Numbering is strictly sequential and never reused. ADR 001 is reserved for
> the first historical record; filling it in is tracked separately from any
> individual feature PR.
