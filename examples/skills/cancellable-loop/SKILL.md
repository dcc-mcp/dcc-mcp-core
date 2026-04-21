---
name: cancellable-loop
description: "Demonstrates cooperative cancellation inside a skill script loop using check_cancelled(). Use when showing how long-running skills should respond to notifications/cancelled."
license: MIT
compatibility: Python 3.7+
tags: [example, cancellation, long-running]
dcc: python
version: "1.0.0"
search-hint: "cancellation, cancel, long-running, cooperative, check_cancelled, abort"
---

# Cancellable Loop

A minimal example that shows how to write a skill script that honours
`notifications/cancelled` from the MCP client.

The pattern is simple: call `check_cancelled()` at the top of every
iteration of a long-running loop.  When the dispatcher installs a
`CancelToken` and the client cancels the request, `check_cancelled()`
raises `CancelledError` and the script unwinds cleanly.  Outside of a
request context (REPL, unit tests) `check_cancelled()` is a no-op, so
the same script remains easy to run in isolation.

## Tools

- `cancellable_loop__count` — Iterate `iterations` times, sleeping
  `sleep_ms` milliseconds per step, checking for cancellation each
  iteration.

## Example

```python
{"name": "cancellable_loop__count", "arguments": {"iterations": 100, "sleep_ms": 50}}
# → {"success": true, "message": "Completed 100 iterations", "context": {"iterations": 100}}
```

If the client sends `notifications/cancelled` while the loop is
running, the next `check_cancelled()` call raises `CancelledError` and
the `@skill_entry` wrapper converts it into a standard error dict.

## Related

- `dcc_mcp_core.check_cancelled` — the API this skill demonstrates.
- Issue #329 — cooperative cancellation checkpoints.
- Issue #318 — async dispatcher integration (wires the CancelToken).
