# ADR-019: Build reproducible agent experiments on the session timeline

## Status

Proposed — 2026-07-27

## Context

DCC-MCP needs repeatable scenarios, branched agent sessions, comparable run
metrics, judge evidence, and audit-ready replay. Core already owns the required
runtime foundations:

- gateway traces and `session_events` provide ordered, attributable history;
- sessions already preserve parent-session relationships;
- `WorkflowSpec`, recordings, and VRS cover workflow, operator, and protocol
  replay respectively;
- Admin SQLite, audit rows, stats, and OTLP provide local retention and export;
- artifacts and validation results preserve DCC-native evidence.

Adding another recorder, database, runner service, or provider SDK would split
the source of truth without improving runtime correctness.

## Decision

Represent experiments as versioned events in the existing session timeline:

- `experiment.created` defines a named scenario and optional workflow or
  recording reference;
- `experiment.run.<status>` records the latest state, parameters, metrics,
  evidence, seed, and optional parent run/session;
- `experiment.judge.result` records bounded evaluator output with
  `authority: evidence_only`.

The Admin API projects these events into experiments, latest run states, a
Session DAG, aggregate status counts, judge results, and the ordered audit
timeline. It does not add a second persistence model.

The initial REST surface is:

| Method | Route | Purpose |
| --- | --- | --- |
| `GET`, `POST` | `/v1/experiments` | List or create experiments. |
| `GET` | `/v1/experiments/{experiment_id}` | Read the projected experiment. |
| `POST` | `/v1/experiments/{experiment_id}/runs` | Append a run state. |
| `POST` | `/v1/experiments/{id}/judge-results` | Append judge evidence. |

Write requests require a bounded `x-dcc-mcp-agent-session-id`. Identifiers,
metadata, summaries, scores, and evidence references are size-bounded at the
HTTP trust boundary. Judge output cannot approve a run, widen tool authority,
or replace deterministic DCC validation.

Scenario execution continues to use existing workflow and recording paths.
Experiment records only link to those identifiers; it does not duplicate their
execution or replay logic.

## Consequences

### Positive

- One canonical timeline supports trajectories, Session DAGs, metrics, judge
  evidence, auditing, and replay references.
- Existing redaction, retention, attribution, and SQLite lifecycle apply.
- No new runtime dependency, service, database table, or credential boundary.

### Negative

- Experiment detail currently performs a bounded scan of retained session
  events. Add an indexed projection only after retained volume makes this a
  measured bottleneck.
- The Admin dashboard is read-only; experiment scheduling and execution remain
  owned by existing workflow and recording paths.

## Alternatives considered

### Add a separate experiment store

Rejected. It duplicates session ordering, retention, redaction, and audit
ownership.

### Add a generic evaluator framework

Rejected until more than one concrete evaluator needs a shared execution
contract. The current result envelope is sufficient for persistence and audit.

### Treat judge output as acceptance authority

Rejected. DCC artifacts and deterministic or explicitly human-approved
validation remain authoritative.

## References

- [ADR-013](./013-persistent-tool-call-analytics.md)
- [ADR-017](./017-codex-record-replay-visual-closed-loop.md)
- [Traffic interception and replay RFC](../rfcs/0003-traffic-interception-and-replay.md)
