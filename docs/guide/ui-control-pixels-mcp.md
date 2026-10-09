# Owned pixels MCP runtime

An adapter can explicitly select a verified DCC-CUA public-MCP executable for
the bundled `ui-control` skill. The default remains the shared Host JSONL
transport. This option is generic across DCC applications and requires an
exact PID and HWND from the server owner.

```python
from pathlib import Path
from dcc_mcp_core.server import DccServerOptions, DiagnosticsOptions, UiControlRuntimeOptions, UiControlRecordingOptions

options = DccServerOptions(
    dcc_name="maya",  # Any supported adapter identity, including unreal.
    builtin_skills_dir=Path(adapter_skills),
    diagnostics=DiagnosticsOptions(dcc_pid=owned_pid, window_handle=owned_hwnd),
    ui_control=UiControlRuntimeOptions(
        binary=verified_binary_path,  # Absolute operator-selected executable.
        sha256=verified_binary_sha256,
        runtime_version=verified_runtime_version,
        allowed_actions=("click", "double_click", "keypress", "keyboard_shortcut", "type"),
        window_operations=("activate", "restore_activate", "minimize"),  # Optional; empty by default.
        ttl_minutes=15,
        # Optional manual video recording; the ordinary directory must exist.
        recording=UiControlRecordingOptions(output_root=operator_recording_root),
    ),
)
```

Pass these options to the adapter's existing `DccServerBase` constructor and
register its existing `HostExecutionBridge` before loading skills. The public
`DccServerOptions.from_env(..., ui_control=...)` constructor accepts the same
typed value explicitly; it does not read an executable, digest, mode, or action
scope from tool arguments or new global environment settings. An empty
`allowed_actions` tuple grants no physical input. `window_operations` is a
separate explicit window-mutation ceiling and is also empty by default. The existing
`DCC_MCP_CUA_ALLOW_RAW_INPUT=false` ceiling also removes physical action grants.

Core hashes the binary before launching exactly `binary mcp-server`, verifies
the MCP runtime identity/version and required task tools, and opens one bounded
`pixels_only` task. It never invokes `host-ensure`, connects to the shared Host,
or generates an authorization lease. The runtime's public task broker owns
authorization. An existing policy refusal or Escape interruption remains a
stop condition, with no automatic resume or transport fallback.

Before the first observation or input, report `provider=dcc-cua`, the pinned
runtime version, and the owner-bound target PID/HWND. Then use the ordinary
`ui_control__snapshot` → one `ui_control__act` → `ui_control__snapshot` loop and
finish with `ui_control__stop_computer_use`. Tool calls cannot select this
transport or replace its executable/action ceiling.

In this mode `snapshot_id` is the native `observation_id` and
`accessibility_state_id` remains JSON null. The logical UI `session_id`, public
MCP `task_id`, and native observation `session_id` are distinct identifiers.
The result preserves the full native capture provenance, exact native instance,
source rectangle, geometry, DPI, generation, backend, and task context. A pixel
snapshot exposes no invented accessibility tree or control ids.

Use coordinates from that PNG for `click`/`double_click`, or the existing
`keypress`, `keyboard_shortcut`, and `type` actions within the explicit owner
ceiling. Semantic actions, `find`, semantic `wait_for`, native menu invocation,
show/close window operations, and resume are unsupported by this transport.
They fail explicitly instead of selecting another backend. `get_window_state`
is a read of the same exact target. A new native capture is required after any
physical action attempt, including a failed or uncertain attempt. Requests are
never replayed after timeout.

Only owner-granted `activate_window`, `restore_window` (native
`restore_activate`), and `minimize_window` are supported window mutations.
Activation and restore are explicit calls and never happen during startup,
capture, or an input retry. Minimize requires the matching latest `snapshot_id`
and forwards that native observation to the runtime's window-state authorization
scope. Every window attempt consumes the prior observation; take fresh pixels
before further input. Results retain actual native state, foreground status,
task context, and completion evidence without an implicit follow-up capture.

An owner can additionally grant `window_operations=("set_frame",)` without
physical input or recording. Call `ui_control__act(action="get_window_state")`
and pass `context.window_state.window_state_id` to one
`ui_control__act(action="set_frame", window_state_id=..., frame={"x": ..., "y": ...,
"width": ..., "height": ...})` in the same session. This short-lived native
metadata token does not authorize content input and needs no screenshot. Every
value must be an integer, width and height must be positive, and the complete
physical Win32 outer frame, including right/bottom extents, must fit signed
32-bit coordinates. Negative monitor coordinates are supported. The native
operation preserves activation and z-order and does not restore a minimized
window. A failed or uncertain attempt consumes the token; explicitly read new
metadata before another frame request and take fresh pixels before content
input. Retain the exact requested/applied frame, native instance, actual state,
task context and any native error instead of replaying the request.

Physical acknowledgement requires native `delivery_completed` and
`post_dispatch_validated`. Delivery returns `effect=unverifiable` and requires application-state
verification. It is not a claim that the requested edit or UI operation
succeeded. Stop, skill unload, or server shutdown revokes only the owned task
and closes only its executable; the shared installed Host remains independent.

Native failures retain bounded content-free `details`, task context, and native
delivery evidence when provided. `input_sent=not_sent`, `sent`, and `unknown`
remain distinct. A failed or uncertain mutation still consumes its observation
and requires fresh pixels; it never triggers a blind retry or implicit activation.

Recording is disabled unless the owner supplies `UiControlRecordingOptions`.
The owned child strips ambient `DCC_CUA_RECORDING_OUTPUT_ROOT`; the typed option
sets this variable only in that child. It grants no physical input or window
mutation and does not start a recorder during task creation. The native broker
allocates an immutable child directory under the precreated ordinary local root.
The operator must retain ownership of that root; path checks are not an atomic
filesystem sandbox against concurrent changes by another local process.

Call `ui_control__recording_start` manually in the same UI session, omitting
`output_dir` and `record_video`. An optional `output_dir` must exactly equal the
runtime-authorized task directory; `record_video=false` is refused. The existing
`recording_state` and `recording_stop` tools use that same task, Host session,
producer and owned executable. Pixels recording is video only, with
`trajectory_available=false` and `trajectory=null`. Retain paused, degraded,
failed, terminal-source and partial-artifact evidence. After a native stop error,
read `recording_state` explicitly; an error is never successful finalization.

The Host lifecycle id is `mcp-<task_id>`, distinct from the native observation's
window-session id. Recording cleanup uses an owner-bounded wait greater than
60 seconds and at most 65 seconds, aligned with the native 60-second deadline.
`stop_computer_use` retains the actual inactive/nonpending ACK, or the failed or
unknown conclusion. Repeated stops do not retry or change that conclusion.
A forced/nonzero child exit or timeout cannot certify cleanup. Recording does
not extend the task lease. Decode and attribute real MP4/sidecar output in a
separate native acceptance run; protocol tests are not recording acceptance.

### Owner-required recording progress

Set `UiControlRecordingOptions(output_root, require_progress=True)` to require
typed media-sample progress. The default remains `False`. This owner option
does not change tool parameters, authorize input, or start recording.
The pinned executable must advertise the exact
`dcc-cua.recording-sample-progress.v1` descriptor. The actual connected Host
must advertise that contract in its negotiated hello; Core retains its
`host_connection_id` and checks the same connection on subsequent responses.
An outer descriptor or requested capability alone is insufficient. Cached
Host capability proof identifies the implementation, not current health.

`recording_start` establishes a baseline. It does not certify advancement.
`recording_state` succeeds only when `video.media_sample_progress` advances
within the same recording interval, task, native window instance, and source
stream, while recording, video, and source remain active and healthy.
The tuple contains `recording_interval_id`, `media_samples_admitted`, and
`latest_admitted_source_sequence`; sequence zero is valid. Admission means a
nonempty OpenH264 media sample with source provenance. It does not certify
muxing, durable output, finalization, or application motion.

The retained `recording_progress` projection distinguishes `advanced`,
`stalled`, `regressed`, `baseline_required`, and `unavailable`. A new interval
or stream requires a new baseline. Missing or malformed evidence cannot be
replaced by file size, timestamps, or encoded-frame estimates. Failed state
RPCs do not advance the baseline. Core does not poll or invent a deadline.
`recording_stop` and task cleanup remain available without advancement;
they clear the baseline and preserve their existing cleanup receipts.
Strict progress failures retain bounded recording evidence through the
bundled `ui-control` scripts. Real recording acceptance still requires the
matching formal wheel, Native executable, and connected Host.

Every owned pixels task validates this cleanup ACK, including tasks without
recording permission. Those tasks keep the default five-second cleanup budget
and have no recording destination constraint. A pinned older runtime that omits
the actual Host ACK cannot certify cleanup; Core retains `cleanup_unknown`.
The legacy shared JSONL transport retains its existing stop semantics.

Automatic skill unload and Core shutdown attempt every client. Owned pixels
cleanup failures retain their cached typed outcome and reach the existing
executor warning hooks with bounded native evidence; legacy shared JSONL
cleanup remains best effort. `Core.stop()` returns no native cleanup receipt,
and catalog unload success does not certify it. Call
`ui_control__stop_computer_use` explicitly and retain its actual ACK before stopping
Core when the caller requires cleanup proof.

When the native ACK exports typed `recording_video` or `live_observation`
summaries, Core retains bounded partial/segment paths inside the authorized task
directory, actual sidecar hashes/counters and source cleanup state. An unknown
cleanup's retained Host response must still name the same logical Host session.
Malformed component receipts cannot certify success. These are reported native
receipts; Core does not infer file existence or successful MP4 decoding from them.

New runtimes may expose `native_visible_bounds` separately from Win32 outer
bounds and `wgc_geometry` with actual frame/item/pool/content/texture sizes.
PNG dimensions must match the physical source rectangle without scaling. A WGC
origin must uniquely match Win32 or DWM bounds; equal-sized rectangles at
different origins are refused. Older pinned runtimes remain usable without
inventing these newer provenance fields.

The focused `tests/test_cua_mcp_pixels.py` suite exercises public MCP envelopes,
real bundled script dispatch, owner-only selection, native pixel fences, and
cleanup with in-memory pipe fixtures. These tests do not certify Windows
capture/input, a visible DCC session, or a new binary release. Native acceptance
must use the same formal bundled-tool route against the verified runtime and
read back the application's state after input.

## Explicit foreground preparation

Use `ui_control__prepare_foreground(session_id=..., process_id=...,
window_handle=..., operation="restore_activate")` when the task requires the
exact application in front before capture. `operation="activate"` is available
for an already visible target. Both window mutation and snapshot policy must
permit the call; owned pixels additionally requires the matching
`window_operations` entry. The call grants no new content input permissions.

The retained client performs the explicit mutation, independently reads actual
foreground/visibility, then captures a fresh observation before returning its
`snapshot_id`, pixels and provenance. It never switches HWND within the same
PID, rebinds an existing logical session, resumes stopped control, retries, or
sends content input. Every attempted window mutation consumes the prior snapshot.
The ordinary `ui_control__snapshot` path remains free of foreground mutation.

The `foreground_preparation` result identifies `binding`, `authorization`,
`activation`, `foreground_readback`, `capture`, or `ready`. Activation and
readback receipts remain available when capture fails. Capture reports its
`semantic` or `pixels_only` mode and retains native error details such as
`root_overlap`; a successful activation does not turn a rejected screenshot
into success. Only `ready` carries a new usable snapshot; one subsequent input
still requires the existing owner permission and fresh native checks. Verify
its application effect and finish with `ui_control__stop_computer_use`.

Preparation checks request and host-job cancellation after acquiring the session
lock, before mutation, and between native stages. Cancellation waits for an
already dispatched native call to return; it prevents the next stage and returns
`cancelled` with completed receipts, no screenshot or usable input token. Any
resolved pixels client's action evidence is invalidated, including pixels
captured immediately before cancellation. The retained session remains available
for explicit stop. Ordinary activation is persistent: cancellation does not
claim to restore previous foreground or window ordering.
