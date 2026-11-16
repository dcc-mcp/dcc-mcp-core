"""Native preflight distinguishes lazy imports from a usable native wheel."""

from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import sys
from types import SimpleNamespace

import pytest

from dcc_mcp_core.deployment import native_core as _native


@pytest.mark.parametrize("host", ["MotionBuilder2024", "Houdini20.5", "StudioHost"])
@pytest.mark.parametrize("failure", [ImportError("DLL load failed"), OSError(126, "Module not found")])
def test_preflight_reports_original_loader_failure_and_host(monkeypatch, tmp_path, host, failure):
    monkeypatch.setattr(_native.sys, "platform", "win32")
    monkeypatch.setattr(_native.sys, "executable", str(tmp_path / host / "host.exe"))

    def fail(name):
        assert name == "dcc_mcp_core._core"
        raise failure

    monkeypatch.setattr(_native.importlib, "import_module", fail)
    report = _native.verify_native_core()
    assert report["success"] is False
    assert report["reason"] == "native_load_failed"
    assert report["error"] == str(failure)
    assert host in report["executable"]
    assert report["dll_search_directories"]
    assert "python3.dll" in " ".join(report["next_steps"])
    assert "DCC_MCP_PYTHON3_DLL_DIR" in " ".join(report["next_steps"])
    json.dumps(report)
    with pytest.raises(ImportError, match="DCC_MCP_PYTHON3_DLL_DIR") as error:
        _native._load_native_core()
    assert error.value.__cause__ is failure


def test_missing_native_wheel_remains_distinct_from_missing_dependency(monkeypatch):
    def fail(name):
        raise ModuleNotFoundError("No native wheel", name=name)

    monkeypatch.setattr(_native.importlib, "import_module", fail)
    assert _native.verify_native_core()["reason"] == "native_extension_missing"
    from dcc_mcp_core._json_codec import json_dumps

    assert json.loads(json_dumps({"lite": True})) == {"lite": True}


def test_broken_native_codec_does_not_silently_fall_back(monkeypatch):
    def fail(name):
        raise ModuleNotFoundError("Missing runtime dependency", name="native_dependency")

    monkeypatch.setattr(_native.importlib, "import_module", fail)
    from dcc_mcp_core._json_codec import json_dumps

    assert _native.verify_native_core()["reason"] == "native_load_failed"
    with pytest.raises(ImportError, match="Missing runtime dependency"):
        json_dumps({"requires_native": True})


def test_preflight_retries_after_runtime_repair(monkeypatch):
    state = {"repaired": False}

    def load(name):
        if not state["repaired"]:
            raise ImportError("missing dependency")
        return SimpleNamespace(__version__="test-native")

    monkeypatch.setattr(_native.importlib, "import_module", load)
    assert _native.verify_native_core()["success"] is False
    state["repaired"] = True
    assert _native.verify_native_core()["native_version"] == "test-native"


def test_cached_native_calls_do_not_repeat_dll_directory_filesystem_probes(monkeypatch):
    core = SimpleNamespace(__version__="cached-native")
    monkeypatch.setitem(sys.modules, "dcc_mcp_core._core", core)

    def unexpected_probe():
        raise AssertionError("cached JSON/native calls must not probe the filesystem")

    monkeypatch.setattr(_native, "prepare_embedded_python_dll_search", unexpected_probe)
    assert _native._load_native_core() is core


def test_plain_import_is_light_but_native_server_access_has_actionable_failure():
    code = """
import importlib.abc
import json
import sys
class BrokenNative(importlib.abc.MetaPathFinder):
    def find_spec(self, fullname, *args):
        if fullname == 'dcc_mcp_core._core':
            raise ImportError('simulated embedded-host loader failure')
sys.meta_path.insert(0, BrokenNative())
import dcc_mcp_core
assert 'dcc_mcp_core._core' not in sys.modules
assert 'dcc_mcp_core.deployment.native_core' not in sys.modules
assert 'verify_native_core' in dir(dcc_mcp_core)
report = dcc_mcp_core.verify_native_core()
assert not report['success']
try:
    from dcc_mcp_core import DccServerBase
except ImportError as exc:
    assert 'simulated embedded-host loader failure' in str(exc)
    assert 'interpreter and architecture' in str(exc)
else:
    raise AssertionError('native server access must fail closed')
print(json.dumps(report))
"""
    env = dict(os.environ, PYTHONPATH=str(Path(__file__).resolve().parents[1] / "python"))
    result = subprocess.run([sys.executable, "-B", "-c", code], env=env, text=True, capture_output=True, timeout=20)
    assert result.returncode == 0, result.stderr
    assert json.loads(result.stdout)["reason"] == "native_load_failed"


@pytest.mark.parametrize("operation", ["resolve", "is_dir"])
def test_inaccessible_override_preserves_light_import_and_native_failure_report(operation):
    code = """
import importlib.abc
import json
import os
from pathlib import Path
import sys

inaccessible = Path.cwd() / 'inaccessible-native-runtime'
operation = sys.argv[1]
original = getattr(Path, operation)
def deny_candidate(path, *args, **kwargs):
    if path == inaccessible:
        raise PermissionError('runtime directory is inaccessible')
    return original(path, *args, **kwargs)
setattr(Path, operation, deny_candidate)
os.environ['DCC_MCP_PYTHON3_DLL_DIR'] = str(inaccessible)
os.add_dll_directory = lambda path: object()
sys.platform = 'win32'
class BrokenNative(importlib.abc.MetaPathFinder):
    def find_spec(self, fullname, *args):
        if fullname == 'dcc_mcp_core._core':
            raise ImportError('native runtime still unavailable')
sys.meta_path.insert(0, BrokenNative())
import dcc_mcp_core
assert 'dcc_mcp_core._core' not in sys.modules
report = dcc_mcp_core.verify_native_core()
assert report['error'] == 'native runtime still unavailable'
print(json.dumps(report))
"""
    env = dict(os.environ, PYTHONPATH=str(Path(__file__).resolve().parents[1] / "python"))
    result = subprocess.run(
        [sys.executable, "-B", "-c", code, operation], env=env, text=True, capture_output=True, timeout=20
    )
    assert result.returncode == 0, result.stderr
    assert json.loads(result.stdout)["reason"] == "native_load_failed"
