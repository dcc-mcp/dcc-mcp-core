"""Regression coverage for host classes and dynamic namespaces."""

from __future__ import annotations

import importlib
import sys
from types import ModuleType
from types import SimpleNamespace
from unittest.mock import Mock

import pytest

from dcc_mcp_core.introspect import introspect_list_module
from dcc_mcp_core.introspect import introspect_search
from dcc_mcp_core.introspect import introspect_signature


@pytest.fixture
def extension_host(monkeypatch):
    """Expose a non-package module with a class, like an embedded SDK."""

    class BoneWeights:
        @staticmethod
        def set_vertex_bone_weights(mesh, vertex_id, weights):
            """Set weights for a single vertex."""
            raise AssertionError("Introspection must not invoke the method")

    module = ModuleType("sdk_host")
    module.GeometryScript_BoneWeights = BoneWeights
    monkeypatch.setitem(sys.modules, "sdk_host", module)
    return module


def test_extension_class_method_signature(extension_host):
    result = introspect_signature("sdk_host.GeometryScript_BoneWeights.set_vertex_bone_weights")
    assert result["success"] is True
    assert result["context"]["signature"] == "set_vertex_bone_weights(mesh, vertex_id, weights)"
    assert result["context"]["doc"] == "Set weights for a single vertex."


def test_class_namespace_list_and_search(extension_host):
    namespace = "sdk_host.GeometryScript_BoneWeights"
    listed = introspect_list_module(namespace)
    assert listed["success"] is True
    assert listed["context"]["names"] == ["set_vertex_bone_weights"]
    found = introspect_search("vertex", namespace)
    assert found["success"] is True
    assert found["context"]["hits"][0]["qualname"] == namespace + ".set_vertex_bone_weights"


def test_dynamic_nested_namespace(monkeypatch):
    module = ModuleType("operator_host")
    module.ops = SimpleNamespace(object=SimpleNamespace(join=lambda objects: objects))
    monkeypatch.setitem(sys.modules, "operator_host", module)
    result = introspect_signature("operator_host.ops.object.join")
    assert result["success"] is True
    assert result["context"]["signature"] == "join(objects)"


@pytest.mark.parametrize("name", ["builtins.str.upper", "len", "json.decoder.JSONDecoder.decode"])
def test_standard_library_and_builtin_methods(name):
    assert introspect_signature(name)["success"] is True


def test_longest_importable_module_wins(tmp_path, monkeypatch):
    package = tmp_path / "namespace_precedence"
    package.mkdir()
    (package / "__init__.py").write_text("child = object()\n", encoding="utf-8")
    (package / "child.py").write_text("def function(value):\n    return value\n", encoding="utf-8")
    monkeypatch.syspath_prepend(str(tmp_path))
    monkeypatch.delitem(sys.modules, "namespace_precedence", raising=False)
    monkeypatch.delitem(sys.modules, "namespace_precedence.child", raising=False)
    result = introspect_signature("namespace_precedence.child.function")
    assert result["success"] is True
    assert result["context"]["signature"] == "function(value)"


@pytest.mark.parametrize(
    "source,error",
    [
        ("import missing_sdk_dependency_xyz\n", "missing_sdk_dependency_xyz"),
        ("raise ImportError('SDK initialization failed')\n", "SDK initialization failed"),
        ("raise RuntimeError('SDK initialization failed')\n", "SDK initialization failed"),
    ],
)
def test_module_initialization_failures_do_not_fall_back(tmp_path, monkeypatch, source, error):
    package = tmp_path / "broken_sdk"
    package.mkdir()
    (package / "__init__.py").write_text("child = type('Fallback', (), {'function': len})\n", encoding="utf-8")
    (package / "child.py").write_text(source, encoding="utf-8")
    monkeypatch.syspath_prepend(str(tmp_path))
    monkeypatch.delitem(sys.modules, "broken_sdk", raising=False)
    monkeypatch.delitem(sys.modules, "broken_sdk.child", raising=False)
    result = introspect_signature("broken_sdk.child.function")
    assert result["success"] is False
    assert error in result["message"]


def test_missing_nested_attribute_reports_parent(extension_host):
    result = introspect_signature("sdk_host.GeometryScript_BoneWeights.missing")
    assert result["success"] is False
    assert "'missing' not found in 'sdk_host.GeometryScript_BoneWeights'" in result["message"]


@pytest.mark.parametrize("name", ["", "sdk_host..Class", "sdk_host.Class()", "sdk_host.Class[0]", "sdk_host;print(1)"])
def test_invalid_names_are_rejected_before_import(monkeypatch, name):
    importer = Mock(side_effect=AssertionError("Invalid names must not import"))
    monkeypatch.setattr(importlib, "import_module", importer)
    for result in (introspect_list_module(name), introspect_search(".*", name), introspect_signature(name)):
        assert result["success"] is False
    importer.assert_not_called()
