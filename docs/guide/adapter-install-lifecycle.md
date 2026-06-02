# Adapter Install Lifecycle

Adapter installers often run inside the DCC process they are updating. On
Windows, importing `dcc_mcp_core._core` loads `_core.pyd`; that native module
stays locked until the process exits, so an uninstall or upgrade can fail while
removing the adapter's bundled package tree.

Use `dcc_mcp_core.install_lifecycle` for installer and uninstaller code that
must stay import-light. The module uses only the Python standard library and
does not import `_core`.

## Rez Or Filesystem Deployment Layout

Pipeline teams can use the same bootstrap script before packages are formally
built. Resolve package roots first, then prepend the returned paths to the
process environment that launches the sidecar or gateway:

```python
from dcc_mcp_core.install_lifecycle import resolve_deployment_layout

layout = resolve_deployment_layout(
    r"G:\_thm\rez_local_cache\ext",
    adapter_package="dcc_mcp_maya",
)

python_paths = layout["environment"]["prepend"]["PYTHONPATH"]
path_entries = layout["environment"]["prepend"]["PATH"]
```

When Rez is active, the helper prefers `REZ_<PACKAGE>_ROOT` variables such as
`REZ_DCC_MCP_CORE_ROOT`, `REZ_DCC_MCP_SERVER_ROOT`, and
`REZ_DCC_MCP_MAYA_ROOT`. Without Rez, pass a shared cache root or explicit
`package_roots` mapping:

```python
layout = resolve_deployment_layout(
    package_roots={
        "dcc_mcp_core": r"G:\_thm\rez_local_cache\ext\dcc_mcp_core",
        "dcc_mcp_server": r"G:\_thm\rez_local_cache\ext\dcc_mcp_server",
        "dcc_mcp_maya": r"G:\_thm\rez_local_cache\ext\dcc_mcp_maya",
    },
    adapter_package="dcc_mcp_maya",
)
```

This keeps development, loose internal drops, and packaged Rez deployments on
one code path.

## Import-Light Sidecar Launch

DCC plugins that run at application startup can build or launch the per-DCC
sidecar without importing `_core` or blocking the host process:

```python
from dcc_mcp_core.install_lifecycle import launch_sidecar

result = launch_sidecar(
    dcc_type="maya",
    host_rpc="commandport://127.0.0.1:6000",
    watch_pid=current_dcc_pid,
    display_name="Maya-Anim",
    adapter_version="1.2.3",
)
```

`launch_sidecar()` uses `subprocess.Popen` with stdin/stdout/stderr detached by
default. The child runs `dcc-mcp-server sidecar`, registers a
`per-dcc-sidecar` row in the shared `FileRegistry`, ensures the machine-wide
gateway daemon unless `no_ensure_gateway=True`, and exits when `watch_pid`
dies. Use `build_sidecar_command()` instead when the adapter wants to hand the
argv list to a studio process supervisor:

Registration is not the same thing as dispatch readiness. The generic sidecar
only becomes callable after its `--host-rpc` URI resolves to a supported
`HostRpcClient`, that client connects to the DCC, and the sidecar publishes
`metadata.dispatch_status=ready` plus a live `metadata.mcp_url` in the registry
row. Startup failures keep the row visible for operators but mark
`metadata.dispatch_status=unavailable` with `failure_stage` / `failure_reason`
metadata. Adapter plugins must still expose a real host RPC bridge to their
DCC dispatcher or skills; `launch_sidecar()` only launches and supervises the
sidecar process. For Maya `commandport://` sidecars, a present
`dcc_mcp_maya` package with a missing `dcc_mcp_maya.sidecar._dispatcher`
returns a structured `sidecar-dispatcher-unavailable` backend envelope on the
first call instead of a generic transport error, so installers can distinguish
partial adapter installs from gateway routing failures.

```python
from dcc_mcp_core.install_lifecycle import build_sidecar_command
from dcc_mcp_core.install_lifecycle import wait_for_sidecar_ready

contract = build_sidecar_command(
    dcc_type="houdini",
    host_rpc="qtserver://127.0.0.1:7001",
    watch_pid=current_dcc_pid,
    registry_dir=r"C:\dcc-mcp\registry",
)
command = contract["command"]
env_updates = contract["environment"]["set"]

ready = wait_for_sidecar_ready(
    dcc_type="houdini",
    host_rpc="qtserver://127.0.0.1:7001",
    timeout_secs=5,
)
```

Use `sidecar_readiness_status()` for a one-shot verdict (`ready`, `missing`,
`booting`, `unavailable`, or `dead`) and `wait_for_sidecar_ready()` from an
installer, supervisor, or background startup task when a short bounded poll is
acceptable. Do not block a DCC UI or main thread waiting for readiness; launch
the sidecar first and surface the verdict through logs or Gateway Admin.

## Import-Light Preflight

```python
from dcc_mcp_core.install_lifecycle import inspect_install_root

diagnostic = inspect_install_root(r"C:\Users\me\Documents\3dsMax\scripts\dcc_mcp_3dsmax")
if diagnostic["requires_restart"]:
    schedule_deferred_cleanup(diagnostic)
```

`inspect_install_root()` checks modules already loaded in the current process.
If a native artifact under the install root is loaded, it returns:

```json
{
  "status": "requires_restart",
  "requires_restart": true,
  "locked_path": "C:\\...\\dcc_mcp_core\\_core.pyd",
  "recommended_next_action": "Defer cleanup until the DCC host restarts, then remove or replace the install root."
}
```

## Registry Query And Sidecar Stop

Installers can inspect the shared FileRegistry without creating any Rust-backed
objects:

```python
from dcc_mcp_core.install_lifecycle import query_runtime_state
from dcc_mcp_core.install_lifecycle import sidecar_readiness_status
from dcc_mcp_core.install_lifecycle import stop_runtime_entries

state = query_runtime_state(dcc_type="3dsmax", role="per-dcc-sidecar")
ready = sidecar_readiness_status(dcc_type="3dsmax")
stop = stop_runtime_entries(dcc_type="3dsmax")
```

For sidecars, each normalized entry exposes `dispatch_status`,
`dispatch_ready`, `host_rpc_uri`, `host_rpc_scheme`, `failure_stage`, and
`failure_reason` at the top level. Startup hooks can poll
`dispatch_ready=True` after `launch_sidecar()` without importing `_core`.
Gateway Admin exposes the same sidecar readiness contract on
`GET /admin/api/workers` as `dispatch_status`, `dispatch_ready`,
`host_rpc_uri`, `host_rpc_scheme`, `failure_stage`, and `failure_reason`, so
operators can distinguish registered-but-not-callable sidecars from routing
failures.

By default, `stop_runtime_entries()` only targets rows that publish
`metadata.sidecar_pid`. It does not terminate the parent DCC process unless
`include_host_processes=True` is passed explicitly.

## Mixed Runtime Version Plan

A gateway can see several DCC runtimes at once. For example, Maya may still be
running an old sidecar while 3ds Max has already started a newer one. Treat each
registered instance independently and plan restarts from registry metadata:

```python
from dcc_mcp_core.install_lifecycle import plan_runtime_updates
from dcc_mcp_core.install_lifecycle import query_runtime_state

state = query_runtime_state()
plan = plan_runtime_updates(
    state,
    target_versions={
        "core": "0.17.21",
        "server": "0.17.21",
        "adapter": "1.2.0",
    },
)
```

`ServiceEntry.version` is the DCC application's version, such as `Maya 2026` or
`Photoshop 25.9`; it is not the `dcc-mcp-core` package version. Runtime rows
must publish package versions through metadata keys such as
`dcc_mcp_core_version`, `dcc_mcp_server_version`, and `adapter_version`.
When package metadata is missing, `plan_runtime_updates()` reports
`action=verify_runtime_metadata` instead of treating the DCC app version as a
package version.

Each plan row reports the component drift and a restart action:

```json
{
  "dcc_type": "maya",
  "action": "restart_sidecar",
  "restart_scope": "sidecar",
  "stale_components": ["core", "server", "adapter"],
  "recommended_next_action": "Stop the registered sidecar, restart it from the target deployment, then re-run MCP readiness."
}
```

Admin surfaces should render `action=restart_sidecar` as a safe sidecar restart
button when `sidecar_pid` is present. If a row reports
`manual_restart_required`, the runtime is host-owned and the DCC process must
be restarted before reset or MCP calls are expected to use the newer code.
If a row reports `verify_runtime_metadata`, the registry row is missing enough
package-version metadata to decide safely; verify or restart that runtime before
assuming it is using the target deployment.
After any stop or restart, verify readiness with the instance MCP endpoint and
refresh gateway registry state before sending reset calls.

The gateway Admin JSON already exposes these operator hints on each instance:

```json
{
  "lifecycle": {
    "role": "per-dcc-sidecar",
    "owner": "release-smoke-test",
    "session": "test",
    "sidecar_pid": 31337,
    "supports_safe_stop": true,
    "safe_stop_url": "http://127.0.0.1:19000/safe-stop",
    "safe_stop_method": "POST",
    "restartable": true,
    "restart_command": "rez-env dcc_mcp_maya -- maya-sidecar"
  }
}
```

Release smoke tests that launch their own DCC process should publish stable,
public lifecycle metadata (`owner`, `session`) and, when supported, a
`safe_stop_url` callback. The gateway and `dcc-mcp-cli stop-instance` only
forward safe-stop requests to that explicit callback and never terminate a
process directly.

## Safe Remove Or Replace

```python
from dcc_mcp_core.install_lifecycle import safe_remove_tree
from dcc_mcp_core.install_lifecycle import safe_replace_tree

removed = safe_remove_tree(install_root)
replaced = safe_replace_tree(staged_payload, install_root)
```

Both helpers attempt immediate cleanup when preflight is clear. If Windows
reports a native-file lock, the result is structured for a deferred startup
hook:

```json
{
  "status": "requires_restart",
  "requires_restart": true,
  "locked_path": "C:\\...\\_core.pyd",
  "reason": "windows_file_lock",
  "deferred_operation": {
    "operation": "remove_tree",
    "path": "C:\\...\\dcc_mcp_3dsmax"
  }
}
```

Run the same helpers from a subprocess when a DCC-specific installer needs a
JSON-only control path:

```bash
python -m dcc_mcp_core.install_lifecycle inspect C:\path\to\adapter
python -m dcc_mcp_core.install_lifecycle stop --dcc-type 3dsmax
python -m dcc_mcp_core.install_lifecycle layout --cache-root G:\_thm\rez_local_cache\ext --adapter-package dcc_mcp_maya
python -m dcc_mcp_core.install_lifecycle sidecar-command --dcc maya --host-rpc commandport://127.0.0.1:6000 --watch-pid 12345
python -m dcc_mcp_core.install_lifecycle launch-sidecar --dcc maya --host-rpc commandport://127.0.0.1:6000 --watch-pid 12345
python -m dcc_mcp_core.install_lifecycle sidecar-ready --dcc maya --timeout-secs 5
python -m dcc_mcp_core.install_lifecycle plan-update --target-version core=0.17.21 --target-version server=0.17.21
python -m dcc_mcp_core.install_lifecycle remove C:\path\to\adapter
```
