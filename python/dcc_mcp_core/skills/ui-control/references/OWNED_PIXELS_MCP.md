# Explicit owned pixels mode

This mode exists only when the adapter owner constructs
`dcc_mcp_core.server.UiControlRuntimeOptions` and supplies it as
`DccServerOptions.ui_control`. The configuration pins an absolute executable,
SHA-256, runtime version, action ceiling, and task TTL. Do not supply those
values as MCP tool arguments or change the installed shared Host.

Report `provider=dcc-cua`, runtime version, PID, and HWND before the first
snapshot or input. Both target ids must be owner-bound. Follow
`ui_control__snapshot` → one `ui_control__act` → fresh snapshot, then verify
actual application state. End with `ui_control__stop_computer_use`.

The returned `snapshot_id` is the native observation id. AX is intentionally
absent: `accessibility_state_id=null`, no semantic controls, no fabricated AX
token. Keep the logical UI session id, public task id, and native observation
session id distinct. Preserve `capture_provenance.native_capture_provenance`
and its exact native instance, geometry, DPI, generation, source/backend, plus
the task context when retaining evidence.

Physical `click`, `double_click`, `keypress`, `keyboard_shortcut`, and `type`
require the explicit owner action ceiling and the latest snapshot. Coordinates
are PNG-relative. Delivery is `effect=unverifiable`; re-observe and verify before
claiming success. Every attempted action consumes the previous observation.
Timeouts are not retried.

The default window mutation ceiling is empty. With explicit owner
`window_operations`, `activate_window` activates the exact window,
`restore_window` restores and activates it, and `minimize_window` consumes the
matching latest snapshot. They never happen implicitly or take pixels after
mutation. Retain actual foreground/native state and take a fresh snapshot
before more input. Physical acknowledgement also requires native
`delivery_completed` and `post_dispatch_validated`; it does not prove the edit.

Semantic actions, find, semantic waits, menus, recording, show/close window operations, and
resume are unsupported. `get_window_state` only reads the same exact target.
Do not switch transports, restart with another session id, or retry another
input route after authorization/policy/security/Escape/desktop failures.

Cleanup stops the owned task and owned MCP executable only. It does not stop
the shared Host, another adapter, or another logical consumer.
