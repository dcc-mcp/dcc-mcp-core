"""Starter template for a DCC-specific HostAdapter subclass.

Copy this file into your DCC repo (``dcc-mcp-blender``, ``dcc-mcp-maya``,
``dcc-mcp-photoshop``, …) and fill in the three hook methods. The base
class handles lifecycle, context-manager, adaptive tick intervals, and
the interactive / background-mode split for you.

Authoring contract
==================

You must implement exactly three methods:

1. ``is_background()`` — return ``True`` when the DCC is running
   headless (no idle callback will fire). In GUI mode the base class
   attaches your tick to the DCC's native timer; in background mode it
   runs ``run_headless`` on a daemon thread instead.
2. ``attach_tick(tick_fn)`` — register ``tick_fn`` with the DCC's
   native idle primitive. ``tick_fn`` is a zero-arg callable that
   returns the next interval in seconds (or ``None`` to cancel).
3. ``detach_tick()`` — undo ``attach_tick``. Must be idempotent.

Do NOT override ``start`` / ``stop`` / ``run_headless`` /
``__enter__`` / ``__exit__`` / ``is_running``. They orchestrate the
three hooks and must stay consistent across every adapter so callers
can treat adapters interchangeably (LSP).
"""

from __future__ import annotations

from dcc_mcp_core.host import HostAdapter
from dcc_mcp_core.host import TickableDispatcher  # noqa: F401 — import for type hints


class YourDccHost(HostAdapter):
    """Replace the body of each hook with your DCC's primitives."""

    # ── Hook 1: tell the base class whether we have a UI loop ─────────
    def is_background(self) -> bool:
        # Example: return bpy.app.background        # Blender
        # Example: return not maya.cmds.about(batch=False)  # Maya
        raise NotImplementedError

    # ── Hook 2: wire the DCC's idle primitive ─────────────────────────
    def attach_tick(self, tick_fn):
        # Example (Blender):
        #   import bpy
        #   bpy.app.timers.register(tick_fn, first_interval=0.0, persistent=True)
        #
        # Example (Maya):
        #   import maya.utils
        #   self._script_job = maya.cmds.scriptJob(
        #       idleEvent=lambda: tick_fn(),
        #   )
        raise NotImplementedError

    # ── Hook 3: undo attach_tick (must be idempotent) ─────────────────
    def detach_tick(self) -> None:
        # Example (Blender):
        #   import bpy
        #   if bpy.app.timers.is_registered(self._tick):
        #       bpy.app.timers.unregister(self._tick)
        #
        # Example (Maya):
        #   import maya.cmds as cmds
        #   if self._script_job is not None and cmds.scriptJob(exists=self._script_job):
        #       cmds.scriptJob(kill=self._script_job)
        #   self._script_job = None
        raise NotImplementedError


# ── Typical entry point inside your DCC ──────────────────────────────
#
# from dcc_mcp_core import McpHttpConfig, McpHttpServer, ToolRegistry
# from dcc_mcp_core.host import BlockingDispatcher
#
# reg = ToolRegistry()
# cfg = McpHttpConfig(port=18765, server_name="your-dcc")
# server = McpHttpServer(reg, cfg)
# dispatcher = BlockingDispatcher()
# server.attach_dispatcher(dispatcher)
# handle = server.start()
#
# host = YourDccHost(dispatcher)
# if host.is_background():
#     host.run_headless()
# else:
#     host.start()
