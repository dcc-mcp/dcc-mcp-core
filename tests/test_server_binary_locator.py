from __future__ import annotations

import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
from types import SimpleNamespace

import pytest

_LOCATOR_SOURCE = (
    Path(__file__).resolve().parents[1] / "pkg" / "dcc-mcp-server-bin" / "python" / "dcc_mcp_server" / "__init__.py"
)


def _load_locator_module():
    spec = importlib.util.spec_from_file_location("_dcc_mcp_server_locator_under_test", _LOCATOR_SOURCE)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _binary(directory, name):
    path = directory / name
    directory.mkdir(parents=True, exist_ok=True)
    path.write_bytes(b"binary fixture")
    path.chmod(0o755)
    return path


@pytest.fixture(params=["nt", "posix"])
def locator(request, monkeypatch, tmp_path):
    """Isolate every discovery source without changing the host's os.name."""
    module = _load_locator_module()
    module.os = SimpleNamespace(name=request.param, environ=os.environ)
    module.sys = SimpleNamespace(prefix=str(tmp_path / "venv"), platform="win32" if request.param == "nt" else "linux")
    module._BINARY_NAME = "dcc-mcp-server.exe" if request.param == "nt" else "dcc-mcp-server"
    module.__file__ = str(tmp_path / "site-packages" / "dcc_mcp_server" / "__init__.py")
    module.shutil = SimpleNamespace(which=lambda name: None)
    scheme = "nt_user" if request.param == "nt" else "posix_user"
    paths = {None: str(tmp_path / "scheme-scripts"), scheme: str(tmp_path / "user-scripts")}
    module.sysconfig = SimpleNamespace(
        get_path=lambda name, scheme=None: paths[scheme],
        get_preferred_scheme=lambda key: scheme,
        get_config_var=lambda name: None,
    )
    monkeypatch.delenv("DCC_MCP_SERVER_BIN", raising=False)
    return module


def _candidates(locator, tmp_path):
    return [
        tmp_path / "override" / locator._BINARY_NAME,
        Path(locator.__file__).parent.parent / "scripts" / locator._BINARY_NAME,
        Path(locator.sys.prefix) / ("Scripts" if locator.os.name == "nt" else "bin") / locator._BINARY_NAME,
        tmp_path / "scheme-scripts" / locator._BINARY_NAME,
        tmp_path / "user-scripts" / locator._BINARY_NAME,
        tmp_path / "path" / locator._BINARY_NAME,
    ]


@pytest.mark.parametrize("first_available", range(6))
def test_binary_path_preserves_discovery_precedence(locator, monkeypatch, tmp_path, first_available):
    """The venv and explicit override continue to beat a stale user/PATH copy."""
    candidates = _candidates(locator, tmp_path)
    for path in candidates[first_available:]:
        _binary(path.parent, path.name)
    monkeypatch.setenv("DCC_MCP_SERVER_BIN", str(candidates[0]))
    locator.shutil.which = lambda name: str(candidates[-1])

    assert locator.binary_path() == candidates[first_available]


@pytest.mark.parametrize("legacy", [False, True])
def test_binary_path_finds_user_install_without_path(locator, tmp_path, legacy):
    if legacy:
        del locator.sysconfig.get_preferred_scheme
    expected = _binary(tmp_path / "user-scripts", locator._BINARY_NAME)

    assert locator.binary_path() == expected


@pytest.mark.parametrize("legacy", [False, True])
def test_binary_path_uses_macos_framework_user_scheme(locator, tmp_path, legacy):
    locator.os.name = "posix"
    locator.sys.platform = "darwin"
    locator._BINARY_NAME = "dcc-mcp-server"
    locator.sysconfig.get_config_var = lambda name: "Python" if name == "PYTHONFRAMEWORK" else None
    if legacy:
        del locator.sysconfig.get_preferred_scheme
    else:
        locator.sysconfig.get_preferred_scheme = lambda key: "osx_framework_user"
    paths = {None: None, "osx_framework_user": str(tmp_path / "framework-user" / "bin")}
    locator.sysconfig.get_path = lambda name, scheme=None: paths[scheme]
    expected = _binary(tmp_path / "framework-user" / "bin", locator._BINARY_NAME)

    assert locator.binary_path() == expected


def test_binary_path_honors_custom_preferred_user_scheme(locator, tmp_path):
    locator.sysconfig.get_preferred_scheme = lambda key: "vendor_user"
    paths = {None: None, "vendor_user": str(tmp_path / "vendor-user")}
    locator.sysconfig.get_path = lambda name, scheme=None: paths[scheme]
    expected = _binary(tmp_path / "vendor-user", locator._BINARY_NAME)

    assert locator.binary_path() == expected


@pytest.mark.parametrize(
    "missing", [None, "", "directory", AttributeError, ImportError, ModuleNotFoundError, KeyError, OSError, ValueError]
)
def test_binary_path_falls_back_when_scheme_paths_are_unavailable(locator, monkeypatch, tmp_path, missing):
    monkeypatch.chdir(tmp_path)
    # Empty/absent paths must not accidentally select a CWD executable.
    _binary(tmp_path, locator._BINARY_NAME)
    fallback = _binary(tmp_path / "path", locator._BINARY_NAME)
    locator.shutil.which = lambda name: str(fallback)
    if missing == "directory":
        for candidate in _candidates(locator, tmp_path)[1:5]:
            candidate.mkdir(parents=True)
    elif missing in (AttributeError, ImportError, ModuleNotFoundError, KeyError, OSError, ValueError):

        def get_path(name, scheme=None):
            raise missing("unavailable interpreter configuration")

        locator.sysconfig.get_path = get_path
    else:
        locator.sysconfig.get_path = lambda name, scheme=None: missing

    assert locator.binary_path() == fallback


@pytest.mark.parametrize("error", [KeyError, ValueError])
def test_binary_path_ignores_unavailable_preferred_user_scheme(locator, tmp_path, error):
    def preferred(key):
        raise error(key)

    locator.sysconfig.get_preferred_scheme = preferred
    fallback = _binary(tmp_path / "path", locator._BINARY_NAME)
    locator.shutil.which = lambda name: str(fallback)

    assert locator.binary_path() == fallback


def test_binary_path_rejects_directories_including_override(locator, monkeypatch, tmp_path):
    candidates = _candidates(locator, tmp_path)
    for candidate in candidates[:-1]:
        candidate.mkdir(parents=True)
    monkeypatch.setenv("DCC_MCP_SERVER_BIN", str(candidates[0]))

    with pytest.raises(FileNotFoundError, match="DCC_MCP_SERVER_BIN"):
        locator.binary_path()


def test_binary_path_preserves_not_found_error(locator):
    with pytest.raises(FileNotFoundError, match="Did you `pip install dcc-mcp-server`"):
        locator.binary_path()


@pytest.mark.skipif(os.name == "nt", reason="POSIX execute permission bits")
def test_binary_path_keeps_path_executable_checks(locator, monkeypatch, tmp_path):
    locator._BINARY_NAME = "dcc-mcp-server"
    path_binary = _binary(tmp_path / "path", locator._BINARY_NAME)
    path_binary.chmod(0o644)
    locator.shutil = shutil
    monkeypatch.setenv("PATH", str(path_binary.parent))

    with pytest.raises(FileNotFoundError):
        locator.binary_path()
    path_binary.chmod(0o755)
    assert locator.binary_path() == path_binary


def test_binary_path_respects_pythonuserbase_in_fresh_interpreter(tmp_path):
    """Exercise the real interpreter's scheme expansion, including its cache."""
    source = tmp_path / "site-packages" / "dcc_mcp_server" / "__init__.py"
    source.parent.mkdir(parents=True)
    shutil.copyfile(str(_LOCATOR_SOURCE), str(source))
    env = os.environ.copy()
    env.pop("DCC_MCP_SERVER_BIN", None)
    env["PYTHONUSERBASE"] = str(tmp_path / "custom user base")
    env["PYTHONPATH"] = str(source.parent.parent)
    env["PATH"] = ""
    script = """
import json
from pathlib import Path
import sys
import sysconfig
from types import SimpleNamespace
import dcc_mcp_server as locator

scheme = locator._user_scheme()
expected = Path(sysconfig.get_path('scripts', scheme=scheme)) / locator._BINARY_NAME
expected.parent.mkdir(parents=True, exist_ok=True)
expected.write_bytes(b'user binary')
expected.chmod(0o755)
# Isolate this fixture from an unrelated installation in the test runner's prefix.
locator.sys = SimpleNamespace(prefix=str(expected.parent / 'absent-prefix'), platform=sys.platform)
original_get_path = sysconfig.get_path
sysconfig.get_path = lambda name, scheme=None: None if name == 'scripts' and scheme is None else original_get_path(name, scheme=scheme)
print(json.dumps({'actual': str(locator.binary_path()), 'expected': str(expected)}))
"""
    completed = subprocess.run(
        [sys.executable, "-c", script], env=env, cwd=str(tmp_path), check=True, stdout=subprocess.PIPE, text=True
    )
    result = json.loads(completed.stdout)
    assert result["actual"] == result["expected"]
    assert str(tmp_path / "custom user base") in result["actual"]


@pytest.mark.skipif(os.name == "nt", reason="POSIX sysconfig data module")
def test_binary_path_keeps_path_fallback_with_missing_sysconfig_data(tmp_path):
    """A failed sysconfig import can also leave its cached variables incomplete."""
    source = tmp_path / "site-packages" / "dcc_mcp_server" / "__init__.py"
    source.parent.mkdir(parents=True)
    shutil.copyfile(str(_LOCATOR_SOURCE), str(source))
    expected = _binary(tmp_path / "path", "dcc-mcp-server")
    env = os.environ.copy()
    env.pop("DCC_MCP_SERVER_BIN", None)
    env["PYTHONPATH"] = str(source.parent.parent)
    env["PATH"] = str(expected.parent)
    env["_PYTHON_SYSCONFIGDATA_NAME"] = "_dcc_mcp_nonexistent_sysconfigdata"
    script = """
import sys
from types import SimpleNamespace
import dcc_mcp_server as locator
locator.sys = SimpleNamespace(prefix=sys.argv[1], platform=sys.platform)
print(locator.binary_path())
"""
    completed = subprocess.run(
        [sys.executable, "-c", script, str(tmp_path / "absent-prefix")],
        env=env,
        cwd=str(tmp_path),
        check=True,
        stdout=subprocess.PIPE,
        text=True,
    )
    assert completed.stdout.strip() == str(expected)
