"""Reusable host pump controller and timer adapters for UI DCC loops.

The dispatcher owns queued jobs; the timer adapter owns the host-specific
primitive (Maya script jobs, Blender timers, Qt ``QTimer``, or a test fake).
``HostPumpController`` is the small composition layer between them.
"""

from __future__ import annotations

from dataclasses import dataclass
import importlib
import sys
import threading
import time
from typing import Any
from typing import Callable

from dcc_mcp_core._server.callable_dispatcher import AdaptivePumpPolicy
from dcc_mcp_core._server.callable_dispatcher import DrainStats

if sys.version_info >= (3, 8):
    from typing import Protocol
    from typing import runtime_checkable
else:  # pragma: no cover - py3.7 only

    def runtime_checkable(cls):
        return cls

    class Protocol:  # type: ignore[no-redef]
        pass


__all__ = [
    "HostPumpController",
    "HostPumpSnapshot",
    "HostPumpTimerAdapter",
    "ManualHostTimerAdapter",
    "QtHostTimerAdapter",
    "ThreadedHostTimerAdapter",
]


HostTick = Callable[[], float | None]


@runtime_checkable
class HostPumpTimerAdapter(Protocol):
    """Host-specific timer primitive used by :class:`HostPumpController`."""

    def install(self, tick: HostTick) -> None:
        """Install *tick* as the host timer callback."""
        ...

    def uninstall(self) -> None:
        """Remove the host timer callback."""
        ...

    def schedule_soon(self) -> None:
        """Ask the host to call the tick callback as soon as possible."""
        ...


@dataclass
class HostPumpSnapshot:
    """Common pump/controller counters exposed to adapters and tests."""

    queue_size: int = 0
    active_jobs: int = 0
    interval_secs: float = 0.0
    overrun_count: int = 0
    last_tick_time: float = 0.0
    shutdown: bool = False
    ticks: int = 0
    drained_jobs: int = 0
    last_elapsed_ms: float = 0.0


@dataclass
class _DrainOutcome:
    drained: int
    queue_size: int | None
    elapsed_ms: float
    overrun: bool


class HostPumpController:
    """Drive a DCC dispatcher/pump from a host-specific timer adapter."""

    def __init__(
        self,
        pump: Any,
        timer_adapter: HostPumpTimerAdapter,
        *,
        policy: AdaptivePumpPolicy | None = None,
        budget_ms: int = 8,
        deferred_pending_provider: Callable[[], bool] | None = None,
        clock: Callable[[], float] = time.monotonic,
        shutdown_pump_on_stop: bool = False,
    ) -> None:
        if budget_ms <= 0:
            raise ValueError("budget_ms must be > 0")
        self.pump = pump
        self.timer_adapter = timer_adapter
        self.policy = policy or AdaptivePumpPolicy(clock=clock)
        self.budget_ms = int(budget_ms)
        self.deferred_pending_provider = deferred_pending_provider
        self.clock = clock
        self.shutdown_pump_on_stop = shutdown_pump_on_stop
        self._installed = False
        self._shutdown = False
        self._stats = HostPumpSnapshot(interval_secs=self.policy.stats.last_interval_secs)

    @property
    def stats(self) -> HostPumpSnapshot:
        """Return the latest controller snapshot."""
        return self._stats

    @property
    def is_running(self) -> bool:
        """Return whether the timer is currently installed."""
        return self._installed and not self._shutdown

    def start(self) -> None:
        """Install the timer callback once and schedule an immediate tick."""
        if self._installed:
            return
        self._shutdown = False
        self.timer_adapter.install(self.tick)
        self._installed = True
        self.schedule_soon()

    def stop(self) -> None:
        """Uninstall the timer callback and optionally shut down the pump."""
        if self._shutdown and not self._installed:
            return
        self._shutdown = True
        if self._installed:
            self.timer_adapter.uninstall()
            self._installed = False
        if self.shutdown_pump_on_stop:
            shutdown = getattr(self.pump, "shutdown", None)
            if callable(shutdown):
                shutdown()
        self._refresh_snapshot(interval_secs=0.0, elapsed_ms=0.0)

    def schedule_soon(self) -> bool:
        """Schedule the next tick immediately when running."""
        if self._shutdown or not self._installed:
            return False
        self.policy.mark_client_activity()
        self.timer_adapter.schedule_soon()
        return True

    def tick(self) -> float | None:
        """Drain the pump once and return the next timer interval."""
        if self._shutdown or _is_shutdown(self.pump):
            self._shutdown = True
            self._refresh_snapshot(interval_secs=0.0, elapsed_ms=0.0)
            return None

        start = self.clock()
        raw = self.pump.drain_queue(self.budget_ms)
        elapsed_ms = max((self.clock() - start) * 1000.0, 0.0)
        outcome = _normalize_drain_outcome(raw, elapsed_ms=elapsed_ms, budget_ms=self.budget_ms)
        queue_size = outcome.queue_size if outcome.queue_size is not None else _queue_size(self.pump)
        overrun = outcome.overrun or outcome.elapsed_ms > self.budget_ms

        self.policy.record_tick(
            drained=outcome.drained,
            elapsed_ms=outcome.elapsed_ms,
            overrun=overrun,
        )
        interval = self.policy.next_interval(
            has_pending=queue_size > 0,
            deferred_pending=self._deferred_pending(),
        )
        self._refresh_snapshot(
            interval_secs=interval,
            elapsed_ms=outcome.elapsed_ms,
            queue_size=queue_size,
            active_jobs=_active_jobs(self.pump),
        )
        return None if self._shutdown else interval

    def _deferred_pending(self) -> bool:
        provider = self.deferred_pending_provider
        if provider is None:
            return False
        return bool(provider())

    def _refresh_snapshot(
        self,
        *,
        interval_secs: float,
        elapsed_ms: float,
        queue_size: int | None = None,
        active_jobs: int | None = None,
    ) -> None:
        policy_stats = self.policy.stats
        self._stats = HostPumpSnapshot(
            queue_size=_queue_size(self.pump) if queue_size is None else queue_size,
            active_jobs=_active_jobs(self.pump) if active_jobs is None else active_jobs,
            interval_secs=interval_secs,
            overrun_count=policy_stats.overrun_cycles,
            last_tick_time=self.clock(),
            shutdown=self._shutdown or _is_shutdown(self.pump),
            ticks=policy_stats.ticks,
            drained_jobs=policy_stats.drained_jobs,
            last_elapsed_ms=elapsed_ms,
        )


class ManualHostTimerAdapter:
    """Manual timer adapter for deterministic tests and adapter smoke checks."""

    def __init__(self) -> None:
        self.tick: HostTick | None = None
        self.installed = False
        self.install_count = 0
        self.uninstall_count = 0
        self.scheduled_count = 0
        self.last_interval_secs: float | None = None

    def install(self, tick: HostTick) -> None:
        if self.installed:
            self.tick = tick
            return
        self.tick = tick
        self.installed = True
        self.install_count += 1

    def uninstall(self) -> None:
        if not self.installed:
            return
        self.installed = False
        self.tick = None
        self.uninstall_count += 1

    def schedule_soon(self) -> None:
        if self.installed:
            self.scheduled_count += 1

    def fire(self) -> float | None:
        if not self.installed or self.tick is None:
            raise RuntimeError("ManualHostTimerAdapter is not installed")
        self.last_interval_secs = self.tick()
        return self.last_interval_secs


class ThreadedHostTimerAdapter:
    """Stdlib timer adapter for standalone/headless integration tests."""

    def __init__(self) -> None:
        self._tick: HostTick | None = None
        self._timer: threading.Timer | None = None
        self._lock = threading.Lock()
        self.installed = False

    def install(self, tick: HostTick) -> None:
        with self._lock:
            self._tick = tick
            self.installed = True

    def uninstall(self) -> None:
        with self._lock:
            self.installed = False
            self._tick = None
            if self._timer is not None:
                self._timer.cancel()
                self._timer = None

    def schedule_soon(self) -> None:
        self._schedule(0.0)

    def _schedule(self, interval_secs: float) -> None:
        with self._lock:
            if not self.installed:
                return
            if self._timer is not None:
                self._timer.cancel()
            timer = threading.Timer(max(interval_secs, 0.0), self._fire)
            timer.daemon = True
            self._timer = timer
            timer.start()

    def _fire(self) -> None:
        with self._lock:
            tick = self._tick if self.installed else None
        if tick is None:
            return
        interval = tick()
        if interval is not None:
            self._schedule(interval)


class QtHostTimerAdapter:
    """Generic Qt ``QTimer`` adapter for Qt-bearing DCC hosts."""

    def __init__(self, qt_core: Any | None = None) -> None:
        self.qt_core = qt_core
        self.timer: Any | None = None
        self.tick: HostTick | None = None
        self.installed = False

    def install(self, tick: HostTick) -> None:
        if self.installed:
            self.tick = tick
            return
        qt_core = self.qt_core or _import_qt_core()
        timer = qt_core.QTimer()
        if callable(getattr(timer, "setSingleShot", None)):
            timer.setSingleShot(True)
        timer.timeout.connect(self._fire)
        self.qt_core = qt_core
        self.timer = timer
        self.tick = tick
        self.installed = True

    def uninstall(self) -> None:
        if self.timer is not None and callable(getattr(self.timer, "stop", None)):
            self.timer.stop()
        self.installed = False
        self.tick = None
        self.timer = None

    def schedule_soon(self) -> None:
        self._start(0.0)

    def _fire(self) -> None:
        tick = self.tick if self.installed else None
        if tick is None:
            return
        interval = tick()
        if interval is not None:
            self._start(interval)

    def _start(self, interval_secs: float) -> None:
        if not self.installed or self.timer is None:
            return
        self.timer.start(max(int(interval_secs * 1000), 0))


def _normalize_drain_outcome(raw: Any, *, elapsed_ms: float, budget_ms: int) -> _DrainOutcome:
    if isinstance(raw, DrainStats):
        return _DrainOutcome(
            drained=max(int(raw.drained), 0),
            queue_size=None,
            elapsed_ms=float(raw.elapsed_ms or elapsed_ms),
            overrun=bool(raw.overrun),
        )
    if isinstance(raw, tuple) and len(raw) >= 2:
        return _DrainOutcome(
            drained=max(int(raw[0]), 0),
            queue_size=max(int(raw[1]), 0),
            elapsed_ms=elapsed_ms,
            overrun=elapsed_ms > budget_ms,
        )
    if isinstance(raw, dict):
        queue_size = raw.get("queue_size", raw.get("remaining"))
        return _DrainOutcome(
            drained=max(int(raw.get("drained", raw.get("executed", 0))), 0),
            queue_size=None if queue_size is None else max(int(queue_size), 0),
            elapsed_ms=float(raw.get("elapsed_ms", elapsed_ms)),
            overrun=bool(raw.get("overrun", False)),
        )
    if isinstance(raw, int):
        return _DrainOutcome(
            drained=max(raw, 0),
            queue_size=None,
            elapsed_ms=elapsed_ms,
            overrun=elapsed_ms > budget_ms,
        )
    return _DrainOutcome(drained=0, queue_size=None, elapsed_ms=elapsed_ms, overrun=elapsed_ms > budget_ms)


def _queue_size(pump: Any) -> int:
    for name in ("queue_size", "pending_count"):
        attr = getattr(pump, name, None)
        if callable(attr):
            return max(int(attr()), 0)
    pending = getattr(pump, "pending_count", None)
    if isinstance(pending, int):
        return max(pending, 0)
    return 0


def _active_jobs(pump: Any) -> int:
    attr = getattr(pump, "active_count", None)
    if callable(attr):
        return max(int(attr()), 0)
    active = getattr(pump, "active_jobs", None)
    if isinstance(active, int):
        return max(active, 0)
    return 0


def _is_shutdown(pump: Any) -> bool:
    attr = getattr(pump, "is_shutdown", None)
    if callable(attr):
        return bool(attr())
    if attr is not None:
        return bool(attr)
    return False


def _import_qt_core() -> Any:
    for module_name in ("PySide6.QtCore", "PyQt6.QtCore", "PySide2.QtCore", "PyQt5.QtCore"):
        try:
            return importlib.import_module(module_name)
        except ImportError:
            continue
    raise RuntimeError("No supported Qt binding is available for QtHostTimerAdapter")
