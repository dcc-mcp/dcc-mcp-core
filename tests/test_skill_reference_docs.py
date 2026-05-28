"""Tests for skill_refs__* reference document tools."""

from __future__ import annotations

from pathlib import Path
import warnings

import pytest

from dcc_mcp_core.constants import METADATA_SKILL_REFERENCE_DOCS_KEY
from dcc_mcp_core.skill_reference_docs import _handle_list
from dcc_mcp_core.skill_reference_docs import _handle_read
from dcc_mcp_core.skill_reference_docs import register_skill_reference_docs_tools


class _Meta:
    def __init__(self, name: str, skill_path: str, **kwargs: object) -> None:
        self.name = name
        self.skill_path = skill_path
        self.metadata = dict(kwargs.get("metadata", {}))
        self.introspection_file = kwargs.get("introspection_file")


def test_list_default_references_glob(tmp_path: Path) -> None:
    skill_dir = tmp_path / "demo-skill"
    skill_dir.mkdir()
    (skill_dir / "references").mkdir(parents=True)
    (skill_dir / "references" / "NOTE.md").write_text("# hi\n", encoding="utf-8")
    deep = skill_dir / "references" / "deep"
    deep.mkdir(parents=True)
    (deep / "x.md").write_text("x", encoding="utf-8")

    md = _Meta("demo-skill", str(skill_dir))
    out = _handle_list({"demo-skill": md}, {"skill": "demo-skill"})
    assert out["success"] is True
    ctx = out["context"]
    paths = {f["path"] for f in ctx["files"]}
    assert "references/NOTE.md" in paths
    assert "references/deep/x.md" in paths


def test_list_custom_globs(tmp_path: Path) -> None:
    skill_dir = tmp_path / "g"
    skill_dir.mkdir()
    (skill_dir / "docs").mkdir()
    (skill_dir / "docs" / "a.txt").write_text("t", encoding="utf-8")
    md = _Meta(
        "g",
        str(skill_dir),
        metadata={METADATA_SKILL_REFERENCE_DOCS_KEY: ["docs/*.txt"]},
    )
    out = _handle_list({"g": md}, {"skill": "g"})
    assert out["success"] is True
    ctx = out["context"]
    assert any(f["path"] == "docs/a.txt" for f in ctx["files"])


def test_read_rejects_traversal(tmp_path: Path) -> None:
    skill_dir = tmp_path / "s"
    skill_dir.mkdir()
    (skill_dir / "references").mkdir()
    (skill_dir / "references" / "ok.md").write_text("ok", encoding="utf-8")
    md = _Meta("s", str(skill_dir))
    out = _handle_read({"s": md}, {"skill": "s", "path": "references/../../outside.md"})
    assert out["success"] is False


def test_read_roundtrip(tmp_path: Path) -> None:
    skill_dir = tmp_path / "s2"
    skill_dir.mkdir()
    (skill_dir / "references").mkdir()
    (skill_dir / "references" / "ok.md").write_text("body", encoding="utf-8")
    md = _Meta("s2", str(skill_dir))
    out = _handle_read({"s2": md}, {"skill": "s2", "path": "references/ok.md"})
    assert out["success"] is True
    assert out["context"]["content"] == "body"


def test_legacy_introspection_file_warns_and_still_surfaces(tmp_path: Path) -> None:
    """``metadata.dcc-mcp.introspection`` (pre-#616) must still expose the
    declared file via ``skill_refs__list`` but emit a ``DeprecationWarning``
    so authors migrate to ``skill-reference-docs``.

    Regression: deprecation must remain non-breaking for one release window;
    once removed, this test flips to assert the file is *not* listed.
    """
    skill_dir = tmp_path / "legacy"
    skill_dir.mkdir()
    (skill_dir / "references").mkdir()
    (skill_dir / "references" / "INTROSPECTION.md").write_text("legacy body", encoding="utf-8")

    md = _Meta(
        "legacy",
        str(skill_dir),
        introspection_file="references/INTROSPECTION.md",
    )

    with warnings.catch_warnings(record=True) as captured:
        warnings.simplefilter("always", DeprecationWarning)
        out = _handle_list({"legacy": md}, {"skill": "legacy"})

    deprecations = [w for w in captured if issubclass(w.category, DeprecationWarning)]
    assert deprecations, "Expected DeprecationWarning for the legacy introspection key"
    assert "introspection" in str(deprecations[0].message)
    assert "skill-reference-docs" in str(deprecations[0].message)

    assert out["success"] is True
    paths = {f["path"] for f in out["context"]["files"]}
    assert "references/INTROSPECTION.md" in paths


def test_modern_skill_reference_docs_key_does_not_emit_deprecation(tmp_path: Path) -> None:
    """Skills using the supported ``skill-reference-docs`` key must never
    trigger the legacy ``DeprecationWarning``.
    """
    skill_dir = tmp_path / "modern"
    skill_dir.mkdir()
    (skill_dir / "references").mkdir()
    (skill_dir / "references" / "NOTE.md").write_text("modern body", encoding="utf-8")

    md = _Meta(
        "modern",
        str(skill_dir),
        metadata={METADATA_SKILL_REFERENCE_DOCS_KEY: ["references/*.md"]},
    )

    with warnings.catch_warnings(record=True) as captured:
        warnings.simplefilter("always", DeprecationWarning)
        out = _handle_list({"modern": md}, {"skill": "modern"})

    deprecations = [w for w in captured if issubclass(w.category, DeprecationWarning)]
    assert not deprecations, "skill-reference-docs callers must not see DeprecationWarning; got: " + ", ".join(
        str(w.message) for w in deprecations
    )
    assert out["success"] is True


# Silence pytest-on-warning so other tests do not flake on the legacy path.
pytestmark = pytest.mark.filterwarnings(
    "ignore:Skill.*deprecated 'metadata.dcc-mcp.introspection':DeprecationWarning",
)


def test_register_tools_smoke() -> None:
    class _Reg:
        def __init__(self) -> None:
            self.names: list[str] = []

        def register(self, **kwargs: object) -> None:
            self.names.append(str(kwargs.get("name", "")))

    class _Srv:
        def __init__(self) -> None:
            self.registry = _Reg()
            self.handlers: dict[str, object] = {}

        def register_handler(self, name: str, handler: object) -> None:
            self.handlers[name] = handler

    srv = _Srv()
    md = _Meta("x", "/nonexistent")
    register_skill_reference_docs_tools(srv, skills=[md], dcc_name="maya")
    assert "skill_refs__list" in srv.registry.names
    assert "skill_refs__read" in srv.registry.names
