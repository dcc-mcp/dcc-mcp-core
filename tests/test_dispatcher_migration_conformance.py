"""Conformance fixtures for adapter dispatcher migrations.

These tests model the small adapter-owned layer that should remain after
adapters move queueing, pump lifecycle, and sidecar payload handling into
dcc-mcp-core.
"""

from __future__ import annotations

from pathlib import Path
from typing import Any
from typing import Mapping

import pytest

from dcc_mcp_core._server.callable_dispatcher import AdaptivePumpPolicy
from dcc_mcp_core._server.host_pump import HostPumpController
from dcc_mcp_core._server.host_pump import ManualHostTimerAdapter
from dcc_mcp_core._server.host_ui_dispatcher import DispatcherErrorCode
from dcc_mcp_core._server.host_ui_dispatcher import HostUiDispatcherBase
from dcc_mcp_core.cancellation import check_dcc_cancelled
from dcc_mcp_core.sidecar import ERROR_DISPATCH_FAILED
from dcc_mcp_core.sidecar import ERROR_NO_SOURCE_FILE
from dcc_mcp_core.sidecar import ERROR_PAYLOAD_MALFORMED
from dcc_mcp_core.sidecar import ERROR_SERVER_NOT_RUNNING
from dcc_mcp_core.sidecar import ERROR_UNKNOWN_ACTION
from dcc_mcp_core.sidecar import SidecarActionDispatcher
from dcc_mcp_core.sidecar import SidecarDispatchRequest


class _ImmediateManualTimer(ManualHostTimerAdapter):
    """Manual timer that fires immediately when the fake host is poked."""

    def schedule_soon(self) -> None:
        super().schedule_soon()
        if self.installed and self.tick is not None:
            self.last_interval_secs = self.tick()


class _PumpedUiDispatcher(HostUiDispatcherBase):
    def __init__(self, label: str) -> None:
        super().__init__(label=label)
        self.controller: HostPumpController | None = None
        self.events: list[tuple[str, str, int, int]] = []

    def bind_controller(self, controller: HostPumpController) -> None:
        self.controller = controller

    def poke_host_pump(self) -> None:
        if self.controller is not None:
            self.controller.schedule_soon()

    def on_job_queued(self, job) -> None:
        self.events.append(("queued", job.request_id, self.queue_size(), self.active_count()))

    def on_job_started(self, job) -> None:
        self.events.append(("started", job.request_id, self.queue_size(), self.active_count()))

    def on_job_finished(self, job) -> None:
        self.events.append(("finished", job.request_id, self.queue_size(), self.active_count()))

    def format_exception_error(self, exc: BaseException) -> str:
        return f"{self.dispatcher_label}:{type(exc).__name__}:{exc}"


class _DeferredUiDispatcher(HostUiDispatcherBase):
    def __init__(self) -> None:
        super().__init__(label="deferred-ui")
        self.pokes = 0

    def poke_host_pump(self) -> None:
        self.pokes += 1


class _FakeMayaAdapter:
    def __init__(self, tmp_path: Path) -> None:
        self.skill_root = tmp_path / "maya_skill"
        script = self.skill_root / "scripts" / "create_cube.py"
        script.parent.mkdir(parents=True)
        script.write_text("def main(**_): return {'success': True}\n", encoding="utf-8")
        boom = self.skill_root / "scripts" / "boom.py"
        boom.write_text("def main(**_): raise RuntimeError('boom')\n", encoding="utf-8")

        self.server_running = True
        self.server = {"dcc": "maya"}
        self.dispatcher = _PumpedUiDispatcher(label="maya-ui")
        self.timer = _ImmediateManualTimer()
        self.controller = HostPumpController(
            self.dispatcher,
            self.timer,
            policy=AdaptivePumpPolicy(active_interval_secs=0.01, idle_interval_secs=0.5),
            budget_ms=8,
        )
        self.dispatcher.bind_controller(self.controller)
        self.controller.start()
        self.executed: list[SidecarDispatchRequest] = []
        self.actions: dict[str, Mapping[str, Any]] = {
            "maya__create_cube": {
                "source_file": "scripts/create_cube.py",
                "skill_name": "maya_primitives",
                "thread_affinity": "main",
                "execution": "sync",
                "timeout_hint_secs": 1,
            },
            "maya__broken_source": {
                "skill_name": "broken_skill",
                "thread_affinity": "main",
            },
            "maya__executor_crash": {
                "source_file": "scripts/boom.py",
                "skill_name": "maya_primitives",
                "thread_affinity": "main",
            },
        }
        self.sidecar = SidecarActionDispatcher(
            "maya",
            server_provider=self.get_server,
            action_resolver=self.resolve_action,
            executor=self.execute,
            bundled_skill_roots=[self.skill_root],
        )

    def get_server(self) -> Mapping[str, Any] | None:
        return self.server if self.server_running else None

    def resolve_action(self, action: str, **_: Any) -> Mapping[str, Any] | None:
        return self.actions.get(action)

    def execute(self, request: SidecarDispatchRequest) -> Mapping[str, Any]:
        self.executed.append(request)
        if request.action == "maya__executor_crash":
            raise RuntimeError("host executor stopped")

        def task() -> Mapping[str, Any]:
            return {
                "success": True,
                "message": "maya dispatch ok",
                "context": {
                    "dcc": request.dcc_name,
                    "action": request.action,
                    "object_name": request.args.get("name"),
                    "script_path": request.script_path,
                },
            }

        outcome = self.dispatcher.submit_callable(
            request.request_id or request.action,
            task,
            affinity=request.thread_affinity or "main",
            timeout_ms=(request.timeout_hint_secs or 1) * 1000,
        )
        if outcome.get("success") is True and isinstance(outcome.get("output"), Mapping):
            return outcome["output"]
        return outcome


class _FakeMaxAdapter:
    def __init__(self, tmp_path: Path) -> None:
        self.script = tmp_path / "scripts" / "box.py"
        self.script.parent.mkdir(parents=True)
        self.script.write_text("def main(**_): return {'success': True}\n", encoding="utf-8")
        self.calls: list[tuple[str, Mapping[str, Any]]] = []
        self.sidecar = SidecarActionDispatcher(
            "3dsmax",
            server_provider=lambda: {"dcc": "3dsmax"},
            executor=SidecarActionDispatcher.script_executor(self.run_skill_script),
        )

    def run_skill_script(self, script_path: str, args: Mapping[str, Any]) -> Mapping[str, Any]:
        self.calls.append((script_path, dict(args)))
        if args.get("raise"):
            raise RuntimeError("max script runner failed")
        return {"node": "Box001", "length": args.get("length")}


def test_maya_like_sidecar_uses_core_dispatcher_pump_and_hooks(tmp_path: Path) -> None:
    adapter = _FakeMayaAdapter(tmp_path)

    result = adapter.sidecar.dispatch_payload(
        {
            "action": "maya__create_cube",
            "args": {"name": "pCube1"},
            "request_id": "req-1",
        },
    )

    assert result["success"] is True
    assert result["message"] == "maya dispatch ok"
    assert result["context"]["dcc"] == "maya"
    assert result["context"]["object_name"] == "pCube1"
    assert Path(result["context"]["script_path"]).name == "create_cube.py"
    assert adapter.executed[0].skill_name == "maya_primitives"
    assert adapter.executed[0].thread_affinity == "main"
    assert adapter.controller.stats.drained_jobs == 1
    assert adapter.dispatcher.events == [
        ("queued", "req-1", 1, 0),
        ("started", "req-1", 0, 1),
        ("finished", "req-1", 0, 0),
    ]


def test_3dsmax_like_sidecar_uses_core_script_runner_envelope(tmp_path: Path) -> None:
    adapter = _FakeMaxAdapter(tmp_path)

    result = adapter.sidecar.dispatch_payload(
        {
            "action": "max__create_box",
            "args": {"length": 10},
            "source_file": str(adapter.script),
            "request_id": "req-max",
        },
    )

    assert result == {
        "success": True,
        "message": "Sidecar action dispatched",
        "context": {
            "dcc_name": "3dsmax",
            "action": "max__create_box",
            "request_id": "req-max",
            "script_path": str(adapter.script),
            "result": {"node": "Box001", "length": 10},
        },
    }
    assert adapter.calls == [(str(adapter.script), {"length": 10})]


def test_sidecar_conformance_errors_cover_adapter_boundaries(tmp_path: Path) -> None:
    maya = _FakeMayaAdapter(tmp_path)
    assert maya.sidecar.dispatch_payload({"args": {}})["error"] == ERROR_PAYLOAD_MALFORMED
    assert maya.sidecar.dispatch_payload({"action": "maya__missing"})["error"] == ERROR_UNKNOWN_ACTION
    assert maya.sidecar.dispatch_payload({"action": "maya__broken_source"})["error"] == ERROR_NO_SOURCE_FILE

    maya.server_running = False
    assert maya.sidecar.dispatch_payload({"action": "maya__create_cube"})["error"] == ERROR_SERVER_NOT_RUNNING

    maya.server_running = True
    result = maya.sidecar.dispatch_payload({"action": "maya__executor_crash"})
    assert result["error"] == ERROR_DISPATCH_FAILED
    assert result["context"]["error_type"] == "RuntimeError"
    assert result["context"]["error_message"] == "host executor stopped"

    max_adapter = _FakeMaxAdapter(tmp_path)
    result = max_adapter.sidecar.dispatch_payload(
        {
            "action": "max__create_box",
            "args": {"raise": True},
            "source_file": str(max_adapter.script),
        },
    )
    assert result["error"] == ERROR_DISPATCH_FAILED
    assert result["context"]["error_message"] == "max script runner failed"


def test_ui_dispatcher_conformance_cancellation_timeout_and_shutdown() -> None:
    timeout_dispatcher = _DeferredUiDispatcher()

    timeout = timeout_dispatcher.submit_callable("timeout", lambda: "late", affinity="main", timeout_ms=1)
    assert timeout["success"] is False
    assert timeout["error"] == "Timeout (0.0s) waiting for main-thread execution"
    assert timeout_dispatcher.pokes == 1

    cancel_dispatcher = _DeferredUiDispatcher()
    pending = cancel_dispatcher.submit_async_callable("cancelled", lambda: check_dcc_cancelled(), affinity="main")
    assert pending["status"] == "pending"
    assert cancel_dispatcher.cancel("cancelled") is True
    drained, remaining = cancel_dispatcher.drain_queue(8)
    assert drained == 0
    assert remaining == 0

    cleanup = _DeferredUiDispatcher()
    queued = cleanup.submit_async_callable("pending", lambda: "ok", affinity="main")
    assert queued["status"] == "pending"
    assert cleanup.queue_size() == 1
    assert cleanup.shutdown(DispatcherErrorCode.INTERRUPTED) == 1
    assert cleanup.queue_size() == 0
    assert cleanup.active_count() == 0
    assert cleanup.is_shutdown is True
