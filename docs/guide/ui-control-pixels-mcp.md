# Owned pixels MCP runtime

An adapter can explicitly select a verified DCC-CUA public-MCP executable for
the bundled `ui-control` skill. The default remains the shared Host JSONL
transport. This option is generic across DCC applications and requires an
exact PID and HWND from the server owner.

```python
from pathlib import Path
from dcc_mcp_core.server import DccServerOptions, DiagnosticsOptions, UiControlRuntimeOptions

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
recording, show/close window operations, and resume are unsupported by this transport.
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

Physical acknowledgement requires native `delivery_completed` and
`post_dispatch_validated`. Delivery returns `effect=unverifiable` and requires application-state
verification. It is not a claim that the requested edit or UI operation
succeeded. Stop, skill unload, or server shutdown revokes only the owned task
and closes only its executable; the shared installed Host remains independent.

Native failures retain bounded content-free `details`, task context, and native
delivery evidence when provided. `input_sent=not_sent`, `sent`, and `unknown`
remain distinct. A failed or uncertain mutation still consumes its observation
and requires fresh pixels; it never triggers a blind retry or implicit activation.

The focused `tests/test_cua_mcp_pixels.py` suite exercises public MCP envelopes,
real bundled script dispatch, owner-only selection, native pixel fences, and
cleanup with in-memory pipe fixtures. These tests do not certify Windows
capture/input, a visible DCC session, or a new binary release. Native acceptance
must use the same formal bundled-tool route against the verified runtime and
read back the application's state after input.
