# Authoring a DCC Host Adapter

> **Audience**: anyone building a DCC integration repo
> (`dcc-mcp-blender`, `dcc-mcp-maya`, `dcc-mcp-photoshop`,
> `dcc-mcp-unreal`, or a new one).
>
> **TL;DR**: subclass [`dcc_mcp_core.host.HostAdapter`][HostAdapter],
> fill in 3 methods, wire one entry-point. The base class owns the
> rest — lifecycle, context-manager, adaptive tick intervals, and
> the interactive/background split.

This guide assumes you already understand why main-thread affinity
matters — if not, start with [`dcc-thread-safety.md`][thread-safety].

## The 3-hook contract

`HostAdapter` requires exactly three methods on every subclass.

| Hook | Purpose | When called |
|---|---|---|
| `is_background() -> bool` | Is the DCC running headless? | Once per `start()` call |
| `attach_tick(tick_fn)` | Register `tick_fn` with the DCC's native idle primitive | Once, during `start()` in interactive mode |
| `detach_tick()` | Undo `attach_tick` — must be idempotent | During `stop()` |

You do **not** override `start`, `stop`, `run_headless`, `is_running`,
`__enter__`, or `__exit__`. Those orchestrate the 3 hooks and must stay
consistent across every adapter so callers can treat them
interchangeably (LSP).

## Minimal subclass

```python
from dcc_mcp_core.host import HostAdapter


class BlenderHost(HostAdapter):
    def is_background(self) -> bool:
        import bpy
        return bpy.app.background

    def attach_tick(self, tick_fn):
        import bpy
        # Returning ``tick_fn`` reuses the same callable every time the
        # timer fires, so `detach_tick` can find and unregister it.
        bpy.app.timers.register(tick_fn, first_interval=0.0, persistent=True)
        self._tick_fn = tick_fn

    def detach_tick(self) -> None:
        import bpy
        fn = getattr(self, "_tick_fn", None)
        if fn is not None and bpy.app.timers.is_registered(fn):
            bpy.app.timers.unregister(fn)
        self._tick_fn = None
```

Done. That's the whole adapter. Everything else — panic handling,
dispatcher shutdown on stop, the "wait up to 5s for the headless
thread to join" safeguard, the adaptive interval that returns 0s
when the queue is hot and 0.5s when it's idle — is in the base.

## Wiring it into an MCP server

The adapter **drives** the dispatcher; it doesn't own it. Your entry
point owns both:

```python
from dcc_mcp_core import McpHttpConfig, McpHttpServer, ToolRegistry
from dcc_mcp_core.host import BlockingDispatcher

# 1. Build the server.
reg = ToolRegistry()
cfg = McpHttpConfig(port=18765, server_name="blender")
server = McpHttpServer(reg, cfg)

# 2. Create a dispatcher. BlockingDispatcher is right for --background
#    DCCs; QueueDispatcher is right for GUI sessions. Either one is
#    accepted by HostAdapter, McpHttpServer.attach_dispatcher, and
#    StandaloneHost (LSP in practice).
dispatcher = BlockingDispatcher()
server.attach_dispatcher(dispatcher)

# 3. Start the server. This returns immediately — it only binds the
#    port and spawns the tokio runtime.
handle = server.start()

# 4. Drive the dispatcher with your adapter.
host = BlenderHost(dispatcher)
if host.is_background():
    host.run_headless()   # blocks until shutdown
else:
    host.start()          # non-blocking; returns immediately
```

Every `tools/call` that arrives on the HTTP port will now be posted
into the dispatcher and executed on whatever thread drives
`host._tick` — i.e. the DCC main thread in interactive mode, or the
`run_headless` thread in headless mode. Handlers never see a tokio
worker thread.

## Maya example

```python
class MayaHost(HostAdapter):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self._script_job = None

    def is_background(self) -> bool:
        import maya.cmds as cmds
        return cmds.about(batch=True)

    def attach_tick(self, tick_fn):
        import maya.cmds as cmds
        # `idleEvent` fires on the UI idle tick — native main-thread.
        # Wrap in a lambda so `tick_fn`'s return value is discarded
        # (scriptJob doesn't care about the next interval).
        self._script_job = cmds.scriptJob(
            idleEvent=lambda: tick_fn(),
        )

    def detach_tick(self) -> None:
        import maya.cmds as cmds
        if self._script_job is not None and cmds.scriptJob(
            exists=self._script_job,
        ):
            cmds.scriptJob(kill=self._script_job)
        self._script_job = None
```

Maya's `idleEvent` fires more aggressively than Blender's timer, so
the default `tick_interval_idle=0.5` is conservative enough. If you
find the CPU usage too high, bump `tick_interval_idle` to `1.0`.

## Headless-only DCCs (ExtendScript, MaxScript)

When the DCC has no Python-callable idle primitive (Adobe Photoshop's
ExtendScript, 3ds Max pre-2022's MAXScript bridge, …), run the whole
thing headlessly:

```python
class PhotoshopHost(HostAdapter):
    def is_background(self) -> bool:
        return True  # always headless — no ExtendScript UI idle hook

    def attach_tick(self, tick_fn):
        # Never called (is_background is always True).
        raise NotImplementedError(
            "PhotoshopHost is always headless; run_headless is the only path",
        )

    def detach_tick(self) -> None:
        pass  # no-op; nothing was attached
```

Your entry point then calls `host.run_headless()` unconditionally.

## Substitutability test

Every well-behaved subclass should pass the same contract test, which
is essentially what `tests/test_host_adapter.py::test_subclass_overriding_hooks_drives_dispatcher`
already exercises on a fake subclass. Copy it into your repo, swap in
your real subclass, and you have a CI gate:

```python
def test_my_host_drives_dispatcher(live_dcc_fixture):
    dispatcher = QueueDispatcher()
    host = MyDccHost(dispatcher)
    with host:
        result = dispatcher.post(lambda: 42).wait(timeout=5.0)
    assert result == 42
```

## Checklist when opening a DCC-integration repo

- [ ] Subclass `HostAdapter`, implement the 3 hooks.
- [ ] Ship at least one example skill (a single tool is enough) that
  proves `bpy.ops` / `maya.cmds` / equivalent works on the main thread.
- [ ] Add a CI job that starts the DCC headless, runs an `mcporter`
  call against the live server, and asserts success.
- [ ] Write a `README.md` pointing back at this doc so future
  maintainers understand the contract.
- [ ] Open a tracking issue in your repo; cross-reference the core's
  [umbrella issue][umbrella] so progress is visible across repos.

[HostAdapter]: https://github.com/loonghao/dcc-mcp-core/blob/main/python/dcc_mcp_core/host/_adapter.py
[thread-safety]: ./dcc-thread-safety.md
[umbrella]: https://github.com/loonghao/dcc-mcp-core/issues/690
