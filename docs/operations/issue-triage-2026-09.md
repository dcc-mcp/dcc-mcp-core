# Issue triage — 2026-09-12

This review records the disposition of the currently open Core issues after checking their scope, dependencies, and available implementation evidence.

## Continue / prioritize

| Issue | Disposition | Reason |
| --- | --- | --- |
| #2417 | Continue, P0 correctness | Concrete 3ds Max reproduction reports a one-call response skew. It needs request correlation and a fail-closed mismatch path before any retry or async behavior is trusted. |
| #2405 | Continue, P1 reliability | A live port holder can accept TCP while the service is dead. Application-level readiness and bounded election recovery are still required. |
| #2436 | Continue, protocol conformance | Depends on the final MCP 2026 wire contract and needs an official SDK round-trip oracle. |
| #2403 | Continue, lifecycle | Project-bound launch is an adapter-facing capability with explicit authorization and readiness evidence; no Core-only shortcut should be merged. |
| #2382, #2387 | Continue, dependency gated | Catalog synchronization and OBS registration require independently verifiable adapter releases. |
| #2384, #2260, #2261, #2262, #2270 | Continue, roadmap/umbrella | These are cross-cutting acceptance and verification tracks. They should remain open until their downstream contracts and evidence exist. |

## Superseded / close

| Issue | Disposition | Evidence |
| --- | --- | --- |
| #2256 | Close as superseded by #2417 | #2256 is an umbrella transport-correctness proposal. #2417 contains the current concrete reproduction, impact, and acceptance boundary for the request-correlation failure. Long-running jobs remain independently tracked by #2262. |

## Keep open without implementation in this pass

#2297–#2300, #2252, #2253, #2259, #2263, and #2277 remain valid but require coordinated design or adapter-owned work. Closing them would hide unfinished contracts; implementing them opportunistically in Core would create scope drift.

This document is a triage record, not evidence that a real-host or licensed acceptance gate passed. Any implementation PR must re-check the exact issue head, run the relevant source and integration tests, and obtain fresh CI evidence before merge.
