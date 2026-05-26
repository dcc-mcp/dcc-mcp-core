"""Tests for reusable host pump controllers and timer adapters."""

from __future__ import annotations

import threading
from typing import Any

import dcc_mcp_core
from dcc_mcp_core._server.callable_dispatcher import AdaptivePumpPolicy
from dcc_mcp_core._server.callable_dispatcher import DrainStats
from dcc_mcp_core._server.host_pump import HostPumpController
from dcc_mcp_core._server.host_pump import HostPumpSnapshot
from dcc_mcp_core._server.host_pump import HostPumpTimerAdapter
from dcc_mcp_core._server.host_pump import ManualHostTimerAdapter
from dcc_mcp_core._server.host_pump import QtHostTimerAdapter
from dcc_mcp_core._server.host_pump import ThreadedHostTimerAdapter
from dcc_mcp_core._server.host_ui_dispatcher import HostUiDispatcherBase


class _Clock:
    def __init__(self) -> None:
        self.now = 0.0

    def __call__(self) -> float:
        return self.now

    def advance(self, seconds: float) -> None:
        self.now += seconds


class _ManualUiDispatcher(HostUiDispatcherBase):
    def __init__(self) -> None:
        super().__init__()
        self.pokes = 0

    def poke_host_pump(self) -> None:
        self.pokes += 1


def test_host_pump_controller_exported() -> None:
    assert dcc_mcp_core.HostPumpController is HostPumpController
    assert dcc_mcp_core.HostPumpSnapshot is HostPumpSnapshot
    assert dcc_mcp_core.ManualHostTimerAdapter is ManualHostTimerAdapter
    assert dcc_mcp_core.QtHostTimerAdapter is QtHostTimerAdapter
    assert dcc_mcp_core.ThreadedHostTimerAdapter is ThreadedHostTimerAdapter
    assert "HostPumpController" in dcc_mcp_core.__all__
    assert "ManualHostTimerAdapter" in dcc_mcp_core.__all__


def test_timer_protocol_is_runtime_checkable() -> None:
    assert isinstance(ManualHostTimerAdapter(), HostPumpTimerAdapter)


def test_controller_start_stop_are_idempotent() -> None:
    pump = _ManualUiDispatcher()
    timer = ManualHostTimerAdapter()
    controller = HostPumpController(pump, timer)

    controller.start()
    controller.start()
    assert controller.is_running is True
    assert timer.install_count == 1
    assert timer.scheduled_count == 1

    controller.stop()
    controller.stop()
    assert controller.is_running is False
    assert timer.uninstall_count == 1
    assert controller.stats.shutdown is True


def test_controller_drains_host_ui_dispatcher_with_manual_timer() -> None:
    pump = _ManualUiDispatcher()
    timer = ManualHostTimerAdapter()
    clock = _Clock()
    policy = AdaptivePumpPolicy(active_interval_secs=0.01, idle_interval_secs=0.5, clock=clock)
    controller = HostPumpController(pump, timer, policy=policy, budget_ms=8, clock=clock)
    done = threading.Event()
    outcomes: list[dict[str, Any]] = []

    pump.submit_async_callable(
        "req-1",
        lambda: "ok",
        affinity="main",
        job_id="job-1",
        on_complete=lambda result: (outcomes.append(result), done.set()),
    )
    assert pump.queue_size() == 1
    assert pump.active_count() == 0

    controller.start()
    interval = timer.fire()

    assert interval == 0.01
    assert done.wait(timeout=1.0)
    assert outcomes[0]["success"] is True
    assert outcomes[0]["output"] == "ok"
    assert pump.queue_size() == 0
    assert controller.stats.queue_size == 0
    assert controller.stats.active_jobs == 0
    assert controller.stats.ticks == 1
    assert controller.stats.drained_jobs == 1
    assert controller.stats.interval_secs == 0.01


def test_schedule_soon_tracks_client_activity_and_running_state() -> None:
    pump = _ManualUiDispatcher()
    timer = ManualHostTimerAdapter()
    controller = HostPumpController(pump, timer)

    assert controller.schedule_soon() is False
    controller.start()
    assert controller.schedule_soon() is True
    assert timer.scheduled_count == 2
    controller.stop()
    assert controller.schedule_soon() is False


def test_budget_overrun_and_queue_stats_are_exposed() -> None:
    class _BudgetPump:
        def __init__(self) -> None:
            self.budgets: list[int] = []
            self.queue = 2
            self.active = 1

        def drain_queue(self, budget_ms: int) -> DrainStats:
            self.budgets.append(budget_ms)
            self.queue = 1
            return DrainStats(drained=2, elapsed_ms=12.0, overrun=True)

        def queue_size(self) -> int:
            return self.queue

        def active_count(self) -> int:
            return self.active

    pump = _BudgetPump()
    timer = ManualHostTimerAdapter()
    controller = HostPumpController(pump, timer, budget_ms=5)
    controller.start()

    interval = timer.fire()

    assert interval == controller.policy.active_interval_secs
    assert pump.budgets == [5]
    assert controller.stats.queue_size == 1
    assert controller.stats.active_jobs == 1
    assert controller.stats.overrun_count == 1
    assert controller.stats.last_elapsed_ms == 12.0


def test_stop_can_shutdown_owned_pump() -> None:
    class _Pump:
        def __init__(self) -> None:
            self.shutdown_count = 0

        def drain_queue(self, budget_ms: int) -> tuple[int, int]:
            return 0, 0

        def shutdown(self) -> None:
            self.shutdown_count += 1

    pump = _Pump()
    timer = ManualHostTimerAdapter()
    controller = HostPumpController(pump, timer, shutdown_pump_on_stop=True)
    controller.start()

    controller.stop()
    controller.stop()

    assert pump.shutdown_count == 1
    assert timer.uninstall_count == 1


def test_tick_returns_none_when_pump_is_shutdown() -> None:
    class _Pump:
        is_shutdown = True

        def drain_queue(self, budget_ms: int) -> tuple[int, int]:
            raise AssertionError("shutdown pump must not be drained")

    timer = ManualHostTimerAdapter()
    controller = HostPumpController(_Pump(), timer)
    controller.start()

    assert timer.fire() is None
    assert controller.stats.shutdown is True


def test_threaded_timer_adapter_schedules_tick() -> None:
    adapter = ThreadedHostTimerAdapter()
    fired = threading.Event()

    def tick() -> None:
        fired.set()
        return None

    adapter.install(tick)
    adapter.schedule_soon()
    try:
        assert fired.wait(timeout=1.0)
    finally:
        adapter.uninstall()
    assert adapter.installed is False


def test_qt_timer_adapter_uses_single_shot_qtimer() -> None:
    class _Signal:
        def __init__(self) -> None:
            self.callback = None

        def connect(self, callback) -> None:
            self.callback = callback

        def emit(self) -> None:
            self.callback()

    class _Timer:
        def __init__(self) -> None:
            self.timeout = _Signal()
            self.single_shot = False
            self.starts: list[int] = []
            self.stopped = False

        def setSingleShot(self, value: bool) -> None:
            self.single_shot = value

        def start(self, interval_ms: int) -> None:
            self.starts.append(interval_ms)

        def stop(self) -> None:
            self.stopped = True

    class _QtCore:
        def __init__(self) -> None:
            self.timer = _Timer()

        def QTimer(self) -> _Timer:
            return self.timer

    qt_core = _QtCore()
    adapter = QtHostTimerAdapter(qt_core=qt_core)
    ticks = 0

    def tick() -> float | None:
        nonlocal ticks
        ticks += 1
        return 0.25 if ticks == 1 else None

    adapter.install(tick)
    adapter.schedule_soon()
    qt_core.timer.timeout.emit()
    adapter.uninstall()

    assert qt_core.timer.single_shot is True
    assert qt_core.timer.starts == [0, 250]
    assert qt_core.timer.stopped is True
