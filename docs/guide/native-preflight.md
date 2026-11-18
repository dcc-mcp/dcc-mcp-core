# Native extension preflight for embedded Python hosts

`import dcc_mcp_core` is intentionally light. It verifies package discovery,
not whether the native extension or a DCC server can run. In the exact host
interpreter, before importing native-backed server classes or constructing
schemas, run:

```python
from dcc_mcp_core import verify_native_core

report = verify_native_core()
if not report["success"]:
    raise RuntimeError(report)
```

This explicit probe imports `_core` without starting a gateway, importing a
host API, or creating a server. `native_core_available` proves native loading
only; host readiness and main-thread HTTP tool execution still require their
own checks. `native_extension_missing` distinguishes a lite/missing wheel from
`native_load_failed` (a present extension with unresolved dependencies or an
incompatible runtime). Repeating the probe retries the import after repair.
Diagnostics contain local interpreter/search paths; redact them before sharing.

On Windows with Python 3.8+, Core retains DLL-directory handles for existing absolute
`sys.prefix`, `sys.base_prefix`, and interpreter directories. A deployment can
also set `DCC_MCP_PYTHON3_DLL_DIR` to an existing absolute directory containing
a trusted Python runtime compatible with the host's Python version and
architecture. DLL search preparation runs when `dcc_mcp_core` is imported,
before lazy native access while `_core` is not loaded, and on every
explicit `verify_native_core()` call. Setting the variable after the package
import therefore takes effect when the probe is called again.
Relative or inaccessible candidates are skipped so other interpreter directories
can still be registered; PATH and the working directory are never searched.
Python 3.8+ extension dependency loading does not use PATH alone.

Registered directory handles are retained until process exit. Changing or
unsetting the variable and repeating the probe adds any new candidate but does
not remove an earlier registered directory or unload a native module or runtime
DLL. Restart the host with the intended environment to replace or isolate a
previously registered runtime directory.

Windows Python 3.7 has no `os.add_dll_directory`, so this helper cannot register
DLL search directories there. Setting the override and repeating the probe does
not repair a missing dependency on that interpreter. Use the matching
[`native_py37` wheel](py37-lite-architecture.md#native_py37) and provision the
host-compatible Python/runtime DLLs through the deployment's existing setup
before launching the host; the probe then verifies native loading. The
Python 3.8+ directory-registration behavior is not a Python 3.7 fallback.

For example, MotionBuilder 2024 may ship `python310.dll` without the
`python3.dll` required by an ABI3 wheel. Adding a directory cannot create that
missing DLL. Inspect `_core.pyd` dependencies and supply a compatible vendor or
CPython runtime through the deployment's existing provisioning process. Do not
rename `python310.dll` or copy a DLL from another Python version. A generic
Windows loader failure can also concern other runtime dependencies; Core does
not claim to identify the exact missing DLL from that message.

Native-backed lazy exports and the JSON codec preserve actionable failures and
the original exception cause. Only an actually absent `_core` module retains
the existing lite JSON fallback; a broken extension never silently downgrades.

Adapter maintainers should add this probe to installation verification and
startup before loading server modules. Public agent Skill packages now live in
`dcc-mcp-agent-plugins`; document this API there as pending until the Core
version containing it is released. Do not advertise it for older wheels.

Core's `create_dcc_server()`, `start_embedded_dcc_server()`, and the
`start_server()` returned by `make_start_stop()` run the probe before creating
a dispatcher, constructing a server, or building schemas. A present but
unloadable extension raises an actionable `ImportError` at that boundary.
An actually absent `_core` remains valid for the lite/sidecar deployment, so
those helpers keep their existing extensionless startup behavior.
