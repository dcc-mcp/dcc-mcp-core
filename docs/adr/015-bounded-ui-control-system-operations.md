# ADR-015: Bound Windows system configuration to operator grants

## Status

Accepted

## Context

Some DCC plug-ins need setup outside the application window. Typical examples
are an `HKEY_CURRENT_USER` value, a file or directory symbolic link, and an
in-application switch that enables a remote-control endpoint. Custom DCC tools
may also expose face-shaping or rig controls only through a painted canvas.

ADR-014 intentionally scopes UI Control to one operator-bound PID/HWND. A
registry value or symbolic link may be required before that window exists, so
pretending it is a window action would weaken the capability model. Invoking
PowerShell, `reg.exe`, `cmd /c mklink`, or an arbitrary executable would be a
larger and unauditable authority than these setup tasks require.

Credentials are a separate concern. Passing passwords through an agent-facing
text field or a generic system-operation value would expose them to transports,
traces, and model context. Windows logon, credential dialogs, password managers,
UAC, and security/privacy surfaces therefore remain outside this decision.

## Decision

### Keep application and system authority separate

Application interaction remains on the ADR-014 path:

- bind one exact PID/HWND;
- prefer a native DCC tool, then UI Automation;
- use observation-fenced pointer input only as a fallback;
- take a fresh snapshot after every mutation.

Pointer actions may hold Ctrl, Shift, or Alt while clicking, scrolling, or
dragging. This covers custom canvases, viewport manipulators, and face-shaping
controls without adding application-specific primitives. Controls whose text
contains `remote control`, `remote connection`, or `allow remote` are treated
as account/access changes and always require action-time confirmation.

Add a separate `ui_control__system_operation` tool, exposed by
`dcc-mcp-cli ui-control system-operation`. It opens a short-lived system
session and does not accept a PID, HWND, snapshot, coordinate, command, or
executable. Protocol v2 advertises this through the
`typed_system_operations` capability.

### Require an operator-owned exact grant

The native host loads an operator-owned JSON catalog from the absolute path in
`DCC_MCP_UI_CONTROL_SYSTEM_GRANTS_FILE`. The adapter selects one entry through
`DCC_MCP_UI_CONTROL_SYSTEM_GRANT_ID`. Agent-facing arguments cannot supply or
widen the catalog.

Each catalog entry contains a stable grant id, a DCC type, and named typed
operations. Each name is a non-sensitive identifier; the complete desired
operation, including its value or paths, remains inside the operator-owned
catalog. For example:

```json
[
  {
    "system_grant_id": "photoshop-plugin-setup",
    "dcc_type": "photoshop",
    "operations": [
      {
        "operation_id": "enable-remote-control",
        "operation": {
          "type": "ensure_registry_dword",
          "key": "Software\\Vendor\\Plugin",
          "value_name": "RemoteEnabled",
          "value": 1
        }
      },
      {
        "operation_id": "link-vendor-plugin",
        "operation": {
          "type": "ensure_directory_symlink",
          "link": "C:\\Users\\Artist\\Documents\\Dcc\\Plug-ins\\Vendor",
          "target": "D:\\Studio\\Plug-ins\\Vendor"
        }
      }
    ]
  }
]
```

The catalog is read by the already isolated, current-user UI Control host. A
client sends only the grant id and a non-sensitive operation id. The native
host resolves the typed operation locally; registry values and symbolic-link
paths never cross the agent-facing tool or named-pipe boundary. The host mints
an opaque system capability owned by that named-pipe connection and invalidates
it when the operation completes or the connection closes.

### Support only bounded idempotent operations

The first protocol slice contains four operations:

- ensure one HKCU `REG_SZ` value;
- ensure one HKCU `REG_DWORD` value;
- ensure one file symbolic link;
- ensure one directory symbolic link.

Registry operations read back the current state and return `created`,
`updated`, or `unchanged`. Symbolic links require an existing target and an
existing canonical parent. An identical link returns `unchanged`; any other
existing object returns `conflict` and is never replaced.

Every mutation requires the trusted native confirmation surface. The dialog
shows the exact registry key/value name or link/target, but not the registry
value. Audit events and tool results contain only the static operation kind,
outcome, policy tier, and a redacted message. The catalog is trusted operator
configuration, not a credential store; the host deliberately avoids heuristic
"secret detection" and the agent-facing protocol has no value field at all.

The host rejects:

- HKLM and alternate registry hives;
- Windows Run, RunOnce, and policy-based autorun keys;
- registry deletion or arbitrary registry types;
- link deletion, replacement, relative paths, UNC paths, and device paths;
- links under Windows, Program Files, or ProgramData;
- shell commands, scripts, executable launch, and arbitrary file operations;
- automatic elevation, UAC interaction, and secure-desktop automation.

Permission failures return `elevation_required`; they do not launch an elevated
process. A future privileged broker requires its own ADR, signed short-lived
binary, exact manifest, and per-operation UAC consent.

### Keep credential input as a hand-off

UI Control may fill non-sensitive account fields and navigate an application
login flow. Password controls, authentication surfaces, password managers, and
password changes remain hard denied. The agent pauses for user input or follows
an application-owned OAuth/browser flow, then takes a fresh snapshot before
continuing. A future credential broker must use opaque grants or a host-owned
secure prompt; it must not reuse `text` or registry string values.

### Preserve the capture worker process boundary

The process executing synchronous `PrintWindow` remains a separate,
short-lived, killable child, but it re-enters
`dcc-mcp-ui-control-host.exe --dcc-mcp-ui-control-capture-worker` instead of
requiring a second shipped executable. Windows core wheels, server wheels, and
server bundles therefore package only `dcc-mcp-ui-control-host.exe`. The host
process spawns a new copy for every bounded capture; the long-lived host never
calls `PrintWindow` itself.

There is no legacy worker compatibility. Discovery and the optional
`DCC_MCP_UI_CONTROL_HOST` override select the current host, and only
`--dcc-mcp-ui-control-capture-worker` enters capture-worker mode. The standalone
server keeps the same one-file fallback for raw binary installations.

## Consequences

### Positive

- common plug-in setup no longer requires a generic shell;
- operations are deterministic, idempotent, operator-scoped, and auditable;
- setup can run without manufacturing a window capability;
- custom DCC face controls gain modifier-assisted drag without DCC-specific
  branches;
- the capture deadline remains enforceable by process termination.

### Negative

- operators must create an exact grant catalog before invoking setup;
- grants are loaded by the session host, so deployment must make the catalog
  available before that host starts;
- protected locations and plug-ins that genuinely require elevation still
  require a separate, human-approved installer path;
- password entry is not fully autonomous.

## Alternatives Considered

### Add unrestricted PowerShell or terminal control

Rejected because a command string can escape every typed grant, target scope,
and idempotence guarantee.

### Treat registry and links as ordinary window actions

Rejected because these resources can exist before a DCC window and do not have
meaningful screenshot or UIA observation fences.

### Allow the client to submit its own allowlist

Rejected because the agent could widen its own authority. The host-owned
catalog is the source of truth.

### Execute capture inside the long-lived host

Rejected because the separate process provides the killable `PrintWindow`
boundary. Reusing one executable reduces packaging surface without merging the
runtime boundary.

## References

- [ADR-014](./014-isolate-ui-control-host.md)
- `crates/dcc-mcp-ui-control/src/host_protocol.rs`
- `crates/dcc-mcp-computer-use/src/ui_control_host.rs`
- `crates/dcc-mcp-capture/src/capture_worker.rs`
- `python/dcc_mcp_core/skills/ui-control`
