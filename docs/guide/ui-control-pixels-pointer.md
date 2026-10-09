# Bounded pointer actions through pixels MCP

The canonical `ui_control__act` route accepts owner-granted `move` and `drag`
with one fresh ordinary pixels `snapshot_id`. This Core integration requires
a Native runtime that implements these operations in its guarded exact-pixels
dispatcher. Older runtimes refuse them; there is no fallback to a different
input route. Source contract tests are not live camera acceptance.

- `move`: `x` and `y` inside the latest screenshot; no held button.
- `drag`: 2–256 `path` points inside that screenshot and `button` equal to
  `left`, `middle`, or `right`. The Native operation must press, move and
  release within that single request.
- `duration_ms`: 1–1000, default 500. Modified drags and unrelated keyboard,
  text or scroll fields are refused by this bounded route.

Coordinates remain physical screenshot pixels. Do not supply desktop or
viewport-local coordinates or multiply by DPI. A viewport-local point must
first be mapped into the observed screenshot using verified application
geometry. Core forwards these pixels unchanged; Native validates and applies
the captured source origin, exact process/window creation identity, DPI,
bounds, generation, foreground state and authorization lease at dispatch.

Every dispatched operation consumes the observation. Take another ordinary
snapshot and read the actual camera or scene state before claiming an effect.
Prepared passive captures never grant input and cannot supply this token.

Native must release inputs on normal completion, cancellation, timeout,
instance/geometry change, task expiry and close, preserving failed or unknown
cleanup evidence. The Core transport cannot infer successful release from a
timeout, killed process or mocked acknowledgement. A compatible fixed Native
binary, its release contract tests and separate real camera E2E remain required.
