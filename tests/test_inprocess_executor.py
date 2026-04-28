"""Tests for the in-process Python skill executor (issue #521)."""

# Import built-in modules
from __future__ import annotations

from pathlib import Path
from typing import Any
from typing import Callable
from typing import Mapping

# Import third-party modules
import pytest

# Import local modules
import dcc_mcp_core
from dcc_mcp_core._server.inprocess_executor import BaseDccCallableDispatcher
from dcc_mcp_core._server.inprocess_executor import build_inprocess_executor
from dcc_mcp_core._server.inprocess_executor import run_skill_script

# ── public surface ───────────────────────────────────────────────────────────


def test_base_dispatcher_exported_from_top_level() -> None:
    assert hasattr(dcc_mcp_core, "BaseDccCallableDispatcher")
    assert "BaseDccCallableDispatcher" in dcc_mcp_core.__all__


def test_helpers_exported_from_underscore_server() -> None:
    # Import local modules
    from dcc_mcp_core._server import BaseDccCallableDispatcher as B
    from dcc_mcp_core._server import build_inprocess_executor as BIE
    from dcc_mcp_core._server import run_skill_script as RSS

    assert B is BaseDccCallableDispatcher
    assert BIE is build_inprocess_executor
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
        "import sys\n"
        "__mcp_result__ = {'ok': True, 'frames': 12}\n"
        "def main(**_):\n"
        "    sys.exit(0)\n",
    )
    assert run_skill_script(str(p), {}) == {"ok": True, "frames": 12}


def test_run_skill_script_systemexit_at_module_level(tmp_path: Path) -> None:
    p = _write_script(
        tmp_path,
        "__mcp_result__ = {'fast_path': True}\n"
        "raise SystemExit(0)\n",
    )
    assert run_skill_script(str(p), {}) == {"fast_path": True}


def test_run_skill_script_does_not_pollute_sys_modules(tmp_path: Path) -> None:
    # Import built-in modules
    import sys

    before = {k for k in sys.modules if k.startswith("_dcc_mcp_inproc_")}
    p = _write_script(tmp_path, "def main(): return 'ok'\n")
    run_skill_script(str(p), {})
    after = {k for k in sys.modules if k.startswith("_dcc_mcp_inproc_")}
    assert after == before, "synthetic module name leaked into sys.modules"


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
    assert func is run_skill_script
    assert args == (str(p), {"x": 41})
    assert kwargs == {}


def test_executor_propagates_dispatcher_exceptions(tmp_path: Path) -> None:
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
    with pytest.raises(RuntimeError, match="UI thread shutdown"):
        executor(str(p), {})


def test_executor_uses_custom_runner() -> None:
    seen: list[tuple[str, Mapping[str, Any]]] = []

    def _fake_runner(script_path: str, params: Mapping[str, Any]) -> str:
        seen.append((script_path, params))
        return f"{script_path}|{dict(params)}"

    executor = build_inprocess_executor(None, runner=_fake_runner)
    out = executor("/tmp/skill.py", {"k": "v"})
    assert seen == [("/tmp/skill.py", {"k": "v"})]
    assert out == "/tmp/skill.py|{'k': 'v'}"


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


def test_register_inprocess_executor_calls_underlying_setter() -> None:
    # Import local modules
    from dcc_mcp_core import McpHttpConfig
    from dcc_mcp_core.server_base import DccServerBase

    base = DccServerBase("test_inproc_a", McpHttpConfig(port=0))
    captured: list[Callable[..., Any]] = []
    _patch_set_in_process_executor(base, captured)

    base.register_inprocess_executor()
    assert len(captured) == 1
    assert callable(captured[0])


def test_register_inprocess_executor_with_dispatcher_routes(tmp_path: Path) -> None:
    # Import local modules
    from dcc_mcp_core import McpHttpConfig
    from dcc_mcp_core.server_base import DccServerBase

    base = DccServerBase("test_inproc_b", McpHttpConfig(port=0))
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
