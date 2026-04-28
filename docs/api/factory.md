# Server Factory API

Singleton factory helpers for DCC MCP server instances. Eliminates the boilerplate threading lock + `None` check that every adapter would otherwise duplicate.

**Exported symbols:** `create_dcc_server`, `get_server_instance`, `make_start_stop`

## create_dcc_server

```python
create_dcc_server(
    *, instance_holder: list, lock: threading.Lock, server_class: type,
    port: int = 8765, register_builtins: bool = True,
    extra_skill_paths: list[str] | None = None,
    include_bundled: bool = True, enable_hot_reload: bool = False,
    hot_reload_env_var: str | None = None, **server_kwargs
) -> McpServerHandle
```

Create-or-return a singleton DCC MCP server and start it. Thread-safe.

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `instance_holder` | `list` | (required) | Single-element list used as mutable reference |
| `lock` | `threading.Lock` | (required) | Module-level lock for thread safety |
| `server_class` | `type` | (required) | DccServerBase subclass to instantiate |
| `port` | `int` | `8765` | TCP port for the MCP HTTP server |
| `register_builtins` | `bool` | `True` | Call `register_builtin_actions()` after creation |
| `extra_skill_paths` | `list[str] \| None` | `None` | Additional skill directories |
| `include_bundled` | `bool` | `True` | Include dcc-mcp-core bundled skills |
| `enable_hot_reload` | `bool` | `False` | Enable skill hot-reload |
| `hot_reload_env_var` | `str \| None` | `None` | Env var for hot-reload override |

## make_start_stop

```python
make_start_stop(server_class: type, hot_reload_env_var: str | None = None) -> tuple[Callable, Callable]
```

Generate a `(start_server, stop_server)` function pair for a DCC adapter. Zero-boilerplate.

```python
from dcc_mcp_core import make_start_stop

start_server, stop_server = make_start_stop(
    MyDccServer,
    hot_reload_env_var="DCC_MCP_MYDCC_HOT_RELOAD",
)
```

## get_server_instance

```python
get_server_instance(instance_holder: list) -> server | None
```

Return the current singleton instance (or `None` if not started).

---

## Embedded-host wiring (issues #521, #525)

Embedded DCC plugins (Maya, Houdini, Unreal Python, Blender) typically need
two pieces of glue beyond the bare server:

1. A **callable dispatcher** that routes skill scripts onto the host's
   UI / main thread.
2. A **declarative skill list** so launch-on-startup is reproducible across
   sessions.

`DccServerBase` exposes `register_inprocess_executor()` and
`register_builtin_actions(minimal_mode=...)` for exactly this:

```python
from dcc_mcp_core import (
    DccServerBase, McpHttpConfig,
    InProcessCallableDispatcher, build_inprocess_executor,
    MinimalModeConfig,
)

class MayaDccServer(DccServerBase):
    @classmethod
    def dcc_name(cls) -> str: return "maya"

server = MayaDccServer(McpHttpConfig(port=8765))

# 1) Wire the in-process executor BEFORE registering builtins.
dispatcher = InProcessCallableDispatcher()    # or your Maya UI-thread subclass
server.register_inprocess_executor(build_inprocess_executor(dispatcher))

# 2) Pin the boot-time skill set declaratively.
server.register_builtin_actions(minimal_mode=MinimalModeConfig(
    skills=("scene_inspector", "render_queue"),
    deactivate_groups={"render_queue": ("submit",)},
    env_var_minimal="DCC_MCP_MAYA_MINIMAL",
))

server.start()
```

See [Callable Dispatcher API](./dispatcher.md) for the full
`BaseDccCallableDispatcher` / `BaseDccCallableDispatcherFull` / `BaseDccPump`
contract and the `MinimalModeConfig` resolution order.
