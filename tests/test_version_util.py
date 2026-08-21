"""Tests for shared Python version helpers."""

import types

import dcc_mcp_core._version_util as version_util
from dcc_mcp_core._version_util import package_version
from dcc_mcp_core._version_util import parse_semver


def test_parse_semver_normalizes_supported_versions():
    assert parse_semver("0.18.15") == (0, 18, 15)
    assert parse_semver("v2.3") == (2, 3, 0)
    assert parse_semver("5") == (5, 0, 0)
    assert parse_semver("V1.2.3-rc1") == (1, 2, 3)
    assert parse_semver("1.2.3+studio.4") == (1, 2, 3)


def test_parse_semver_rejects_invalid_numeric_core():
    assert parse_semver("") is None
    assert parse_semver("release") is None
    assert parse_semver("1.two.3") is None
    assert parse_semver("1..3") is None


def test_package_version_prefers_loaded_core(monkeypatch):
    core = types.ModuleType("dcc_mcp_core._core")
    core.__version__ = "9.9.9-core"
    monkeypatch.setitem(version_util.sys.modules, "dcc_mcp_core._core", core)

    assert package_version(fallback="unknown") == "9.9.9-core"


def test_package_version_loads_core_only_when_allowed(monkeypatch):
    monkeypatch.delitem(version_util.sys.modules, "dcc_mcp_core._core", raising=False)
    core = types.SimpleNamespace(__version__="8.8.8-core")
    imported = []

    def import_module(name):
        imported.append(name)
        return core

    monkeypatch.setattr(version_util.importlib, "import_module", import_module)

    assert version_util._core_version(False) is None
    assert imported == []
    assert version_util._core_version(True) == "8.8.8-core"
    assert imported == ["dcc_mcp_core._core"]


def test_package_version_uses_distribution_metadata(monkeypatch):
    monkeypatch.setattr(version_util, "_core_version", lambda _load: None)
    metadata = types.SimpleNamespace(version=lambda _name: "0.19.7")
    monkeypatch.setattr(version_util.importlib, "import_module", lambda _name: metadata)

    assert package_version(fallback="unknown") == "0.19.7"


def test_package_version_preserves_explicit_fallback(monkeypatch):
    monkeypatch.setattr(version_util, "_core_version", lambda _load: None)

    def fail_version(_name):
        raise RuntimeError("no dist")

    metadata = types.SimpleNamespace(version=fail_version)
    monkeypatch.setattr(version_util.importlib, "import_module", lambda _name: metadata)

    assert package_version(fallback="0.0.0-dev") == "0.0.0-dev"
