"""Tests for the write-verb pairing lint rule (core#2269 part 1)."""

from __future__ import annotations

import pytest

from dcc_mcp_core.verification import DeclarationFinding
from dcc_mcp_core.verification import find_unpaired_write_verbs
from dcc_mcp_core.verification import lint_tool_table


def _tool(name, read_only=False, **extra):
    tool = {"name": name, "read_only": read_only, "destructive": False, "idempotent": False}
    tool.update(extra)
    return tool


class TestPairingRule:
    def test_write_verb_without_any_read_tool_is_flagged(self):
        tools = [_tool("set_keyframes"), _tool("apply_material")]
        findings = find_unpaired_write_verbs(tools)
        assert {f.tool for f in findings} == {"set_keyframes", "apply_material"}
        assert all(f.code == "unpaired_write_verb" for f in findings)

    def test_write_verb_paired_by_domain_token(self):
        tools = [
            _tool("set_keyframes"),
            _tool("get_keyframes", read_only=True),
        ]
        assert find_unpaired_write_verbs(tools) == []

    def test_write_verb_paired_by_next_success(self):
        tools = [
            _tool("sequence_to_mp4", **{"next-tools": {"on-success": ["media__probe"]}}),
            _tool("probe", read_only=True),
        ]
        assert find_unpaired_write_verbs(tools) == []

    def test_write_verb_paired_by_explicit_readback(self):
        tools = [
            _tool("bake_simulation", readback="inspect_sim_cache"),
            _tool("inspect_sim_cache", read_only=True),
        ]
        assert find_unpaired_write_verbs(tools) == []

    def test_common_verbs_do_not_count_as_domain_signal(self):
        # set/get share only the "set"/"get" verbs, which are stopwords; with no
        # shared domain token the write is flagged.
        tools = [_tool("set"), _tool("get", read_only=True)]
        findings = find_unpaired_write_verbs(tools)
        assert {f.tool for f in findings} == {"set"}

    def test_read_only_tools_are_never_flagged(self):
        tools = [_tool("probe", read_only=True), _tool("get_anim_curves", read_only=True)]
        assert find_unpaired_write_verbs(tools) == []

    def test_absent_read_only_is_treated_as_write(self):
        tools = [{"name": "mystery_verb"}]
        findings = find_unpaired_write_verbs(tools)
        assert {f.tool for f in findings} == {"mystery_verb"}

    def test_domain_tokens_match_across_camel_and_snake(self):
        tools = [_tool("setSkinWeights"), _tool("export_skin_state", read_only=True)]
        assert find_unpaired_write_verbs(tools) == []

    def test_lint_tool_table_alias_matches(self):
        tools = [_tool("set_keyframes"), _tool("get_keyframes", read_only=True)]
        assert lint_tool_table(tools) == find_unpaired_write_verbs(tools)

    def test_finding_serializes(self):
        tools = [_tool("set_keyframes")]
        finding = find_unpaired_write_verbs(tools)[0]
        assert isinstance(finding, DeclarationFinding)
        assert finding.to_dict() == {
            "tool": "set_keyframes",
            "code": "unpaired_write_verb",
            "reason": finding.reason,
        }


class TestKeySpellingCompatibility:
    """Both key spellings must pair a write verb.

    ``tools.yaml`` uses hyphens (``next-tools.on-success``) because that is the
    wire contract shipped by the bundled skills; Python-side
    ``ToolDeclaration`` metadata uses underscores (``next_tools.on_success``).
    A declaration is a pairing signal in either spelling, so a verb must not be
    reported unpaired just because the table used the other convention.
    """

    def test_underscore_outer_and_inner_keys_pair(self):
        tools = [
            _tool("sequence_to_mp4", **{"next_tools": {"on_success": ["media__probe"]}}),
            _tool("probe", read_only=True),
        ]
        assert find_unpaired_write_verbs(tools) == []

    def test_underscore_outer_key_with_hyphen_inner_key_pairs(self):
        tools = [
            _tool("sequence_to_mp4", **{"next_tools": {"on-success": ["media__probe"]}}),
            _tool("probe", read_only=True),
        ]
        assert find_unpaired_write_verbs(tools) == []

    def test_hyphen_outer_key_with_underscore_inner_key_pairs(self):
        tools = [
            _tool("sequence_to_mp4", **{"next-tools": {"on_success": ["media__probe"]}}),
            _tool("probe", read_only=True),
        ]
        assert find_unpaired_write_verbs(tools) == []

    def test_bundled_media_skill_hyphen_spelling_pairs_its_write_verbs(self):
        """The real bundled table uses the hyphen spelling; it must still pair."""
        from pathlib import Path

        # yaml_loads is Rust-backed, so this one test needs the native extension.
        pytest.importorskip("dcc_mcp_core._core", reason="native extension not compiled")
        from dcc_mcp_core import yaml_loads

        tools_yaml = Path(__file__).parent.parent / "python" / "dcc_mcp_core" / "skills" / "media" / "tools.yaml"
        table = yaml_loads(tools_yaml.read_text(encoding="utf-8")) or {}
        tools = table.get("tools") or []
        assert tools, "the bundled media skill must declare a tool table"

        declared = {
            tool["name"]
            for tool in tools
            if any(key in tool for key in ("next-tools", "next_tools"))
            and "on-success" in (tool.get("next-tools") or tool.get("next_tools") or {})
        }
        assert {"sequence_to_mp4", "transcode", "thumbnail"} <= declared

        reported = {finding.tool for finding in find_unpaired_write_verbs(tools)}
        assert reported & declared == set(), "a verb declaring on-success must pair regardless of spelling"

    def test_misspelled_keys_stay_unpaired(self):
        # A near-miss must not silently pair: only the two accepted spellings count.
        tools = [
            _tool("sequence_to_mp4", **{"nexttools": {"onsuccess": ["probe"]}}),
            _tool("probe", read_only=True),
        ]
        assert {f.tool for f in find_unpaired_write_verbs(tools)} == {"sequence_to_mp4"}
