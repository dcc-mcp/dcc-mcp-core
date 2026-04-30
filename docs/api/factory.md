# Server Factory API

Singleton factory helpers for DCC MCP server instances. Eliminates the boilerplate threading lock + `None` check that every adapter would otherwise duplicate.

**Exported symbols:** `create_dcc_server`, `start_embedded_dcc_server`,
`get_server_instance`, `make_start_stop`

## create_dcc_server

```python
create_dcc_server(
    *, instance_holder: list, lock: threading.Lock, server_class: type,
    port: int = 8765, dispatcher=None, dispatcher_factory=None,
    register_builtins: bool = True,
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
| `dispatcher` | `Any \| None` | `None` | Pre-created host dispatcher forwarded to the server constructor before skill discovery |
| `dispatcher_factory` | `Callable[[], Any \| None] \| None` | `None` | Lazily creates a dispatcher only when a new singleton is constructed |
| `register_builtins` | `bool` | `True` | Call `register_builtin_actions()` after creation |
| `extra_skill_paths` | `list[str] \| None` | `None` | Additional skill directories |
| `include_bundled` | `bool` | `True` | Include dcc-mcp-core bundled skills |
| `enable_hot_reload` | `bool` | `False` | Enable skill hot-reload |
| `hot_reload_env_var` | `str \| None` | `None` | Env var for hot-reload override |

## start_embedded_dcc_server

```python
start_embedded_dcc_server(
    *,
    dcc_name: str,
    instance_holder: list,
    lock: threading.Lock,
    server_class: type,
    dispatcher_factory: Callable[[], Any | None] | None = None,
    dispatcher: Any | None = None,
    env_prefix: str | None = None,
    ...
) -> McpServerHandle
```

Adapter bootstrap helper that fixes the safe order for embedded hosts:

1. Create or receive the host dispatcher.
2. Construct the `DccServerBase` subclass with that dispatcher.
3. Discover/load skills.
4. Start the HTTP server and gateway registration.

Use this when a DCC plugin has to build its dispatcher before any skill can be
loaded:

```python
from dcc_mcp_core import start_embedded_dcc_server

_holder = [None]
_lock = threading.Lock()

def start_server(port=8765):
    return start_embedded_dcc_server(
        dcc_name="blender",
        instance_holder=_holder,
        lock=_lock,
        server_class=BlenderMcpServer,
        dispatcher_factory=create_blender_dispatcher,
        env_prefix="DCC_MCP_BLENDER",
        port=port,
    )
```

## make_start_stop

```python
make_start_stop(
    server_class: type,
    hot_reload_env_var: str | None = None,
    dispatcher_factory: Callable[[], Any | None] | None = None,
) -> tuple[Callable, Callable]
```

Generate a `(start_server, stop_server)` function pair for a DCC adapter. Zero-boilerplate.

```python
from dcc_mcp_core import make_start_stop

start_server, stop_server = make_start_stop(
    MyDccServer,
    hot_reload_env_var="DCC_MCP_MYDCC_HOT_RELOAD",
    dispatcher_factory=create_my_dcc_dispatcher,
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
from dcc_mcp_core import DccServerBase, InProcessCallableDispatcher, MinimalModeConfig

class MayaDccServer(DccServerBase):
    @classmethod
    def dcc_name(cls) -> str: return "maya"

# 1) Pass the dispatcher into the constructor BEFORE registering builtins.
dispatcher = InProcessCallableDispatcher()    # or your Maya UI-thread subclass
server = MayaDccServer(port=8765, dispatcher=dispatcher)

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
