# Passive temporary capture preparation

This adapter consumes the fixed native source contract identified by contract
SHA256 `7ae0f425e391620bb4fd22baf32db3a19c603cbc40a7a6f584dad71b50890221`.
Native binary binding and live application acceptance are separate gates. Do not
enable this option for the older `.11` binary, which lacks this protocol.

Trusted bootstrap code can pass `UiControlCapturePreparationOptions(journal_root=...)`
as `UiControlRuntimeOptions.capture_preparation`. The directory must already be
ordinary, absolute, stable across tasks, and distinct from recording output.
The owned transport clears inherited journal configuration and supplies only this
operator-selected root. Tool arguments cannot select the root or widen authority.
`allow_snapshot=False` grants lifecycle/status without passive image capture.
This option grants neither content input nor ordinary foreground activation.

Use the canonical discovery/describe/call route for `ui-control`. Keep one explicit
logical session, exact PID/HWND and owner runtime throughout:

1. Call `ui_control__act` with `action="get_window_state"`.
2. Call `ui_control__capture_preparation` with `operation="begin"`, the returned
   `window_state_id`, and `lifetime_ms` from 1 through 30000. Native metadata is
   single-use and at most five seconds old; native also bounds duration by the
   trusted task lease.
3. Use `operation="state"` and inspect the actual phase. An accepted begin may
   still be pending. Call `operation="snapshot"` only for an active preparation.
4. Call `operation="stop"`; require `capture_revoked=true` and
   `cleanup_verified=true`. Pending/unknown restoration is not success. Finish
   the task with `ui_control__stop_computer_use`, retaining its cleanup receipt.

State/stop/snapshot take no metadata token or lifetime. Every operation runs under
the existing session lock. Scope checks remain active when policy is narrowed;
read/status and revocation do not require snapshot or mutation policy. Native
instance, preparation identity, original state and fixed deadline cannot change.
Gateway-disabled direct MCP discovery does not require a manufactured gateway UUID.

Passive evidence uses schema `dcc-cua-passive-prepared-evidence-v1`. It preserves
exact native identity, actual foreground at capture/publication, physical bounds,
image dimensions and monotonic capture time. PNG bytes must match their descriptor.
No actionable snapshot, observation, accessibility or element token is returned
or stored. Existing action evidence is invalidated. For later input, stop and
verify restoration, then separately satisfy ordinary input permission and fresh
foreground observation requirements.

Cancellation observed after native begin or capture requests one stop and retains
the real restoration status. Core does not preempt an in-flight native call or
replay a timed-out mutation. Transport loss remains cleanup_unknown until native
guardian/journal evidence establishes restoration; successful process exit alone
does not establish it. Structured native failure reasons are preserved without
parsing human-readable error messages. Session stop must include the preparation
cleanup component whenever preparation was attempted.
