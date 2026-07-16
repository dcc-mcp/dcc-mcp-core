"""Tests for the in-process Python skill executor (issue #521)."""

# Import built-in modules
from __future__ import annotations

from concurrent.futures import ThreadPoolExecutor
import os
from pathlib import Path
import sys
import threading
import time
from types import SimpleNamespace
from typing import Any
from typing import Callable
from typing import Mapping
from unittest.mock import MagicMock
from unittest.mock import patch

# Import third-party modules
import pytest

# Import local modules
import dcc_mcp_core
from dcc_mcp_core._server.inprocess_executor import BaseDccCallableDispatcher
from dcc_mcp_core._server.inprocess_executor import DeferredToolResult
from dcc_mcp_core._server.inprocess_executor import HostExecutionBridge
from dcc_mcp_core._server.inprocess_executor import InProcessExecutionContext
from dcc_mcp_core._server.inprocess_executor import build_inprocess_executor
from dcc_mcp_core._server.inprocess_executor import clear_script_package
from dcc_mcp_core._server.inprocess_executor import exception_to_error_envelope
from dcc_mcp_core._server.inprocess_executor import run_skill_script

# ── public surface ───────────────────────────────────────────────────────────


def test_base_dispatcher_exported_from_top_level() -> None:
    assert hasattr(dcc_mcp_core, "BaseDccCallableDispatcher")
    assert "BaseDccCallableDispatcher" in dcc_mcp_core.__all__
    assert hasattr(dcc_mcp_core, "DeferredToolResult")
    assert "DeferredToolResult" in dcc_mcp_core.__all__
    assert hasattr(dcc_mcp_core, "HostExecutionBridge")
    assert "HostExecutionBridge" in dcc_mcp_core.__all__
    assert hasattr(dcc_mcp_core, "InProcessExecutionContext")
    assert "InProcessExecutionContext" in dcc_mcp_core.__all__


def test_helpers_exported_from_underscore_server() -> None:
    # Import local modules
    from dcc_mcp_core._server import BaseDccCallableDispatcher as B
    from dcc_mcp_core._server import DeferredToolResult as DTR
    from dcc_mcp_core._server import HostExecutionBridge as HEB
    from dcc_mcp_core._server import InProcessExecutionContext as IEC
    from dcc_mcp_core._server import build_inprocess_executor as BIE
    from dcc_mcp_core._server import run_skill_script as RSS

    assert B is BaseDccCallableDispatcher
    assert DTR is DeferredToolResult
    assert HEB is HostExecutionBridge
    assert BIE is build_inprocess_executor
    assert IEC is InProcessExecutionContext
    assert RSS is run_skill_script


def test_protocol_is_runtime_checkable() -> None:
    class _D:
        def dispatch_callable(self, func: Callable[..., Any], *args: Any, **kwargs: Any) -> Any:
            return func(*args, **kwargs)

    assert isinstance(_D(), BaseDccCallableDispatcher)


# ── run_skill_script ────────────────────────────────────────────────────────


def _write_script(tmp_path: Path, body: str) -> Path:
    p = tmp_path / "skill.py"
    p.write_text(body, encoding="utf-8")
    return p


def test_run_skill_script_calls_main_with_params(tmp_path: Path) -> None:
    p = _write_script(
        tmp_path,
        "def main(a, b=2):\n    return {'sum': a + b}\n",
    )
    assert run_skill_script(str(p), {"a": 5}) == {"sum": 7}


def test_run_skill_script_strips_reserved_metadata(tmp_path: Path) -> None:
    p = _write_script(
        tmp_path,
        "def main(**kwargs):\n    return sorted(kwargs)\n",
    )
    assert run_skill_script(str(p), {"color": "red", "_meta": {"session": "test"}}) == ["color"]


def test_run_skill_script_supports_legacy_bare_sibling_imports(tmp_path: Path) -> None:
    (tmp_path / "_helper.py").write_text("VALUE = 42\n", encoding="utf-8")
    p = _write_script(tmp_path, "from _helper import VALUE\ndef main(): return VALUE\n")
    assert run_skill_script(str(p), {}) == 42
    assert str(tmp_path) not in __import__("sys").path


def test_run_skill_script_supports_isolated_relative_sibling_imports(tmp_path: Path) -> None:
    (tmp_path / "_helper.py").write_text("VALUE = 42\n", encoding="utf-8")
    p = _write_script(tmp_path, "from ._helper import VALUE\ndef main(): return VALUE\n")
    assert run_skill_script(str(p), {}) == 42
    assert str(tmp_path) not in __import__("sys").path


def test_run_skill_script_isolates_same_named_helpers_across_directories_concurrently(tmp_path: Path) -> None:
    scripts = []
    for directory_name, value in (("first", "alpha"), ("second", "beta")):
        script_dir = tmp_path / directory_name
        script_dir.mkdir()
        (script_dir / "_helper.py").write_text(f"VALUE = {value!r}\n", encoding="utf-8")
        script = script_dir / "tool.py"
        script.write_text("from ._helper import VALUE\ndef main(): return VALUE\n", encoding="utf-8")
        scripts.append((script, value))

    calls = scripts * 12
    with ThreadPoolExecutor(max_workers=8) as pool:
        results = list(pool.map(lambda item: run_skill_script(str(item[0]), {}), calls))

    assert results == [expected for _, expected in calls]


def test_legacy_bare_sibling_imports_are_isolated_concurrently(tmp_path: Path) -> None:
    scripts = []
    for directory_name, value in (("first", "alpha"), ("second", "beta")):
        script_dir = tmp_path / directory_name
        script_dir.mkdir()
        (script_dir / "_legacy_helper.py").write_text(f"VALUE = {value!r}\n", encoding="utf-8")
        script = script_dir / "tool.py"
        script.write_text("from _legacy_helper import VALUE\ndef main(): return VALUE\n", encoding="utf-8")
        scripts.append((script, value))

    calls = scripts * 8
    with ThreadPoolExecutor(max_workers=8) as pool:
        results = list(pool.map(lambda item: run_skill_script(str(item[0]), {}), calls))

    assert results == [expected for _, expected in calls]


def test_run_skill_script_shares_helper_state_within_one_skill_directory(tmp_path: Path) -> None:
    (tmp_path / "_state.py").write_text(
        "EVENTS = []\ndef record(event):\n    EVENTS.append(event)\n    return list(EVENTS)\n",
        encoding="utf-8",
    )
    snapshot = tmp_path / "snapshot.py"
    snapshot.write_text("from ._state import record\ndef main(): return record('snapshot')\n", encoding="utf-8")
    act = tmp_path / "act.py"
    act.write_text("from ._state import record\ndef main(): return record('act')\n", encoding="utf-8")

    assert run_skill_script(str(snapshot), {}) == ["snapshot"]
    assert run_skill_script(str(act), {}) == ["snapshot", "act"]


def test_clear_script_package_runs_cleanup_and_reloads_helpers(tmp_path: Path) -> None:
    sentinel = tmp_path / "cleaned.txt"
    helper = tmp_path / "_helper.py"
    helper.write_text(
        "from pathlib import Path\n"
        "VALUE = 'old'\n"
        f"def cleanup(): Path({str(sentinel)!r}).write_text('yes', encoding='utf-8')\n",
        encoding="utf-8",
    )
    script = _write_script(tmp_path, "from ._helper import VALUE\ndef main(): return VALUE\n")

    assert run_skill_script(str(script), {}) == "old"
    assert clear_script_package(tmp_path) >= 2
    assert sentinel.read_text(encoding="utf-8") == "yes"

    helper.write_text("VALUE = 'new-value'\n", encoding="utf-8")
    assert run_skill_script(str(script), {}) == "new-value"
    clear_script_package(tmp_path)


def test_package_calls_can_overlap_while_cleanup_waits(tmp_path: Path) -> None:
    (tmp_path / "_control.py").write_text(
        "release = None\n"
        "def configure(event):\n    global release\n    release = event\n"
        "def request_stop():\n    if release is not None:\n        release.set()\n",
        encoding="utf-8",
    )
    long_call = tmp_path / "act.py"
    long_call.write_text(
        "from ._control import configure\n"
        "def main(started, release):\n"
        "    configure(release)\n"
        "    started.set()\n"
        "    release.wait(2)\n"
        "    return 'act-done'\n",
        encoding="utf-8",
    )
    stop_call = tmp_path / "stop.py"
    stop_call.write_text("def main(stopped): stopped.set(); return 'stopped'\n", encoding="utf-8")
    started = threading.Event()
    release = threading.Event()
    stopped = threading.Event()
    cleanup_started = threading.Event()
    cleaned = threading.Event()

    with ThreadPoolExecutor(max_workers=3) as pool:
        active = pool.submit(run_skill_script, str(long_call), {"started": started, "release": release})
        assert started.wait(1)
        assert run_skill_script(str(stop_call), {"stopped": stopped}) == "stopped"
        assert stopped.is_set()

        def _cleanup() -> None:
            cleanup_started.set()
            clear_script_package(tmp_path)
            cleaned.set()

        cleanup = pool.submit(_cleanup)
        assert cleanup_started.wait(1)
        assert cleaned.wait(1)
        assert active.result(timeout=1) == "act-done"
        cleanup.result(timeout=1)

    assert cleaned.is_set()
    assert release.is_set()


def test_shutdown_defers_unload_for_non_cooperative_active_call(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    caplog: pytest.LogCaptureFixture,
) -> None:
    import dcc_mcp_core._server.inprocess_executor as inprocess_executor

    cleaned = tmp_path / "cleaned.txt"
    (tmp_path / "_state.py").write_text(
        f"from pathlib import Path\ndef cleanup(): Path({str(cleaned)!r}).write_text('yes', encoding='utf-8')\n",
        encoding="utf-8",
    )
    script = _write_script(
        tmp_path,
        "from . import _state\n"
        "def main(started, release):\n"
        "    started.set()\n"
        "    release.wait(5)\n"
        "    return 'done'\n",
    )
    started = threading.Event()
    release = threading.Event()
    bridge = HostExecutionBridge()
    monkeypatch.setattr(inprocess_executor, "_SCRIPT_PACKAGE_CLEAR_TIMEOUT_SECS", 0.05)

    with ThreadPoolExecutor(max_workers=1) as pool:
        call = pool.submit(bridge.execute_script, str(script), {"started": started, "release": release})
        assert started.wait(1)

        before = time.monotonic()
        assert bridge.shutdown_script_execution() == 0
        assert time.monotonic() - before < 0.5
        assert not cleaned.exists()
        assert any(
            getattr(module, "__dcc_mcp_script_dir__", None) == os.path.normcase(str(tmp_path))
            for module in sys.modules.values()
            if module is not None
        )
        assert "modules remain loaded" in caplog.text

        release.set()
        assert call.result(timeout=1) == "done"

    assert cleaned.read_text(encoding="utf-8") == "yes"
    assert not any(
        getattr(module, "__dcc_mcp_script_dir__", None) == os.path.normcase(str(tmp_path))
        for module in sys.modules.values()
        if module is not None
    )


def test_shutdown_stays_bounded_when_request_stop_blocks(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    import dcc_mcp_core._server.inprocess_executor as inprocess_executor

    (tmp_path / "_state.py").write_text(
        "stop_release = None\n"
        "cleaned = None\n"
        "def configure(stop_event, cleaned_event):\n"
        "    global stop_release, cleaned\n"
        "    stop_release, cleaned = stop_event, cleaned_event\n"
        "def request_stop(): stop_release.wait(5)\n"
        "def cleanup(): cleaned.set()\n",
        encoding="utf-8",
    )
    script = _write_script(
        tmp_path,
        "from ._state import configure\n"
        "def main(started, release, stop_release, cleaned):\n"
        "    configure(stop_release, cleaned)\n"
        "    started.set()\n"
        "    release.wait(5)\n"
        "    return 'done'\n",
    )
    started = threading.Event()
    release = threading.Event()
    stop_release = threading.Event()
    cleaned = threading.Event()
    bridge = HostExecutionBridge()
    monkeypatch.setattr(inprocess_executor, "_SCRIPT_PACKAGE_CLEAR_TIMEOUT_SECS", 0.05)

    with ThreadPoolExecutor(max_workers=1) as pool:
        call = pool.submit(
            bridge.execute_script,
            str(script),
            {
                "started": started,
                "release": release,
                "stop_release": stop_release,
                "cleaned": cleaned,
            },
        )
        assert started.wait(1)
        before = time.monotonic()
        assert bridge.shutdown_script_execution() == 0
        assert time.monotonic() - before < 0.5

        release.set()
        assert call.result(timeout=1) == "done"
        assert not cleaned.is_set()
        stop_release.set()
        assert cleaned.wait(1)


def test_run_skill_script_supports_single_dict_main(tmp_path: Path) -> None:
    p = _write_script(
        tmp_path,
        "def main(params):\n    return {'scale': params['scale']}\n",
    )
    assert run_skill_script(str(p), {"scale": 0.25}) == {"scale": 0.25}


def test_run_skill_script_supports_optional_dict_main(tmp_path: Path) -> None:
    p = _write_script(
        tmp_path,
        "def main(args=None):\n    return {'keys': sorted((args or {}).keys())}\n",
    )
    assert run_skill_script(str(p), {"steps": []}) == {"keys": ["steps"]}


def test_run_skill_script_missing_main_raises(tmp_path: Path) -> None:
    p = _write_script(tmp_path, "value = 42\n")
    with pytest.raises(AttributeError, match="`main` callable"):
        run_skill_script(str(p), {})


def test_run_skill_script_missing_file_raises() -> None:
    with pytest.raises(FileNotFoundError):
        run_skill_script("nope/doesnt/exist.py", {})


def test_run_skill_script_systemexit_returns_mcp_result(tmp_path: Path) -> None:
    """Mirrors Maya's existing convention used by some skills."""
    p = _write_script(
        tmp_path,
        "import sys\n__mcp_result__ = {'ok': True, 'frames': 12}\ndef main(**_):\n    sys.exit(0)\n",
    )
    assert run_skill_script(str(p), {}) == {"ok": True, "frames": 12}


def test_run_skill_script_systemexit_at_module_level(tmp_path: Path) -> None:
    p = _write_script(
        tmp_path,
        "__mcp_result__ = {'fast_path': True}\nraise SystemExit(0)\n",
    )
    assert run_skill_script(str(p), {}) == {"fast_path": True}


def test_run_skill_script_cleans_ephemeral_entry_modules(tmp_path: Path) -> None:
    # Import built-in modules
    import sys

    before = {k for k in sys.modules if k.startswith("_dcc_mcp_skill_") and "._entry_" in k}
    p = _write_script(tmp_path, "def main(): return 'ok'\n")
    run_skill_script(str(p), {})
    after = {k for k in sys.modules if k.startswith("_dcc_mcp_skill_") and "._entry_" in k}
    assert after == before, "ephemeral entry module leaked into sys.modules"


# ── build_inprocess_executor ────────────────────────────────────────────────


def test_executor_inline_when_dispatcher_is_none(tmp_path: Path) -> None:
    p = _write_script(tmp_path, "def main(x): return x * 2\n")
    executor = build_inprocess_executor(None)
    assert executor(str(p), {"x": 21}) == 42


def test_executor_routes_through_dispatcher(tmp_path: Path) -> None:
    p = _write_script(tmp_path, "def main(x): return x + 1\n")

    class _DispatcherSpy:
        def __init__(self) -> None:
            self.calls: list[tuple[Any, Any, Any]] = []

        def dispatch_callable(
            self,
            func: Callable[..., Any],
            *args: Any,
            **kwargs: Any,
        ) -> Any:
            self.calls.append((func, args, kwargs))
            return func(*args, **kwargs)

    spy = _DispatcherSpy()
    executor = build_inprocess_executor(spy)
    assert executor(str(p), {"x": 41}) == 42
    assert len(spy.calls) == 1
    func, args, kwargs = spy.calls[0]
    assert callable(func)
    assert args == ()
    assert kwargs["affinity"] == "any"
    assert kwargs["context"] == InProcessExecutionContext()


def test_executor_dispatcher_exception_becomes_error_envelope(tmp_path: Path) -> None:
    """Issue #589 — dispatcher / runner failures must surface as structured
    error dicts so Rust ``CallToolResult`` can flag ``isError: true`` from
    the ``success: false`` heuristic without forcing clients to do a second
    JSON parse on the content text.
    """
    p = _write_script(tmp_path, "def main(): return None\n")

    class _BoomDispatcher:
        def dispatch_callable(
            self,
            func: Callable[..., Any],
            *args: Any,
            **kwargs: Any,
        ) -> Any:
            raise RuntimeError("UI thread shutdown")

    executor = build_inprocess_executor(_BoomDispatcher())
    result = executor(str(p), {})
    assert isinstance(result, dict)
    assert result["success"] is False
    assert "UI thread shutdown" in result["message"]
    assert result["error"]["type"] == "RuntimeError"
    assert result["error"]["message"] == "UI thread shutdown"
    assert "Traceback" in result["error"]["traceback"]


def test_executor_inline_exception_becomes_error_envelope(tmp_path: Path) -> None:
    p = _write_script(
        tmp_path,
        "def main(): raise ValueError('bad input')\n",
    )
    executor = build_inprocess_executor(None)
    result = executor(str(p), {})
    assert isinstance(result, dict)
    assert result["success"] is False
    assert result["error"]["type"] == "ValueError"
    assert result["error"]["message"] == "bad input"
    assert "Traceback" in result["error"]["traceback"]


def test_exception_to_error_envelope_overrides_message() -> None:
    try:
        raise KeyError("missing")
    except KeyError as exc:
        envelope = exception_to_error_envelope(exc, message="custom summary")
    assert envelope == {
        "success": False,
        "message": "custom summary",
        "error": {
            "type": "KeyError",
            "message": "'missing'",
            "traceback": envelope["error"]["traceback"],
        },
    }
    assert "KeyError" in envelope["error"]["traceback"]


def test_executor_uses_custom_runner() -> None:
    seen: list[tuple[str, Mapping[str, Any]]] = []

    def _fake_runner(script_path: str, params: Mapping[str, Any]) -> str:
        seen.append((script_path, params))
        return f"{script_path}|{dict(params)}"

    executor = build_inprocess_executor(None, runner=_fake_runner)
    out = executor("/tmp/skill.py", {"k": "v"})
    assert seen == [("/tmp/skill.py", {"k": "v"})]
    assert out == "/tmp/skill.py|{'k': 'v'}"


def test_executor_passes_execution_context_to_dispatcher() -> None:
    seen: list[tuple[str, Mapping[str, Any]]] = []

    def _fake_runner(script_path: str, params: Mapping[str, Any]) -> dict[str, Any]:
        seen.append((script_path, params))
        return {"ok": True}

    class _DispatcherSpy:
        def __init__(self) -> None:
            self.kwargs: dict[str, Any] = {}

        def dispatch_callable(
            self,
            func: Callable[..., Any],
            *args: Any,
            **kwargs: Any,
        ) -> Any:
            self.kwargs = kwargs
            return func(*args, **kwargs)

    spy = _DispatcherSpy()
    executor = build_inprocess_executor(spy, runner=_fake_runner)
    result = executor(
        "/tmp/tool.py",
        {"value": 1},
        action_name="demo__tool",
        skill_name="demo",
        thread_affinity="main",
        execution="async",
        timeout_hint_secs=30,
    )

    assert result == {"ok": True}
    assert seen == [("/tmp/tool.py", {"value": 1})]
    assert spy.kwargs["affinity"] == "main"
    assert spy.kwargs["action_name"] == "demo__tool"
    assert spy.kwargs["skill_name"] == "demo"
    assert spy.kwargs["execution"] == "async"
    assert spy.kwargs["timeout_hint_secs"] == 30
    assert spy.kwargs["context"] == InProcessExecutionContext(
        action_name="demo__tool",
        skill_name="demo",
        thread_affinity="main",
        execution="async",
        timeout_hint_secs=30,
    )


# ── HostExecutionBridge ─────────────────────────────────────────────────────


def test_host_execution_bridge_executes_script_inline(tmp_path: Path) -> None:
    p = _write_script(tmp_path, "def main(x): return {'value': x + 2}\n")
    bridge = HostExecutionBridge()

    assert bridge.execute_script(str(p), {"x": 40}) == {"value": 42}


def test_host_execution_bridge_shutdown_rejects_queued_and_new_scripts(tmp_path: Path) -> None:
    marker = tmp_path / "ran.txt"
    script = _write_script(
        tmp_path,
        f"from pathlib import Path\ndef main(): Path({str(marker)!r}).write_text('yes'); return 'ran'\n",
    )
    queued = threading.Event()
    release = threading.Event()

    class _QueuedDispatcher:
        def dispatch_callable(self, func: Callable[..., Any], **_kwargs: Any) -> Any:
            queued.set()
            assert release.wait(2)
            return func()

    bridge = HostExecutionBridge(dispatcher=_QueuedDispatcher())
    with ThreadPoolExecutor(max_workers=2) as pool:
        call = pool.submit(bridge.execute_script, str(script), {})
        assert queued.wait(1)
        shutdown = pool.submit(bridge.shutdown_script_execution)
        shutdown.result(timeout=1)
        assert bridge.execute_script(str(script), {})["success"] is False
        bridge.resume_script_execution()
        release.set()
        result = call.result(timeout=1)

    assert result["success"] is False
    assert "shutting down" in result["message"]
    assert not marker.exists()

    assert bridge.execute_script(str(script), {}) == "ran"
    bridge.shutdown_script_execution()


def test_host_execution_bridges_own_independent_script_packages(tmp_path: Path) -> None:
    (tmp_path / "_state.py").write_text(
        "count = 0\ndef next_value():\n    global count\n    count += 1\n    return count\n",
        encoding="utf-8",
    )
    script = _write_script(tmp_path, "from ._state import next_value\ndef main(): return next_value()\n")
    first = HostExecutionBridge()
    second = HostExecutionBridge()

    assert first.execute_script(str(script), {}) == 1
    assert first.execute_script(str(script), {}) == 2
    assert second.execute_script(str(script), {}) == 1

    assert first.clear_script_packages() >= 2
    assert second.execute_script(str(script), {}) == 2
    assert first.execute_script(str(script), {}) == 1

    first.clear_script_packages()
    second.clear_script_packages()


def test_hot_reload_clears_owned_helpers(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    from dcc_mcp_core.hotreload import DccSkillHotReloader

    class _Watcher:
        def __init__(self, debounce_ms: int) -> None:
            self.callback: Callable[[], None] | None = None

        def on_reload(self, callback: Callable[[], None]) -> None:
            self.callback = callback

        def watch(self, _path: str) -> None:
            assert self.callback is not None
            self.callback()

    helper = tmp_path / "_helper.py"
    helper.write_text("VALUE = 'old'\n", encoding="utf-8")
    script = _write_script(tmp_path, "from ._helper import VALUE\ndef main(): return VALUE\n")
    bridge = HostExecutionBridge()
    assert bridge.execute_script(str(script), {}) == "old"
    helper.write_text("VALUE = 'updated-value'\n", encoding="utf-8")

    monkeypatch.setattr(dcc_mcp_core, "SkillWatcher", _Watcher)
    reloader = DccSkillHotReloader("test", SimpleNamespace(_execution_bridge=bridge))
    assert reloader.enable([str(tmp_path)]) is True
    assert bridge.execute_script(str(script), {}) == "updated-value"

    bridge.clear_script_packages()


def test_hot_reload_invalidates_an_already_dispatched_call(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    import dcc_mcp_core._server.inprocess_executor as inprocess_executor

    cleaned = tmp_path / "cleaned.txt"
    (tmp_path / "_state.py").write_text(
        "from pathlib import Path\n"
        "VALUE = 'recreated'\n"
        f"def cleanup(): Path({str(cleaned)!r}).write_text('yes', encoding='utf-8')\n",
        encoding="utf-8",
    )
    script = _write_script(tmp_path, "from ._state import VALUE\ndef main(): return VALUE\n")
    entered_runner = threading.Event()
    release_runner = threading.Event()
    original_runner = inprocess_executor.run_skill_script

    def _delayed_runner(
        script_path: str,
        params: Mapping[str, Any],
        **kwargs: Any,
    ) -> Any:
        entered_runner.set()
        assert release_runner.wait(2)
        return original_runner(script_path, params, **kwargs)

    monkeypatch.setattr(inprocess_executor, "run_skill_script", _delayed_runner)
    bridge = HostExecutionBridge(runner=_delayed_runner)

    with ThreadPoolExecutor(max_workers=1) as pool:
        call = pool.submit(bridge.execute_script, str(script), {})
        assert entered_runner.wait(1)
        # The dispatched call has recorded ownership but has not created its
        # private package yet. Hot reload must invalidate that queued call.
        assert bridge.clear_script_package(tmp_path) == 0
        release_runner.set()
        result = call.result(timeout=1)

    assert result["success"] is False
    assert result["error"]["type"] == "RuntimeError"
    assert not cleaned.exists()
    assert bridge.shutdown_script_execution() == 0
    assert not cleaned.exists()

    bridge.resume_script_execution()
    assert bridge.execute_script(str(script), {}) == "recreated"
    bridge.shutdown_script_execution()
    assert cleaned.read_text(encoding="utf-8") == "yes"


def test_host_execution_bridge_routes_direct_callable_with_context() -> None:
    class _DispatcherSpy:
        def __init__(self) -> None:
            self.kwargs: dict[str, Any] = {}

        def dispatch_callable(
            self,
            func: Callable[..., Any],
            *args: Any,
            **kwargs: Any,
        ) -> Any:
            self.kwargs = kwargs
            return func(*args, **kwargs)

    spy = _DispatcherSpy()
    bridge = HostExecutionBridge(dispatcher=spy)

    result = bridge.dispatch_callable(
        lambda value: value * 2,
        21,
        action_name="demo__call",
        skill_name="demo",
        thread_affinity="main",
        execution="async",
        timeout_hint_secs=10,
    )

    assert result == 42
    assert spy.kwargs["affinity"] == "main"
    assert spy.kwargs["context"] == InProcessExecutionContext(
        action_name="demo__call",
        skill_name="demo",
        thread_affinity="main",
        execution="async",
        timeout_hint_secs=10,
    )


def test_host_execution_bridge_reuses_queue_dispatcher_for_direct_callable() -> None:
    from dcc_mcp_core.host import QueueDispatcher
    from dcc_mcp_core.host import StandaloneHost

    dispatcher = QueueDispatcher()
    bridge = HostExecutionBridge(dispatcher=dispatcher)
    caller_tid = threading.get_ident()

    with StandaloneHost(dispatcher, tick_interval=0.001) as host:
        host_tid = host._thread.ident  # type: ignore[union-attr]
        result = bridge.dispatch_callable(lambda value: (value * 2, threading.get_ident()), 21)

    assert result[0] == 42
    assert result[1] == host_tid
    assert result[1] != caller_tid
    assert bridge.resolve_host_dispatcher() is dispatcher


def test_host_execution_bridge_exposes_host_ui_dispatcher_to_http_routing() -> None:
    """Core UI dispatchers must also satisfy HTTP main-affinity routing."""
    from dcc_mcp_core import HostUiDispatcherBase

    class _UiDispatcher(HostUiDispatcherBase):
        def poke_host_pump(self) -> None:
            pass

        def dispatch_callable(self, func: Callable[..., Any], *args: Any, **kwargs: Any) -> Any:
            result = self.submit_callable("nested", lambda: func(*args), timeout_ms=10)
            if not result["success"]:
                raise RuntimeError(result["error"])
            return result["output"]

    dispatcher = _UiDispatcher()
    bridge = HostExecutionBridge(dispatcher=dispatcher)

    http_dispatcher = bridge.resolve_host_dispatcher()

    assert http_dispatcher is not None
    assert callable(http_dispatcher.post)
    assert callable(http_dispatcher.tick)

    result = http_dispatcher.post(lambda: 42)
    assert dispatcher.pending_count() == 1
    executed, pending = dispatcher.drain_queue(8)

    assert executed == 1
    assert pending == 0
    assert result.wait(timeout=1) == 42

    nested = http_dispatcher.post(
        lambda: bridge.dispatch_callable(
            lambda: 42,
            thread_affinity="main",
            timeout_hint_secs=0.01,
        )
    )
    dispatcher.drain_queue(8)

    assert nested.wait(timeout=1) == 42


def test_host_execution_bridge_as_inprocess_executor_uses_runner() -> None:
    seen: list[tuple[str, Mapping[str, Any]]] = []

    def _fake_runner(script_path: str, params: Mapping[str, Any]) -> dict[str, Any]:
        seen.append((script_path, params))
        return {"ok": True}

    bridge = HostExecutionBridge(runner=_fake_runner)
    executor = bridge.as_inprocess_executor()

    assert executor("/tmp/skill.py", {"k": "v"}) == {"ok": True}
    assert seen == [("/tmp/skill.py", {"k": "v"})]


def test_host_execution_bridge_prepares_file_backed_script_params(tmp_path: Path) -> None:
    bridge = HostExecutionBridge(
        script_materialization_policy="auto",
        script_materialization_root=tmp_path,
    )

    params = bridge.prepare_script_execution_params(
        {"code": "result = 3"},
        dcc_type="custom",
        instance_id="inst-1",
        session_id="sess-1",
        tool_call_id="call-1",
    )

    assert params.file_path is not None
    assert Path(params.file_path).is_file()
    assert params.materialized_script is not None
    assert params.materialized_script.tool_call_id == "call-1"


def test_host_execution_bridge_resolves_deferred_tool_result() -> None:
    attempts = 0

    def _check() -> dict[str, Any] | None:
        nonlocal attempts
        attempts += 1
        if attempts < 3:
            return None
        return {"success": True, "value": 42}

    bridge = HostExecutionBridge()

    result = bridge.dispatch_callable(
        lambda: DeferredToolResult(
            check_is_finished=_check,
            timeout_secs=1,
            poll_interval_secs=0.001,
            stdout="started render",
        ),
        execution="async",
    )

    assert result == {
        "success": True,
        "value": 42,
        "_meta": {
            "dcc.deferred": {
                "stdout": "started render",
                "stderr": "",
            },
        },
    }
    assert attempts == 3


def test_host_execution_bridge_polls_deferred_via_dispatcher() -> None:
    calls: list[str] = []

    class _DispatcherSpy:
        def dispatch_callable(
            self,
            func: Callable[..., Any],
            *args: Any,
            **kwargs: Any,
        ) -> Any:
            calls.append(kwargs["context"].execution)
            return func(*args, **kwargs)

    bridge = HostExecutionBridge(dispatcher=_DispatcherSpy())
    result = bridge.dispatch_callable(
        lambda: DeferredToolResult(
            check_is_finished=lambda: {"done": True},
            timeout_secs=1,
            poll_interval_secs=0.001,
        ),
        execution="async",
    )

    assert result == {"done": True}
    assert calls == ["async", "async"]


def test_deferred_tool_result_timeout_becomes_error_envelope() -> None:
    bridge = HostExecutionBridge()

    result = bridge.dispatch_callable(
        lambda: DeferredToolResult(
            check_is_finished=lambda: None,
            timeout_secs=0.001,
            poll_interval_secs=0.001,
            stderr="still rendering",
        ),
    )

    assert result["success"] is False
    assert result["error"]["type"] == "TimeoutError"
    assert result["_meta"]["dcc.deferred"]["stderr"] == "still rendering"


def test_deferred_tool_result_non_serialisable_result_is_error() -> None:
    bridge = HostExecutionBridge()

    result = bridge.dispatch_callable(
        lambda: DeferredToolResult(
            check_is_finished=lambda: {"bad": object()},
            timeout_secs=1,
            poll_interval_secs=0.001,
        ),
    )

    assert result["success"] is False
    assert result["message"] == "Deferred tool returned a non-serialisable result"


# ── DccServerBase.register_inprocess_executor integration ───────────────────


def _patch_set_in_process_executor(server_base: Any, sink: list[Callable[..., Any]]) -> None:
    """Replace ``base._server.set_in_process_executor`` with a python sink.

    The Rust pyclass attribute is read-only, so the patch is applied at
    the Python wrapper level by reassigning ``_server`` to a tiny
    delegate that forwards everything else to the original handle.
    """

    class _Sink:
        def __init__(self, real: Any) -> None:
            self._real = real

        def set_in_process_executor(self, executor: Callable[..., Any]) -> None:
            sink.append(executor)

        def __getattr__(self, item: str) -> Any:
            return getattr(self._real, item)

    server_base._server = _Sink(server_base._server)


def test_register_host_execution_bridge_is_idempotent_for_same_bridge(tmp_path: Path) -> None:
    from dcc_mcp_core._server.options import DccServerOptions
    from dcc_mcp_core.server_base import DccServerBase

    opts = DccServerOptions.from_env(
        "test_bridge_same",
        tmp_path,
        port=0,
        enable_file_logging=False,
        enable_job_persistence=False,
        enable_telemetry=False,
    )
    with patch("dcc_mcp_core.create_skill_server", return_value=MagicMock()):
        base = DccServerBase(opts)
    captured: list[Callable[..., Any]] = []
    _patch_set_in_process_executor(base, captured)
    bridge = HostExecutionBridge()

    base.register_host_execution_bridge(bridge)
    base.register_host_execution_bridge(bridge)

    assert base._execution_bridge is bridge
    assert len(captured) == 1


def test_register_host_execution_bridge_rejects_different_active_bridge(tmp_path: Path) -> None:
    from dcc_mcp_core._server.options import DccServerOptions
    from dcc_mcp_core.server_base import DccServerBase

    opts = DccServerOptions.from_env(
        "test_bridge_rebind",
        tmp_path,
        port=0,
        enable_file_logging=False,
        enable_job_persistence=False,
        enable_telemetry=False,
    )
    with patch("dcc_mcp_core.create_skill_server", return_value=MagicMock()):
        base = DccServerBase(opts)
    captured: list[Callable[..., Any]] = []
    _patch_set_in_process_executor(base, captured)
    active_bridge = HostExecutionBridge()

    base.register_host_execution_bridge(active_bridge)
    with pytest.raises(RuntimeError, match=r"new DccServerBase"):
        base.register_host_execution_bridge(HostExecutionBridge())

    assert base._execution_bridge is active_bridge
    assert len(captured) == 1


def test_register_inprocess_executor_calls_underlying_setter(tmp_path: Path) -> None:
    # Import local modules
    from dcc_mcp_core._server.options import DccServerOptions
    from dcc_mcp_core.server_base import DccServerBase

    opts = DccServerOptions.from_env(
        "test_inproc_a",
        tmp_path,
        port=0,
        enable_file_logging=False,
        enable_job_persistence=False,
        enable_telemetry=False,
    )
    with patch("dcc_mcp_core.create_skill_server", return_value=MagicMock()):
        base = DccServerBase(opts)
    captured: list[Callable[..., Any]] = []
    _patch_set_in_process_executor(base, captured)

    base.register_inprocess_executor()
    assert len(captured) == 1
    assert callable(captured[0])


def test_register_inprocess_executor_is_idempotent_for_same_dispatcher(tmp_path: Path) -> None:
    from dcc_mcp_core._server.options import DccServerOptions
    from dcc_mcp_core.server_base import DccServerBase

    opts = DccServerOptions.from_env(
        "test_inproc_same",
        tmp_path,
        port=0,
        enable_file_logging=False,
        enable_job_persistence=False,
        enable_telemetry=False,
    )
    with patch("dcc_mcp_core.create_skill_server", return_value=MagicMock()):
        base = DccServerBase(opts)
    captured: list[Callable[..., Any]] = []
    _patch_set_in_process_executor(base, captured)

    class _D:
        def dispatch_callable(self, func: Callable[..., Any], *args: Any, **kwargs: Any) -> Any:
            return func(*args, **kwargs)

    dispatcher = _D()
    base.register_inprocess_executor(dispatcher)
    base.register_inprocess_executor(dispatcher)

    assert len(captured) == 1


def test_register_inprocess_executor_with_dispatcher_routes(tmp_path: Path) -> None:
    # Import local modules
    from dcc_mcp_core._server.options import DccServerOptions
    from dcc_mcp_core.server_base import DccServerBase

    opts = DccServerOptions.from_env(
        "test_inproc_b",
        tmp_path,
        port=0,
        enable_file_logging=False,
        enable_job_persistence=False,
        enable_telemetry=False,
    )
    with patch("dcc_mcp_core.create_skill_server", return_value=MagicMock()):
        base = DccServerBase(opts)
    captured: list[Callable[..., Any]] = []
    _patch_set_in_process_executor(base, captured)

    class _D:
        def __init__(self) -> None:
            self.count = 0

        def dispatch_callable(
            self,
            func: Callable[..., Any],
            *args: Any,
            **kwargs: Any,
        ) -> Any:
            self.count += 1
            return func(*args, **kwargs)

    dispatcher = _D()
    base.register_inprocess_executor(dispatcher)
    assert len(captured) == 1

    p = _write_script(tmp_path, "def main(x): return x * 3\n")
    assert captured[0](str(p), {"x": 7}) == 21
    assert dispatcher.count == 1


def test_bridge_cleans_on_skill_unload_and_every_server_stop(tmp_path: Path) -> None:
    from dcc_mcp_core._testing import make_test_server

    class _EventBus:
        def __init__(self) -> None:
            self.callback: Callable[[dict[str, Any]], None] | None = None

        def subscribe(self, event_name: str, callback: Callable[[dict[str, Any]], None]) -> int:
            assert event_name == "skill.unloaded"
            self.callback = callback
            return 1

        def unsubscribe(self, event_name: str, subscriber_id: int) -> bool:
            assert (event_name, subscriber_id) == ("skill.unloaded", 1)
            self.callback = None
            return True

        def emit_unloaded(self, skill_path: Path) -> None:
            assert self.callback is not None
            self.callback({"attributes": {"skill_path": str(skill_path)}})

    class _Handle:
        port = 0

        def mcp_url(self) -> str:
            return "http://127.0.0.1:0/mcp"

        def shutdown(self) -> None:
            pass

    class _Inner:
        def __init__(self) -> None:
            self.bus = _EventBus()
            self.executor: Callable[..., Any] | None = None

        def set_in_process_executor(self, executor: Callable[..., Any]) -> None:
            self.executor = executor

        def event_bus(self) -> _EventBus:
            return self.bus

        def start(self) -> _Handle:
            return _Handle()

    skill_dir = tmp_path / "skill"
    scripts = skill_dir / "scripts"
    scripts.mkdir(parents=True)
    (scripts / "_state.py").write_text(
        "count = 0\ndef next_value():\n    global count\n    count += 1\n    return count\n",
        encoding="utf-8",
    )
    script = scripts / "tool.py"
    script.write_text("from ._state import next_value\ndef main(): return next_value()\n", encoding="utf-8")

    inner = _Inner()
    base = make_test_server(
        server=inner,
        dcc_name="test-cleanup",
        _builtin_skills_dir=tmp_path,
        _handle=None,
        _enable_gateway_failover=False,
        _strict_gateway=False,
        _hot_reloader=None,
        _gateway_election=None,
        _gateway_guardian=None,
        _config=SimpleNamespace(
            sandbox_policy=None,
            gateway_port=0,
            registry_dir="",
            server_version="test",
            instance_metadata={},
        ),
        _enable_telemetry=False,
        _enable_file_logging=False,
        _enable_job_persistence=False,
        _execution_bridge=None,
        _dcc_dispatcher=None,
        _inprocess_executor_registered=False,
        _standalone_main_thread=False,
        _quit_hooks=[],
    )
    bridge = HostExecutionBridge()
    base.register_host_execution_bridge(bridge)

    base.start(install_atexit_hook=False)
    assert bridge.execute_script(str(script), {}) == 1
    inner.bus.emit_unloaded(skill_dir)
    assert bridge.execute_script(str(script), {}) == 1
    base.stop()

    base.start(install_atexit_hook=False)
    assert bridge.execute_script(str(script), {}) == 1
    base.stop()
    assert bridge.execute_script(str(script), {})["success"] is False


def test_dcc_server_base_constructor_registers_dispatcher_before_discovery(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    # Import local modules
    from dcc_mcp_core._server.options import DccServerOptions
    from dcc_mcp_core.server_base import DccServerBase

    events: list[str] = []

    class _Server:
        def set_in_process_executor(self, executor: Callable[..., Any]) -> None:
            events.append("set_in_process_executor")
            self.executor = executor

        def discover(self, extra_paths: list[str]) -> int:
            events.append("discover")
            return len(extra_paths)

    fake_server = _Server()
    import dcc_mcp_core.server_base as server_base

    monkeypatch.setattr(server_base, "create_adapter_server", lambda *_args, **_kwargs: fake_server)

    class _Dispatcher:
        def dispatch_callable(self, func: Callable[..., Any], *args: Any, **kwargs: Any) -> Any:
            return func(*args, **kwargs)

    opts = DccServerOptions.from_env(
        "test_inproc_ctor",
        tmp_path,
        port=0,
        dispatcher=_Dispatcher(),
        enable_file_logging=False,
        enable_job_persistence=False,
        enable_telemetry=False,
    )
    base = DccServerBase(opts)
    base.register_builtin_actions(include_bundled=False)

    assert events == ["set_in_process_executor", "discover"]


def test_dcc_server_base_constructor_registers_execution_bridge_before_discovery(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    # Import local modules
    from dcc_mcp_core._server.options import DccServerOptions
    from dcc_mcp_core.server_base import DccServerBase

    events: list[str] = []

    class _Server:
        def set_in_process_executor(self, executor: Callable[..., Any]) -> None:
            events.append("set_in_process_executor")
            self.executor = executor

        def discover(self, extra_paths: list[str]) -> int:
            events.append("discover")
            return len(extra_paths)

    fake_server = _Server()
    import dcc_mcp_core.server_base as server_base

    monkeypatch.setattr(server_base, "create_adapter_server", lambda *_args, **_kwargs: fake_server)

    bridge = HostExecutionBridge()
    opts = DccServerOptions.from_env(
        "test_host_bridge_ctor",
        tmp_path,
        port=0,
        execution_bridge=bridge,
        enable_file_logging=False,
        enable_job_persistence=False,
        enable_telemetry=False,
    )
    base = DccServerBase(opts)
    base.register_builtin_actions(include_bundled=False)

    assert events == ["set_in_process_executor", "discover"]


def test_dcc_server_base_standalone_main_thread_registers_inline_executor_before_discovery(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    from dcc_mcp_core._server.options import DccServerOptions
    from dcc_mcp_core.server_base import DccServerBase

    events: list[str] = []

    class _Server:
        def set_in_process_executor(self, executor: Callable[..., Any]) -> None:
            events.append("set_in_process_executor")
            self.executor = executor

        def discover(self, extra_paths: list[str]) -> int:
            events.append("discover")
            return len(extra_paths)

    fake_server = _Server()
    import dcc_mcp_core.server_base as server_base

    monkeypatch.setattr(server_base, "create_adapter_server", lambda *_args, **_kwargs: fake_server)

    opts = DccServerOptions.from_env(
        "test_standalone_ctor",
        tmp_path,
        port=0,
        standalone_main_thread=True,
        enable_file_logging=False,
        enable_job_persistence=False,
        enable_telemetry=False,
    )
    base = DccServerBase(opts)
    base.register_builtin_actions(include_bundled=False)

    assert events == ["set_in_process_executor", "discover"]
    assert base._standalone_main_thread is True


def test_dcc_server_base_execution_bridge_attaches_queue_dispatcher_before_discovery(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    from dcc_mcp_core._server.options import DccServerOptions
    from dcc_mcp_core.host import QueueDispatcher
    from dcc_mcp_core.server_base import DccServerBase

    events: list[str] = []

    class _Server:
        def set_in_process_executor(self, executor: Callable[..., Any]) -> None:
            events.append("set_in_process_executor")
            self.executor = executor

        def attach_dispatcher(self, dispatcher: Any) -> None:
            events.append("attach_dispatcher")
            self.dispatcher = dispatcher

        def discover(self, extra_paths: list[str]) -> int:
            events.append("discover")
            return len(extra_paths)

    fake_server = _Server()
    import dcc_mcp_core.server_base as server_base

    monkeypatch.setattr(server_base, "create_adapter_server", lambda *_args, **_kwargs: fake_server)

    dispatcher = QueueDispatcher()
    bridge = HostExecutionBridge(dispatcher=dispatcher)
    opts = DccServerOptions.from_env(
        "test_host_bridge_queue_ctor",
        tmp_path,
        port=0,
        execution_bridge=bridge,
        enable_file_logging=False,
        enable_job_persistence=False,
        enable_telemetry=False,
    )
    base = DccServerBase(opts)
    base.register_builtin_actions(include_bundled=False)

    assert events == ["set_in_process_executor", "attach_dispatcher", "discover"]
    assert fake_server.dispatcher is dispatcher
