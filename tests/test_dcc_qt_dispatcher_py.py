"""Semantic tests for the universal Qt dispatcher Python source.

The Rust :mod:`dcc_mcp_host_rpc::qtserver` module embeds the canonical
dispatcher source directly via ``include_str!``:

* ``python/dcc_mcp_core/qt_dispatcher.py``
  — the single canonical source of the ``QtCommandServer`` +
    ``_DispatchRegistry`` implementation. Embedded by Rust at build time.
* ``crates/dcc-mcp-host-rpc/python/dcc_qt_dispatcher_bootstrap.py``
  — the installer that wraps the dispatcher source into
    ``sys.modules['_dcc_qt_dispatcher']`` at runtime.

The Rust unit tests in ``qtserver.rs`` cover **wire framing** (request
serialisation, envelope interpretation, host-died classification) and
the **bootstrap helpers** that build the commandPort eval lines. They
cannot verify the *Python semantics* of the embedded source itself
— typos in attribute access, drift between the dispatcher
methods and what callers send over the wire, etc.

This test exercises the canonical dispatcher source and the bootstrap
in a stock Python interpreter (no Qt required for the pure-Python parts;
PySide2/PySide6 optional for the server smoke test) so any regression
fails the dcc-mcp-core CI suite.
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

DISPATCHER_PATH = Path(__file__).parent.parent / "python" / "dcc_mcp_core" / "qt_dispatcher.py"
BOOTSTRAP_PATH = (
    Path(__file__).parent.parent / "crates" / "dcc-mcp-host-rpc" / "python" / "dcc_qt_dispatcher_bootstrap.py"
)


def _read(path: Path) -> str:
    assert path.is_file(), f"source missing at {path}; the Rust binary `include_str!`s this exact file at build time."
    return path.read_text(encoding="utf-8")


def test_canonical_dispatcher_source_exists() -> None:
    """The canonical dispatcher source must exist so the Rust crate's
    ``include_str!`` resolves at build time.  Any rename or deletion of
    ``python/dcc_mcp_core/qt_dispatcher.py`` would break the qtserver
    wire path and must fail this test.
    """
    assert DISPATCHER_PATH.is_file(), (
        f"Canonical dispatcher source missing at {DISPATCHER_PATH}. "
        "The Rust crate `dcc-mcp-host-rpc` embeds this file via "
        "`include_str!` in `qtserver.rs:DISPATCHER_PY`."
    )


@pytest.fixture
def dispatcher_module() -> Iterator[types.ModuleType]:
    """Exec ``dcc_mcp_core.qt_dispatcher`` into a fresh module-style namespace
    and yield it. The module mimics what the bootstrap installs under
    ``sys.modules['_dcc_qt_dispatcher']`` inside a real DCC.
    """
    source = _read(DISPATCHER_PATH)
    module = types.ModuleType("_dcc_qt_dispatcher")
    module.__file__ = str(DISPATCHER_PATH)
    exec(
        compile(source, module.__file__, "exec"),
        module.__dict__,
    )
    yield module


def test_dispatcher_exports_public_api(dispatcher_module: types.ModuleType) -> None:
    """The Rust `QtServerClient` and per-DCC plug-ins both rely on a
    stable public surface — pin it here so refactors of the source
    fail loudly instead of silently breaking the wire path.
    """
    for name in (
        "QtCommandServer",
        "ServerHandle",
        "start_qt_server",
        "stop_qt_server",
        "current_server",
        "DISPATCHER_VERSION",
    ):
        assert hasattr(dispatcher_module, name), f"public symbol missing: {name}"


def test_public_package_import_paths_expose_qt_dispatcher() -> None:
    from dcc_mcp_core.qt_dispatcher import ServerHandle
    from dcc_mcp_core.qt_dispatcher import start_qt_server

    assert ServerHandle.__name__ == "ServerHandle"
    assert callable(start_qt_server)


def test_dispatch_registry_ping(dispatcher_module: types.ModuleType) -> None:
    registry = dispatcher_module._DispatchRegistry()
    envelope = registry.dispatch("ping", {})
    assert envelope == {
        "result": {"pong": True, "version": dispatcher_module.DISPATCHER_VERSION},
    }


def test_dispatch_registry_unknown_method(dispatcher_module: types.ModuleType) -> None:
    registry = dispatcher_module._DispatchRegistry()
    envelope = registry.dispatch("does_not_exist", {})
    assert "error" in envelope
    assert envelope["error"]["code"] == "unknown-method"
    assert "does_not_exist" in envelope["error"]["message"]


def test_dispatch_registry_dispatch_handler_success_and_failure(
    dispatcher_module: types.ModuleType,
) -> None:
    def dispatch_handler(params):
        if params.get("action") == "boom":
            raise RuntimeError("dispatch failed")
        return {
            "action": params.get("action"),
            "args": params.get("args"),
            "request_id": params.get("request_id"),
        }

    registry = dispatcher_module._DispatchRegistry(dispatch_handler=dispatch_handler)
    envelope = registry.dispatch(
        "dispatch",
        {"action": "create", "args": {"radius": 1}, "request_id": "req-1"},
    )
    assert envelope == {
        "result": {
            "action": "create",
            "args": {"radius": 1},
            "request_id": "req-1",
        }
    }

    failed = registry.dispatch("dispatch", {"action": "boom"})
    assert failed["error"]["code"] == "handler-exception"
    assert "dispatch failed" in failed["error"]["message"]


def test_dispatch_registry_execute_returns_value(dispatcher_module: types.ModuleType) -> None:
    registry = dispatcher_module._DispatchRegistry()
    envelope = registry.dispatch("execute", {"code": "1 + 2"})
    assert envelope == {"result": {"value": 3, "result_type": "value"}}


def test_dispatch_registry_execute_mixed_body_and_expression(
    dispatcher_module: types.ModuleType,
) -> None:
    """Statements before the trailing expression run as side effects;
    the expression's value is returned.
    """
    registry = dispatcher_module._DispatchRegistry()
    envelope = registry.dispatch(
        "execute",
        {"code": "x = 10\ny = 20\nx + y"},
    )
    assert envelope == {"result": {"value": 30, "result_type": "value"}}


def test_dispatch_registry_execute_void_returns_none(
    dispatcher_module: types.ModuleType,
) -> None:
    """Code that ends in a statement (no trailing expression) returns
    a ``{"value": None, "result_type": "void"}`` envelope so the wire
    can never get stuck on a "no result" ambiguity.
    """
    registry = dispatcher_module._DispatchRegistry()
    envelope = registry.dispatch("execute", {"code": "import sys"})
    assert envelope == {"result": {"value": None, "result_type": "void"}}


def test_dispatch_registry_execute_repr_mode(dispatcher_module: types.ModuleType) -> None:
    """``result_type='repr'`` returns ``repr(value)`` so the caller can
    see objects that aren't JSON-serialisable.
    """
    registry = dispatcher_module._DispatchRegistry()
    envelope = registry.dispatch(
        "execute",
        {"code": "object()", "result_type": "repr"},
    )
    assert envelope["result"]["result_type"] == "repr"
    assert envelope["result"]["value"].startswith("<object object at ")


def test_dispatch_registry_execute_falls_back_to_repr_for_non_json(
    dispatcher_module: types.ModuleType,
) -> None:
    """``result_type='value'`` should never raise a serialisation
    error — non-JSON objects get repr'd transparently.
    """
    registry = dispatcher_module._DispatchRegistry()
    envelope = registry.dispatch("execute", {"code": "object()"})
    assert envelope["result"]["result_type"] == "value"
    assert isinstance(envelope["result"]["value"], str)


def test_dispatch_registry_execute_surfaces_exception(
    dispatcher_module: types.ModuleType,
) -> None:
    registry = dispatcher_module._DispatchRegistry()
    envelope = registry.dispatch("execute", {"code": "1/0"})
    assert "error" in envelope
    assert envelope["error"]["code"] == "handler-exception"
    assert "ZeroDivisionError" in envelope["error"]["message"]
    assert "Traceback" in envelope["error"]["traceback"]


def test_dispatch_registry_get_session_info(dispatcher_module: types.ModuleType) -> None:
    registry = dispatcher_module._DispatchRegistry()
    envelope = registry.dispatch("get_session_info", {})
    info = envelope["result"]
    assert info["dispatcher_version"] == dispatcher_module.DISPATCHER_VERSION
    assert isinstance(info["python_version"], str)
    assert isinstance(info["platform"], str)


def test_dispatch_registry_stream_capture_install_and_drain(
    dispatcher_module: types.ModuleType,
) -> None:
    registry = dispatcher_module._DispatchRegistry()
    try:
        installed = registry.dispatch("install_stream_capture", {})
        assert installed == {"result": {"installed": True}}
        # idempotent
        installed_again = registry.dispatch("install_stream_capture", {})
        assert installed_again == {"result": {"installed": False, "reused": True}}
        # Print something via the captured stdout (which is the tee)
        print("hello from captured stdout")
        drained = registry.dispatch("get_buffered_output", {})
        assert "hello from captured stdout" in drained["result"]["output"]
        # Drain default is True — second call returns empty unless
        # something else was printed between.
        drained_again = registry.dispatch("get_buffered_output", {})
        assert drained_again["result"]["output"] == ""
    finally:
        # Restore — leaving _Tee installed would poison subsequent
        # tests and pytest's own output.
        sys.stdout = registry._stdout_orig
        sys.stderr = registry._stderr_orig


def test_dispatch_registry_create_module_installs_into_sys_modules(
    dispatcher_module: types.ModuleType,
) -> None:
    registry = dispatcher_module._DispatchRegistry()
    name = "_dcc_qt_dispatcher_test_install"
    try:
        envelope = registry.dispatch(
            "create_module",
            {"name": name, "source": "value = 42\n", "version": "v1"},
        )
        assert envelope == {
            "result": {"installed": True, "name": name, "version": "v1"},
        }
        assert sys.modules[name].value == 42
        # Re-install at same version is a no-op.
        again = registry.dispatch(
            "create_module",
            {"name": name, "source": "value = 999\n", "version": "v1"},
        )
        assert again == {
            "result": {"installed": False, "reused": True, "name": name, "version": "v1"},
        }
        # Still the original value — the no-op didn't re-exec.
        assert sys.modules[name].value == 42
    finally:
        sys.modules.pop(name, None)


def test_dispatch_registry_create_module_validates_inputs(
    dispatcher_module: types.ModuleType,
) -> None:
    registry = dispatcher_module._DispatchRegistry()
    for bad_params, expected in (
        ({"name": "", "source": "x = 1"}, "non-empty string"),
        ({"name": "ok", "source": 123}, "must be a string"),
    ):
        envelope = registry.dispatch("create_module", bad_params)
        assert "error" in envelope, f"should reject {bad_params}"
        assert expected in envelope["error"]["message"], envelope["error"]["message"]


def test_tee_tolerates_broken_sink(dispatcher_module: types.ModuleType) -> None:
    """A closed/broken downstream sink must not propagate to the
    caller — the DCC's console may close at any time during a long
    session and the dispatcher must keep working.
    """

    class Broken:
        def write(self, _data):
            raise OSError("sink is gone")

        def flush(self):
            raise OSError("sink is gone")

    sink = dispatcher_module._Tee(Broken(), Broken())
    sink.write("hello")  # must not raise
    sink.flush()


def test_bootstrap_installs_dispatcher_from_source() -> None:
    """The bootstrap orchestrator must install
    ``sys.modules['_dcc_qt_dispatcher']`` from a string source and
    leave the module in a state where ``start_qt_server`` is
    callable (we don't actually start it here — that needs Qt).
    """
    dispatcher_source = _read(DISPATCHER_PATH)
    bootstrap_source = _read(BOOTSTRAP_PATH)

    namespace: dict = {
        "_DISPATCHER_SOURCE": dispatcher_source,
        "_REQUESTED_PORT": 0,
    }
    try:
        exec(
            compile(bootstrap_source, str(BOOTSTRAP_PATH), "exec"),
            namespace,
        )
        # The install_result should be the installed module itself,
        # not a failure dict.
        installed = namespace["_install_result"]
        assert isinstance(installed, types.ModuleType), f"expected module, got {installed!r}"
        assert installed.__name__ == "_dcc_qt_dispatcher"
        assert hasattr(installed, "start_qt_server")
        assert hasattr(installed, "stop_qt_server")
        assert sys.modules["_dcc_qt_dispatcher"] is installed
    finally:
        sys.modules.pop("_dcc_qt_dispatcher", None)


def test_bootstrap_idempotent_on_same_version() -> None:
    """A second exec of the bootstrap with the same version must
    reuse the already-installed module (no re-exec of the source).
    """
    dispatcher_source = _read(DISPATCHER_PATH)
    bootstrap_source = _read(BOOTSTRAP_PATH)

    try:
        # First install
        ns_a: dict = {
            "_DISPATCHER_SOURCE": dispatcher_source,
            "_REQUESTED_PORT": 0,
        }
        exec(compile(bootstrap_source, str(BOOTSTRAP_PATH), "exec"), ns_a)
        first = ns_a["_install_result"]
        # Mark the module so we can detect a re-exec
        first._dcc_qt_test_marker = "first-install"

        # Second install in a fresh namespace
        ns_b: dict = {
            "_DISPATCHER_SOURCE": dispatcher_source,
            "_REQUESTED_PORT": 0,
        }
        exec(compile(bootstrap_source, str(BOOTSTRAP_PATH), "exec"), ns_b)
        second = ns_b["_install_result"]
        # Same module object — and the marker survives, proving no
        # re-exec.
        assert second is first
        assert getattr(second, "_dcc_qt_test_marker", None) == "first-install"
    finally:
        sys.modules.pop("_dcc_qt_dispatcher", None)


def test_bootstrap_returns_failure_envelope_on_syntax_error() -> None:
    """If the dispatcher source has a syntax error, the bootstrap
    must NOT register the broken module in ``sys.modules`` and
    must surface a structured failure dict so the Rust client
    can map it to a transport error.
    """
    bootstrap_source = _read(BOOTSTRAP_PATH)
    broken_source = "def x(:::\n"  # syntactically invalid

    try:
        namespace: dict = {
            "_DISPATCHER_SOURCE": broken_source,
            "_REQUESTED_PORT": 0,
        }
        exec(compile(bootstrap_source, str(BOOTSTRAP_PATH), "exec"), namespace)
        result = namespace["_install_result"]
        assert isinstance(result, dict)
        assert result["ok"] is False
        assert result["stage"] == "compile"
        assert "SyntaxError" in result["error"]
        assert "_dcc_qt_dispatcher" not in sys.modules, "broken dispatcher must not pollute sys.modules"
    finally:
        sys.modules.pop("_dcc_qt_dispatcher", None)


@pytest.mark.skipif(
    sys.version_info < (3, 10),
    reason="Python 3.8/3.9 feature_version=(3, 7) does not reject walrus operator; fixed in 3.10+",
)
def test_python_3_7_guard_catches_walrus() -> None:
    """Self-test for the ``feature_version=(3, 7)`` guard.

    If a future CPython release silently weakens ``feature_version``
    (or pytest is run on an interpreter old enough to lack it), the
    guard tests below would silently pass without doing any work and
    a 3.8+ regression in the dispatcher source could slip through.
    This negative-path test feeds a known walrus operator and
    asserts ``SyntaxError`` is raised — if it ever stops raising,
    the rest of the 3.7 pinning machinery cannot be trusted and
    this test fails the build.
    """
    walrus_src = "(n := 1)\n"
    with pytest.raises(SyntaxError):
        ast.parse(walrus_src, feature_version=(3, 7))


@pytest.mark.parametrize(
    "path",
    [
        pytest.param(DISPATCHER_PATH, id="dispatcher"),
        pytest.param(BOOTSTRAP_PATH, id="bootstrap"),
    ],
)
def test_source_parses_under_python_3_7_feature_version(path: Path) -> None:
    """Pin the dispatcher and bootstrap to **Python 3.7 syntax**.

    Maya 2020/2022, Blender 2.83 LTS, and several legacy DCC hosts
    still ship Python 3.7. The dispatcher source is ``include_str!``-ed
    into the Rust binary and ships **verbatim** to those hosts — any
    3.8+ syntax (walrus ``:=``, ``match/case``, positional-only ``/``,
    f-string ``=`` debug, PEP 604 ``int | None`` at runtime, …) would
    raise ``SyntaxError`` inside the DCC and break the entire qtserver
    handoff.

    ``ast.parse(feature_version=(3, 7))`` rejects every grammar feature
    added after Python 3.7, so the test fails loudly the moment a
    contributor introduces a 3.8+ construct into either file.

    Note: this only catches **syntax-level** drift. Runtime usage of
    3.8+ stdlib APIs (e.g. ``ast.Module(type_ignores=...)``,
    ``functools.cache``) is covered by the dedicated semantic tests
    above — they exec the source against a real interpreter, so
    failures surface as ``AttributeError`` / ``TypeError`` rather than
    syntax errors. The two layers together prevent every flavour of
    3.7-incompat regression we've actually hit in production.
    """
    src = _read(path)
    try:
        ast.parse(src, filename=str(path), feature_version=(3, 7))
    except SyntaxError as exc:
        pytest.fail(
            f"{path} contains Python 3.8+ syntax that would break on Maya 2020/2022 "
            f"and Blender 2.83 LTS (Python 3.7): {exc}"
        )


def test_dispatcher_source_compiles_without_qt() -> None:
    """``dcc_mcp_core.qt_dispatcher`` must be importable up to the point of
    ``_import_qt`` being called. Calling :func:`start_qt_server` is
    what triggers the Qt import — module-load itself must work
    headless so CI without PySide passes.
    """
    source = _read(DISPATCHER_PATH)
    namespace: dict = {}
    exec(compile(source, str(DISPATCHER_PATH), "exec"), namespace)
    assert "QtCommandServer" in namespace
    assert "start_qt_server" in namespace
    assert namespace["_singleton"]["server"] is None


def test_dispatcher_source_is_valid_python_when_inlined_in_bootstrap_wire() -> None:
    """The Rust ``build_bootstrap_command_line`` inlines both files'
    sources into a single Python eval expression. Verify the
    composition is syntactically valid by reproducing the same
    composition here.
    """
    dispatcher_source = _read(DISPATCHER_PATH)
    bootstrap_source = _read(BOOTSTRAP_PATH)
    composed = "_DISPATCHER_SOURCE = " + repr(dispatcher_source) + "\n_REQUESTED_PORT = 0\n" + bootstrap_source
    # Compile must succeed — runtime semantics covered by the
    # dedicated bootstrap tests above.
    compile(composed, "<wire-bootstrap>", "exec")


def test_start_qt_server_with_fake_qt_handles_ping_dispatch_and_errors(
    dispatcher_module: types.ModuleType,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    class FakeSignal:
        def __init__(self):
            self.callbacks = []

        def connect(self, callback):
            self.callbacks.append(callback)

    class FakeTimer:
        def __init__(self):
            self.timeout = FakeSignal()
            self.started = False

        def start(self, _interval):
            self.started = True

        def stop(self):
            self.started = False

    class FakeTcpServer:
        def __init__(self):
            self.newConnection = FakeSignal()
            self.closed = False
            self._port = 0

        def listen(self, _address, port):
            self._port = port or 18765
            return True

        def errorString(self):
            return "fake listen failure"

        def serverPort(self):
            return self._port

        def hasPendingConnections(self):
            return False

        def close(self):
            self.closed = True

    class FakeQtCore:
        QTimer = FakeTimer

    class FakeQtNetwork:
        QTcpServer = FakeTcpServer
        QHostAddress = str

    class FakeSocket:
        def __init__(self):
            self.writes = []

        def write(self, data):
            self.writes.append(bytes(data))

        def flush(self):
            return None

    def response_from(socket):
        return json.loads(socket.writes[-1].decode("utf-8"))

    def dispatch_handler(params):
        if params.get("action") == "explode":
            raise RuntimeError("fake DCC failed")
        return {"ok": True, "payload": params}

    monkeypatch.setattr(
        dispatcher_module,
        "_import_qt",
        lambda: (FakeQtCore, FakeQtNetwork, "FakeQt"),
    )

    handle = dispatcher_module.start_qt_server(
        port=0,
        dispatch_handler=dispatch_handler,
        session_info_provider=lambda: {"dcc": "fake"},
    )
    try:
        assert handle.port == 18765
        assert handle.url == "qtserver://127.0.0.1:18765"
        assert handle["url"] == handle.url
        assert isinstance(json.dumps(handle), str)

        server = dispatcher_module.current_server()
        ping_socket = FakeSocket()
        server._handle_line(ping_socket, b'{"id":"p1","method":"ping","params":{}}')
        assert response_from(ping_socket)["result"]["pong"] is True

        dispatch_socket = FakeSocket()
        server._handle_line(
            dispatch_socket,
            b'{"id":"d1","method":"dispatch","params":{"action":"create","args":{"radius":1},"request_id":"req-1"}}',
        )
        assert response_from(dispatch_socket)["result"] == {
            "ok": True,
            "payload": {
                "action": "create",
                "args": {"radius": 1},
                "request_id": "req-1",
            },
        }

        error_socket = FakeSocket()
        server._handle_line(
            error_socket,
            b'{"id":"d2","method":"dispatch","params":{"action":"explode"}}',
        )
        error = response_from(error_socket)["error"]
        assert error["code"] == "handler-exception"
        assert "fake DCC failed" in error["message"]

        session_socket = FakeSocket()
        server._handle_line(session_socket, b'{"id":"s1","method":"get_session_info","params":{}}')
        assert response_from(session_socket)["result"]["dcc"] == "fake"
    finally:
        handle.close()


@pytest.mark.skipif(
    "PySide2" not in sys.modules
    and "PySide6" not in sys.modules
    and not any(
        __import__("importlib.util", fromlist=["find_spec"]).find_spec(name)
        for name in ("PySide6", "PySide2", "PyQt6", "PyQt5")
    ),
    reason="no Qt binding available — skip live QTcpServer smoke test",
)
def test_start_qt_server_returns_bound_port_and_is_idempotent(
    dispatcher_module: types.ModuleType,
) -> None:
    """End-to-end smoke against a real Qt binding when one is
    installed in the CI image. Verifies the server actually binds
    and ``start_qt_server`` is idempotent.
    """
    info = dispatcher_module.start_qt_server(port=0, host="127.0.0.1")
    try:
        assert info["host"] == "127.0.0.1"
        assert 1024 < info["port"] < 65536
        assert info["dispatcher_version"] == dispatcher_module.DISPATCHER_VERSION
        assert info["reused"] is False
        # Second call must reuse the same server.
        again = dispatcher_module.start_qt_server(port=0)
        assert again["port"] == info["port"]
        assert again["reused"] is True
    finally:
        dispatcher_module.stop_qt_server()
