"""HostAdapter — base class for per-DCC host adapters.

Downstream DCC integration repos (``dcc-mcp-blender``, ``dcc-mcp-maya``,
``dcc-mcp-photoshop``, ``dcc-mcp-unreal``, …) subclass
:class:`HostAdapter` and implement three small hooks that wire the
dispatcher's tick loop to the DCC's native idle primitive. The base
class owns the full lifecycle (start / stop / run_headless /
context-manager / adaptive interval), so every adapter shares the same
user-visible contract.

Design philosophy
=================

Follows the same *informal duck-typed template* style as
:class:`dcc_mcp_core.adapters.webview.WebViewAdapter` — this is not an
``abc.ABC``. The 3 override points raise :class:`NotImplementedError`
by default so accidentally instantiating a bare :class:`HostAdapter`
fails fast, but subclasses are ordinary Python classes with no
metaclass magic.

SOLID
=====

- **SRP**: the base owns lifecycle + tick orchestration. Subclasses own
  the DCC coupling via 3 hooks.
- **OCP**: new DCCs are new subclasses, zero change to the core.
- **LSP**: every subclass satisfies the same public contract
  (``start`` / ``stop`` / ``run_headless`` / ``is_running`` /
  ``__enter__`` / ``__exit__``), so :class:`HostAdapter`,
  :class:`StandaloneHost`, and any future ``BlenderHost`` /
  ``MayaHost`` are interchangeable in callers.
- **ISP**: 3 override points — :meth:`is_background`,
  :meth:`attach_tick`, :meth:`detach_tick`. No wider surface.
- **DIP**: depends on the :class:`TickableDispatcher` structural
  protocol, never on a concrete ``QueueDispatcher`` /
  ``BlockingDispatcher`` type.

Minimal subclass::

    class BlenderHost(HostAdapter):
        def is_background(self) -> bool:
            import bpy
            return bpy.app.background

        def attach_tick(self, tick_fn):
            import bpy
            bpy.app.timers.register(tick_fn, first_interval=0.0, persistent=True)

        def detach_tick(self):
            import bpy
            if bpy.app.timers.is_registered(self._tick):
                bpy.app.timers.unregister(self._tick)

See ``docs/guide/host-adapter.md`` for the full authoring guide and
``examples/host_adapter_template.py`` for a ready-to-copy starter.
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
import contextlib
import threading
from typing import TYPE_CHECKING
from typing import Callable
from typing import Optional

# Import local modules — re-export the shared protocol so subclass
# authors can type against ``TickableDispatcher`` from a single
# import.
from dcc_mcp_core.host._standalone import _TickableDispatcher as TickableDispatcher

if TYPE_CHECKING:
    # Import local modules
    from dcc_mcp_core._core import BlockingDispatcher
    from dcc_mcp_core._core import QueueDispatcher


__all__ = ["HostAdapter", "TickableDispatcher"]


# Type alias kept short so override signatures read clearly.
# Uses ``typing.Callable`` + ``typing.Optional`` rather than
# ``collections.abc.Callable`` / PEP-604 ``|`` syntax so the module
# imports cleanly on Python 3.7 and 3.8 — the wheel is ABI3-py37 so
# users may run this on any supported interpreter.
TickFn = Callable[[], Optional[float]]


class HostAdapter:
    """Base class for per-DCC host adapters.

    :param dispatcher: a :class:`~dcc_mcp_core.host.QueueDispatcher`,
        :class:`~dcc_mcp_core.host.BlockingDispatcher`, or any object
        satisfying the :class:`TickableDispatcher` protocol.
    :param tick_interval_active: seconds to return from :meth:`_tick`
        when the queue has pending work. ``0.0`` tells ``bpy.app.timers``
        (and kin) to re-fire on the very next frame; most GUI DCCs
        accept this. Default ``0.0``.
    :param tick_interval_idle: seconds to return when the queue is
        drained. Keeps the DCC's idle thread cheap while the MCP
        server is idle. Default ``0.5``.
    :param max_jobs_per_tick: fairness cap passed into each
        ``dispatcher.tick(...)`` call. Default ``16``.
    :param name: debug name (used in the ``run_headless`` thread and
        in error messages).

    Usage
    =====

    Interactive DCC (bpy/Maya/Houdini with a UI running)::

        adapter = BlenderHost(dispatcher)
        adapter.start()              # wires bpy.app.timers
        # ... your DCC runs; mcporter sends tools/call; they execute
        #     on the main thread as the timer fires ...
        adapter.stop()               # unwires cleanly

    Headless DCC (``blender --background`` / ``mayapy`` / ``hython``)::

        adapter = BlenderHost(dispatcher)
        adapter.run_headless()       # blocks; tick_blocking in a loop

    Either mode supports the context-manager form, which picks the
    right path based on :meth:`is_background`.
    """

    def __init__(
        self,
        dispatcher: QueueDispatcher | BlockingDispatcher | TickableDispatcher,
        *,
        tick_interval_active: float = 0.0,
        tick_interval_idle: float = 0.5,
        max_jobs_per_tick: int = 16,
        name: str = "host-adapter",
    ) -> None:
        if tick_interval_active < 0:
            raise ValueError(f"tick_interval_active must be >= 0, got {tick_interval_active!r}")
        if tick_interval_idle <= 0:
            raise ValueError(f"tick_interval_idle must be > 0, got {tick_interval_idle!r}")
        if max_jobs_per_tick <= 0:
            raise ValueError(f"max_jobs_per_tick must be > 0, got {max_jobs_per_tick!r}")
        self._dispatcher = dispatcher
        self._tick_interval_active = float(tick_interval_active)
        self._tick_interval_idle = float(tick_interval_idle)
        self._max_jobs = int(max_jobs_per_tick)
        self._name = name
        self._stop_event = threading.Event()
        self._attached = False
        # Only used by run_headless; None in interactive mode.
        self._headless_thread: threading.Thread | None = None

    # ── Hooks (subclass overrides) ─────────────────────────────────────
    #
    # Subclasses implement these three methods. Everything else is
    # lifecycle orchestration and should be left alone.

    def is_background(self) -> bool:
        """Return ``True`` when the DCC is running headless.

        The base default is the *safe* answer (``True``) — in headless
        mode :meth:`start` redirects to :meth:`run_headless` instead
        of trying to attach a timer that would never fire. Subclasses
        should override to probe the real state, e.g. Blender returns
        ``bpy.app.background``.
        """
        return True

    def attach_tick(self, tick_fn: TickFn) -> None:
        """Wire the DCC's native idle primitive to call ``tick_fn``.

        ``tick_fn`` is a zero-arg callable that returns the next
        interval in seconds (or ``None`` to cancel the timer).
        Implementations should register it with the DCC's timer/idle
        API and return — this method is non-blocking.

        Example for Blender::

            def attach_tick(self, tick_fn):
                import bpy
                bpy.app.timers.register(
                    tick_fn, first_interval=0.0, persistent=True,
                )
        """
        raise NotImplementedError("HostAdapter.attach_tick must be overridden by a DCC-specific subclass.")

    def detach_tick(self) -> None:
        """Undo :meth:`attach_tick`.

        Called by :meth:`stop`. Must be idempotent — ``stop`` may be
        called more than once (context-manager exit + explicit user
        call).
        """
        raise NotImplementedError("HostAdapter.detach_tick must be overridden by a DCC-specific subclass.")

    # ── Lifecycle (do not override) ────────────────────────────────────
    #
    # These orchestrate the hooks. Overriding them would break LSP —
    # callers rely on ``with`` / ``start`` / ``stop`` behaving
    # consistently across every subclass.

    def start(self) -> None:
        """Begin driving the dispatcher on the DCC's main thread.

        In background mode this starts :meth:`run_headless` on a
        daemon thread. In interactive mode it calls
        :meth:`attach_tick` with :meth:`_tick` so the DCC's native
        idle primitive will drain the queue.

        Raises :class:`RuntimeError` if already running.
        """
        if self.is_running:
            raise RuntimeError(f"HostAdapter {self._name!r} is already running")
        self._stop_event.clear()
        if self.is_background():
            t = threading.Thread(
                target=self._run_headless_inner,
                name=f"{self._name}-headless",
                daemon=True,
            )
            t.start()
            self._headless_thread = t
        else:
            self.attach_tick(self._tick)
            self._attached = True

    def stop(self, timeout: float = 5.0) -> None:
        """Stop the tick loop and shut down the dispatcher.

        Idempotent — safe to call multiple times. ``timeout`` is the
        upper bound on how long to wait for a headless thread to
        exit; interactive mode stops immediately (the DCC's next
        frame removes the timer).
        """
        self._stop_event.set()
        with contextlib.suppress(Exception):
            self._dispatcher.shutdown()
        if self._attached:
            with contextlib.suppress(Exception):
                self.detach_tick()
            self._attached = False
        if self._headless_thread is not None:
            self._headless_thread.join(timeout=timeout)
            if self._headless_thread.is_alive():  # pragma: no cover - diagnostic
                raise RuntimeError(f"HostAdapter {self._name!r} headless thread did not stop within {timeout}s")
            self._headless_thread = None

    def run_headless(self, stop_event: threading.Event | None = None) -> None:
        """Drive the dispatcher on the *calling* thread until stopped.

        Intended for headless entry points — e.g. a
        ``blender --background --python bootstrap.py`` script that
        has nothing else to do but hold the process alive while the
        MCP server runs.

        If ``stop_event`` is provided, setting it from another
        thread terminates the loop. Otherwise the loop runs until
        the dispatcher shuts down (e.g. :meth:`stop` is called from
        a signal handler).
        """
        self._stop_event.clear()
        external_stop = stop_event
        dispatcher = self._dispatcher
        max_jobs = self._max_jobs
        use_blocking = hasattr(dispatcher, "tick_blocking")

        # Local stop-flag helper — `Event.wait` returns True if set.
        def _should_stop() -> bool:
            if self._stop_event.is_set():
                return True
            if external_stop is not None and external_stop.is_set():
                return True
            return dispatcher.is_shutdown()

        while not _should_stop():
            if use_blocking:
                # tick_blocking self-paces via its own timeout. Bounded
                # at 50 ms so we notice `stop_event` promptly.
                dispatcher.tick_blocking(max_jobs, 50)  # type: ignore[attr-defined]
            else:
                outcome = dispatcher.tick(max_jobs)
                if not outcome.more_pending:
                    # Respect the configured idle interval. Event.wait
                    # returns early on stop, shortening teardown.
                    self._stop_event.wait(self._tick_interval_idle)

    @property
    def is_running(self) -> bool:
        """``True`` while the host adapter is actively driving the dispatcher."""
        if self._attached:
            return True
        return self._headless_thread is not None and self._headless_thread.is_alive()

    # ── Context manager ────────────────────────────────────────────────

    def __enter__(self) -> HostAdapter:
        self.start()
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        self.stop()

    # ── Internal ───────────────────────────────────────────────────────

    def _tick(self) -> float | None:
        """Tick callback wired by :meth:`attach_tick`.

        Returns the next interval in seconds — ``0`` when there's
        more work pending (re-fire immediately), ``tick_interval_idle``
        otherwise. Returning ``None`` cancels the timer (used by
        :meth:`stop` via the ``_stop_event`` gate).
        """
        if self._stop_event.is_set() or self._dispatcher.is_shutdown():
            return None
        outcome = self._dispatcher.tick(self._max_jobs)
        if outcome.more_pending:
            return self._tick_interval_active
        return self._tick_interval_idle

    def _run_headless_inner(self) -> None:
        """Entry point for the headless driver thread.

        Catches and logs exceptions so a bad dispatcher doesn't leave
        the thread in a half-dead state.
        """
        try:
            self.run_headless()
        except Exception:
            import traceback

            traceback.print_exc()
