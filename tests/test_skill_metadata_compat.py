"""Tests for agentskills.io-compliant ``metadata.dcc-mcp.*`` parsing.

Covers issue #356: the SKILL.md loader dual-reads the new, spec-compliant
``metadata.dcc-mcp.*`` keys and the legacy top-level extension fields,
surfacing a ``is_spec_compliant()`` flag so callers can drive
deprecation warnings.
"""

from __future__ import annotations

import json
from pathlib import Path

import dcc_mcp_core

FIXTURES = Path(__file__).parent / "fixtures" / "skills"
NEW_FORM = FIXTURES / "new-form-skill"


def _parse(skill_dir: Path):
    meta = dcc_mcp_core.parse_skill_md(str(skill_dir))
    assert meta is not None, f"parse_skill_md returned None for {skill_dir}"
    return meta


class TestNewForm:
    """Spec-compliant SKILL.md uses only metadata.dcc-mcp.* keys."""

    def test_parses(self) -> None:
        meta = _parse(NEW_FORM)
        assert meta.name == "new-form-skill"

    def test_is_spec_compliant(self) -> None:
        meta = _parse(NEW_FORM)
        assert meta.is_spec_compliant() is True
        assert meta.legacy_extension_fields == []

    def test_values_populated(self) -> None:
        meta = _parse(NEW_FORM)
        assert meta.dcc == "maya"
        assert meta.version == "1.2.3"
        assert meta.tags == ["modeling", "polygon", "bevel"]
        assert meta.search_hint == "bevel edges mesh polygon modeling"

    def test_sibling_tools_yaml_resolved(self) -> None:
        meta = _parse(NEW_FORM)
        names = [t.name for t in meta.tools]
        assert names == ["bevel", "measure"]
        bevel = meta.tools[0]
        assert bevel.description == "Apply a bevel to the selected edges."
        assert bevel.destructive is True
        # groups sidecar from tools.yaml
        assert [g.name for g in meta.groups] == ["advanced"]
        assert meta.groups[0].default_active is False

    def test_policy_overrides(self) -> None:
        meta = _parse(NEW_FORM)
        # `products` and `allow-implicit-invocation` from metadata.dcc-mcp.*
        # must feed into the SkillPolicy surface.
        assert meta.is_implicit_invocation_allowed() is False
        assert meta.matches_product("maya") is True
        assert meta.matches_product("blender") is False
        policy_json = meta.policy
        assert policy_json is not None
        policy = json.loads(policy_json)
        assert sorted(policy["products"]) == ["houdini", "maya"]
        assert policy["allow_implicit_invocation"] is False


class TestInlineNewForm:
    """Smoke-test an inline new-form SKILL.md written at runtime."""

    def test_inline(self, tmp_path: Path) -> None:
        skill_dir = tmp_path / "inline"
        skill_dir.mkdir()
        (skill_dir / "SKILL.md").write_text(
            (
                "---\n"
                "name: inline\n"
                "description: inline new-form skill\n"
                "metadata:\n"
                "  dcc-mcp.dcc: houdini\n"
                '  dcc-mcp.version: "0.1.0"\n'
                '  dcc-mcp.tags: "a, b"\n'
                "---\n"
                "# body\n"
            ),
            encoding="utf-8",
        )
        meta = _parse(skill_dir)
        assert meta.is_spec_compliant() is True
        assert meta.dcc == "houdini"
        assert meta.version == "0.1.0"
        assert meta.tags == ["a", "b"]


def test_is_spec_compliant_signature_is_a_method() -> None:
    """`is_spec_compliant` must be callable, not a property (issue #356 AC 4)."""
    meta = dcc_mcp_core.SkillMetadata(name="smoke")
    assert callable(meta.is_spec_compliant)
    assert meta.is_spec_compliant() is True
