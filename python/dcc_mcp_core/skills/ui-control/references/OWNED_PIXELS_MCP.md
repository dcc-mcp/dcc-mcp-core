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

The separate owner operation `set_frame` uses metadata rather than pixels:
call `ui_control__act(action="get_window_state")`, then pass its
`context.window_state.window_state_id` and a complete physical
`frame={"x": ..., "y": ..., "width": ..., "height": ...}` to
`ui_control__act(action="set_frame", ...)` in the same session. All four values
must be integers, positive width/height and right/bottom extents must fit signed
32-bit coordinates, and negative monitor positions are valid. It never
activates or restores the window and requires no snapshot. Retain the actual
native requested/applied frame and instance. An attempt consumes the metadata
token and prior input observation; read new metadata before another frame
change and take fresh pixels before content input. No automatic retry occurs.

Semantic actions, find, semantic waits, menus, show/close window operations, and
resume are unsupported. `get_window_state` only reads the same exact target.
Do not switch transports, restart with another session id, or retry another
input route after authorization/policy/security/Escape/desktop failures.

Cleanup stops the owned task and owned MCP executable only. It does not stop
the shared Host, another adapter, or another logical consumer.

Recording is a separate optional owner ceiling:
`UiControlRecordingOptions(output_root=precreated_ordinary_directory)`.
Ambient recording environment variables cannot enable it. It grants no input,
does not start automatically, and uses this same task/session/owned child.
Call `ui_control__recording_start` manually, omitting output_dir and record_video;
the runtime chooses an immutable task directory and requires video only.
Read truthful paused/degraded/failed state with `ui_control__recording_state`,
then stop with `ui_control__recording_stop`. Preserve partial artifacts on error;
never call them finalized. The Host lifecycle id is `mcp-<task_id>`, distinct
from the native window-observation session id. Always finish with
`ui_control__stop_computer_use`, retaining the real ACK/failure/unknown cleanup
outcome. Repeated stops do not retry; force, nonzero exit or timeout never proves
cleanup. Decode and attribute actual MP4 and provenance separately.

All owned pixels tasks require an exact inactive/nonpending Host stop ACK,
including non-recording tasks (default cleanup budget five seconds). A runtime
that omits it produces cleanup_unknown, never a fabricated successful stop.
Automatic unload reports owned cleanup failures through executor warnings,
but `Core.stop()` returns no native receipt and catalog unload is not cleanup
acceptance. Retain the explicit `ui_control__stop_computer_use` ACK before Core
shutdown when proof of native cleanup is required.
