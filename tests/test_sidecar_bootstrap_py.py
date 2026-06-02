"""End-to-end Python test for the sidecar bootstrap payload.

The Rust :func:`CommandPortClient::send_bootstrap` ships this file's
source over Maya's commandPort at connect time (see
``crates/dcc-mcp-host-rpc/python/maya_sidecar_bootstrap.py``). The
Rust unit tests verify the *wire framing* (correct ``exec(compile(…))``
wrapping, string-literal escape, traceback surfacing) but cannot
verify the *Python semantics* of the embedded source itself —
typos in attribute access, accidental side effects on the parent
module, version-marker drift, etc.

This test exercises the embedded source in a real Python interpreter
against a synthetic ``dcc_mcp_maya`` namespace (no Maya required) so
any regression in the bootstrap body fails the dcc-mcp-core CI suite.

We deliberately do **not** import ``dcc_mcp_maya`` from PyPI here —
the bootstrap is meant to be testable against any Python environment
that has a ``dcc_mcp_maya.sidecar._dispatcher`` module on
``sys.modules``, however that arrived.
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
import ast
import json
from pathlib import Path
import sys
import types
from typing import Iterator

# Import third-party modules
import pytest

BOOTSTRAP_PATH = Path(__file__).parent.parent / "crates" / "dcc-mcp-host-rpc" / "python" / "maya_sidecar_bootstrap.py"


def _read_bootstrap_source() -> str:
    assert BOOTSTRAP_PATH.is_file(), (
        f"bootstrap source missing at {BOOTSTRAP_PATH}; the Rust binary include_str!s this exact file at build time."
    )
    return BOOTSTRAP_PATH.read_text(encoding="utf-8")


def _exec_bootstrap(globals_dict: dict | None = None) -> dict:
    """Compile + exec the bootstrap source in a fresh module-style namespace.

    Mirrors how Maya's commandPort eval-path runs the source under the
    wrapping ``exec(compile(<src>, '<dcc-mcp-sidecar-bootstrap>', 'exec'))``
    that ``send_bootstrap`` ships.
    """
    src = _read_bootstrap_source()
    namespace = globals_dict if globals_dict is not None else {}
    compiled = compile(src, "<dcc-mcp-sidecar-bootstrap>", "exec")
    exec(compiled, namespace)
    return namespace


# ── fixtures ──────────────────────────────────────────────────────


@pytest.fixture
def fresh_sys_modules(monkeypatch: pytest.MonkeyPatch) -> Iterator[None]:
    """Sandbox ``sys.modules`` so each test starts from a known state.

    We snapshot the keys we care about, replace them with our stubs,
    and let monkeypatch restore everything on teardown.
    """
    keys_we_touch = [
        "dcc_mcp_maya",
        "dcc_mcp_maya.sidecar",
        "dcc_mcp_maya.sidecar._dispatcher",
        "dcc_mcp_maya._sidecar",
    ]
    for key in keys_we_touch:
        monkeypatch.delitem(sys.modules, key, raising=False)
    yield


def _install_fake_dispatcher() -> tuple[types.ModuleType, callable, callable]:
    """Plant a synthetic ``dcc_mcp_maya.sidecar._dispatcher`` in sys.modules.

    Returns the parent ``dcc_mcp_maya`` module along with the two
    callables the bootstrap expects to re-export.
    """

    def _fake_dispatch(payload):
        return f"dispatched:{payload}"

    def _fake_dispatch_payload(payload, **kwargs):
        return f"dispatch_payload:{payload}"

    parent = types.ModuleType("dcc_mcp_maya")
    parent.__path__ = []
    sub = types.ModuleType("dcc_mcp_maya.sidecar")
    sub.__path__ = []
    dispatcher = types.ModuleType("dcc_mcp_maya.sidecar._dispatcher")
    dispatcher.dispatch = _fake_dispatch
    dispatcher.dispatch_payload = _fake_dispatch_payload

    sys.modules["dcc_mcp_maya"] = parent
    sys.modules["dcc_mcp_maya.sidecar"] = sub
    sys.modules["dcc_mcp_maya.sidecar._dispatcher"] = dispatcher
    parent.sidecar = sub
    sub._dispatcher = dispatcher
    return parent, _fake_dispatch, _fake_dispatch_payload


# ── tests ─────────────────────────────────────────────────────────


class TestBootstrapHappyPath:
    def test_installs_virtual_module(self, fresh_sys_modules):
        parent, dispatch_fn, dispatch_payload_fn = _install_fake_dispatcher()
        _exec_bootstrap()

        assert "dcc_mcp_maya._sidecar" in sys.modules, "bootstrap must register the virtual module in sys.modules"
        installed = sys.modules["dcc_mcp_maya._sidecar"]
        # Wire-frame attribute access — this is exactly what the Rust
        # client does after connect:
        #   __import__('dcc_mcp_maya._sidecar', fromlist=['dispatch']).dispatch(...)
        assert installed.dispatch is dispatch_fn
        assert installed.dispatch_payload is dispatch_payload_fn
        # Parent module gains the leaf attribute so dotted access works:
        #   dcc_mcp_maya._sidecar.dispatch(...)
        assert parent._sidecar is installed

    def test_marks_module_with_bootstrap_version(self, fresh_sys_modules):
        _install_fake_dispatcher()
        _exec_bootstrap()
        installed = sys.modules["dcc_mcp_maya._sidecar"]
        version = getattr(installed, "__dcc_mcp_bootstrap__", None)
        assert version is not None, (
            "bootstrap must stamp the virtual module so reentrant connects can detect the existing install"
        )
        # The literal version is pinned in the source. If the bootstrap
        # bumps it, this test fails loudly so the Rust side picks up
        # the new value too.
        assert version == "1"

    def test_installed_module_has_synthetic_file_marker(self, fresh_sys_modules):
        _install_fake_dispatcher()
        _exec_bootstrap()
        installed = sys.modules["dcc_mcp_maya._sidecar"]
        assert installed.__file__ == "<dcc-mcp-sidecar-bootstrap>"


class TestBootstrapIdempotence:
    def test_second_install_at_same_version_is_no_op(self, fresh_sys_modules):
        _install_fake_dispatcher()

        _exec_bootstrap()
        installed_first = sys.modules["dcc_mcp_maya._sidecar"]

        # Re-running the bootstrap (as happens on every connect) must
        # NOT replace the module — the existing one is returned in place.
        _exec_bootstrap()
        installed_second = sys.modules["dcc_mcp_maya._sidecar"]
        assert installed_first is installed_second

    def test_second_install_at_newer_version_replaces(self, fresh_sys_modules):
        _install_fake_dispatcher()

        # Install a stale-looking module under the canonical name to
        # mimic an older bootstrap version having run earlier.
        stale = types.ModuleType("dcc_mcp_maya._sidecar")
        stale.__dcc_mcp_bootstrap__ = "0-OLD"
        stale.dispatch = lambda p: "stale"
        sys.modules["dcc_mcp_maya._sidecar"] = stale

        _exec_bootstrap()
        installed = sys.modules["dcc_mcp_maya._sidecar"]
        # Should be the fresh install, not the stale one.
        assert installed is not stale
        assert installed.__dcc_mcp_bootstrap__ == "1"


class TestBootstrapNoOpPaths:
    def test_silent_when_parent_not_installed(self, fresh_sys_modules, monkeypatch: pytest.MonkeyPatch):
        # No fake dispatcher installed AND we actively block the
        # ``dcc_mcp_maya`` import from resolving via any sibling repo
        # the dev venv might have on ``sys.path``. ``None`` in
        # ``sys.modules`` is Python's "tried and failed" marker —
        # subsequent ``import dcc_mcp_maya`` raises ``ImportError``
        # without re-searching the path.
        for name in (
            "dcc_mcp_maya",
            "dcc_mcp_maya.sidecar",
            "dcc_mcp_maya.sidecar._dispatcher",
        ):
            monkeypatch.setitem(sys.modules, name, None)

        # Bootstrap must NOT raise; it just no-ops.
        _exec_bootstrap()
        assert "dcc_mcp_maya._sidecar" not in sys.modules

    def test_installs_fallback_when_dispatcher_module_missing(self, fresh_sys_modules):
        # Plant the parent + sub, but leave the dispatcher module out
        # of sys.modules. This emulates a partial install / refactor.
        parent = types.ModuleType("dcc_mcp_maya")
        parent.__path__ = []
        sub = types.ModuleType("dcc_mcp_maya.sidecar")
        sub.__path__ = []
        sys.modules["dcc_mcp_maya"] = parent
        sys.modules["dcc_mcp_maya.sidecar"] = sub
        parent.sidecar = sub
        # NB: no `_dispatcher` registered.

        _exec_bootstrap()
        installed = sys.modules["dcc_mcp_maya._sidecar"]
        envelope = json.loads(
            installed.dispatch(
                {
                    "action": "maya_model__create_cube",
                    "args": {"size": 1},
                    "request_id": "req-1",
                }
            )
        )

        assert parent._sidecar is installed
        assert envelope["success"] is False
        assert envelope["error"] == "sidecar-dispatcher-unavailable"
        assert envelope["context"]["kind"] == "sidecar_dispatcher_unavailable"
        assert envelope["context"]["action"] == "maya_model__create_cube"
        assert envelope["context"]["request_id"] == "req-1"

    def test_fallback_dispatch_payload_returns_dict(self, fresh_sys_modules):
        parent = types.ModuleType("dcc_mcp_maya")
        parent.__path__ = []
        sys.modules["dcc_mcp_maya"] = parent

        _exec_bootstrap()
        installed = sys.modules["dcc_mcp_maya._sidecar"]
        envelope = installed.dispatch_payload({"action": "maya_render__playblast"})

        assert envelope["success"] is False
        assert envelope["error"] == "sidecar-dispatcher-unavailable"
        assert envelope["context"]["action"] == "maya_render__playblast"


# ── pinned-source guard ───────────────────────────────────────────


def test_bootstrap_source_pins_known_contract():
    """If anyone refactors the dispatcher path away, this test fails
    loudly so the bootstrap (which is the wire-format authority) is
    updated in lockstep with the dcc-mcp-maya rename.
    """
    src = _read_bootstrap_source()
    assert "dcc_mcp_maya._sidecar" in src
    assert "dcc_mcp_maya.sidecar._dispatcher" in src
    assert "_BOOTSTRAP_VERSION" in src
    assert "types.ModuleType" in src
    assert "sys.modules" in src


def test_bootstrap_source_parses_under_python_3_7_feature_version():
    """Pin :file:`maya_sidecar_bootstrap.py` to **Python 3.7 syntax**.

    Maya 2020 and 2022 ship Python 3.7. The bootstrap is
    ``include_str!``-ed into the Rust binary and shipped verbatim
    over Maya's ``commandPort`` to the embedded interpreter — any
    3.8+ syntax (walrus ``:=``, ``match/case``, positional-only ``/``,
    f-string ``=`` debug, PEP 604 ``int | None`` at runtime, …) would
    raise ``SyntaxError`` inside Maya and break sidecar mode entirely.

    ``ast.parse(feature_version=(3, 7))`` rejects every grammar
    feature added after Python 3.7, so the test fails loudly the
    moment a contributor introduces a 3.8+ construct.
    """
    src = _read_bootstrap_source()
    try:
        ast.parse(src, filename=str(BOOTSTRAP_PATH), feature_version=(3, 7))
    except SyntaxError as exc:
        pytest.fail(
            f"{BOOTSTRAP_PATH} contains Python 3.8+ syntax that would break on Maya 2020/2022 (Python 3.7): {exc}"
        )
