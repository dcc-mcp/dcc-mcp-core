"""Tests for the write-verb pairing lint rule (core#2269 part 1)."""

from __future__ import annotations

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
            _tool("sequence_to_mp4", next_tools={"on-success": ["media__probe"]}),
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
