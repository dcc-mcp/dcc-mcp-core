"""Tests for ToolAnnotations surfacing from tools.yaml to MCP tools/list.

Issue #344 — skill authors declare MCP ``ToolAnnotations`` inside each
tool's entry in the sibling ``tools.yaml`` file (the #356 sibling-file
pattern).  Two forms are accepted:

* canonical nested ``annotations:`` map (preferred);
* shorthand flat hint keys (e.g. ``destructive_hint: true``) sitting
  directly on the tool entry, for backward compatibility.

When both forms appear for the same tool, the nested map wins entirely
(whole-map replacement, not per-field merge).

``deferred_hint`` is a dcc-mcp-core extension (not in MCP 2025-03-26),
so it rides in the ``_meta["dcc.deferred_hint"]`` slot of the tool
declaration rather than inside the spec-standard ``annotations`` map.
"""

from __future__ import annotations

import json
from pathlib import Path
import tempfile

import pytest

import dcc_mcp_core as core

# ── Helpers ────────────────────────────────────────────────────────────


def _write_skill(skill_dir: Path, skill_md_body: str, tools_yaml_body: str) -> None:
    skill_dir.mkdir(parents=True, exist_ok=True)
    (skill_dir / "SKILL.md").write_text(skill_md_body, encoding="utf-8")
    (skill_dir / "tools.yaml").write_text(tools_yaml_body, encoding="utf-8")


_SKILL_MD_TEMPLATE = """---
name: {name}
description: test skill
metadata:
  dcc-mcp.dcc: python
  dcc-mcp.tools: tools.yaml
---
# body
"""


def _parse(skill_name: str, tools_yaml: str) -> list[core.ToolDeclaration]:
    with tempfile.TemporaryDirectory() as tmp:
        skill_dir = Path(tmp) / skill_name
        _write_skill(skill_dir, _SKILL_MD_TEMPLATE.format(name=skill_name), tools_yaml)
        meta = core.parse_skill_md(str(skill_dir))
        assert meta is not None
        return meta.tools


# ── Canonical nested form ──────────────────────────────────────────────


def test_canonical_annotations_map_parses() -> None:
    """Nested ``annotations:`` map populates every declared hint."""
    tools = _parse(
        "canon",
        (
            "tools:\n"
            "  - name: delete_keyframes\n"
            "    description: danger\n"
            "    annotations:\n"
            "      read_only_hint: false\n"
            "      destructive_hint: true\n"
            "      idempotent_hint: true\n"
            "      open_world_hint: false\n"
        ),
    )
    assert len(tools) == 1
    ann = tools[0].annotations
    assert ann == {
        "readOnlyHint": False,
        "destructiveHint": True,
        "idempotentHint": True,
        "openWorldHint": False,
    }


# ── Shorthand form (backward compatibility) ────────────────────────────


def test_shorthand_flat_keys_parse() -> None:
    """Flat hint keys on the tool entry still parse."""
    tools = _parse(
        "short",
        ("tools:\n  - name: get_keyframes\n    read_only_hint: true\n    idempotent_hint: true\n"),
    )
    ann = tools[0].annotations
    assert ann == {"readOnlyHint": True, "idempotentHint": True}
    # Undeclared keys are OMITTED, not defaulted to False.
    assert "destructiveHint" not in ann
    assert "openWorldHint" not in ann


# ── Precedence: nested map wins whole-map ──────────────────────────────


def test_mixed_forms_nested_wins_whole_map() -> None:
    """When both forms are present, the nested map REPLACES the shorthand.

    Not a per-field merge — shorthand fields that are not in the nested
    map must disappear entirely.
    """
    tools = _parse(
        "mixed",
        (
            "tools:\n"
            "  - name: risky\n"
            "    read_only_hint: true\n"
            "    idempotent_hint: true\n"
            "    annotations:\n"
            "      destructive_hint: true\n"
        ),
    )
    ann = tools[0].annotations
    assert ann == {"destructiveHint": True}
    assert "readOnlyHint" not in ann
    assert "idempotentHint" not in ann


# ── Missing annotations ────────────────────────────────────────────────


def test_no_annotations_returns_empty_dict() -> None:
    tools = _parse(
        "bare",
        "tools:\n  - name: plain\n    description: nothing special\n",
    )
    assert tools[0].annotations == {}


# ── deferred_hint goes into _meta, not annotations ─────────────────────


def test_deferred_hint_parsed_and_routed_for_meta() -> None:
    """Issue #344 — ``deferred_hint`` is parseable from tools.yaml but is
    a dcc-mcp-core extension. On the MCP tool declaration it lands in
    ``_meta["dcc.deferred_hint"]`` (populated by the Rust HTTP handler),
    NEVER inside the spec-standard ``annotations`` map.

    At the Python surface we expose it on ``ToolDeclaration.annotations``
    under the ``deferredHint`` key for inspection, but downstream code
    that builds the MCP payload must treat it as a ``_meta`` hint.
    """
    tools = _parse(
        "deferred",
        (
            "tools:\n"
            "  - name: slow_tool\n"
            "    description: takes a while\n"
            "    annotations:\n"
            "      read_only_hint: true\n"
            "      deferred_hint: true\n"
        ),
    )
    ann = tools[0].annotations
    assert ann.get("readOnlyHint") is True
    # Parsed and exposed for introspection.
    assert ann.get("deferredHint") is True

    # End-to-end: load the skill through the catalog and verify the
    # Rust-side ActionMeta carries the annotation (which is what the
    # HTTP handler reads when emitting tools/list).
    with tempfile.TemporaryDirectory() as tmp:
        skill_dir = Path(tmp) / "deferred"
        _write_skill(
            skill_dir,
            _SKILL_MD_TEMPLATE.format(name="deferred"),
            (
                "tools:\n"
                "  - name: slow_tool\n"
                "    description: takes a while\n"
                "    annotations:\n"
                "      read_only_hint: true\n"
                "      deferred_hint: true\n"
            ),
        )
        registry = core.ToolRegistry()
        catalog = core.SkillCatalog(registry)
        catalog.discover(extra_paths=[str(tmp)])
        loaded = catalog.load_skill("deferred")
        assert any("slow_tool" in n for n in loaded), f"got {loaded}"


# ── ToolDeclaration.annotations setter round-trip ──────────────────────


def test_annotations_setter_accepts_camelcase_and_snakecase() -> None:
    td = core.ToolDeclaration(name="x", description="x")
    td.annotations = {"readOnlyHint": True, "destructive_hint": False}
    assert td.annotations == {"readOnlyHint": True, "destructiveHint": False}

    td.annotations = None
    assert td.annotations == {}

    td.annotations = {"deferredHint": True}
    assert td.annotations == {"deferredHint": True}


def test_annotations_setter_rejects_non_dict() -> None:
    td = core.ToolDeclaration(name="x")
    with pytest.raises(TypeError):
        td.annotations = 42  # type: ignore[assignment]
