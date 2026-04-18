"""Tests for the SEP-986 naming validators exposed from ``dcc_mcp_core``.

The authoritative Rust unit tests live in ``crates/dcc-mcp-naming/src/lib.rs``.
These Python-side tests guard the PyO3 binding layer: constants are reachable,
validators raise ``ValueError`` on bad input, and accept the spec's canonical
examples. See issue #260 for the full contract.
"""

from __future__ import annotations

import re
from typing import ClassVar

import pytest

from dcc_mcp_core import ACTION_ID_RE
from dcc_mcp_core import MAX_TOOL_NAME_LEN
from dcc_mcp_core import TOOL_NAME_RE
from dcc_mcp_core import validate_action_id
from dcc_mcp_core import validate_tool_name

# ── Constants ───────────────────────────────────────────────────────────────


class TestNamingConstants:
    def test_tool_name_regex_is_anchored(self) -> None:
        assert TOOL_NAME_RE.startswith("^")
        assert TOOL_NAME_RE.endswith("$")

    def test_action_id_regex_is_anchored(self) -> None:
        assert ACTION_ID_RE.startswith("^")
        assert ACTION_ID_RE.endswith("$")

    def test_max_tool_name_len_is_48(self) -> None:
        # Capped below MCP spec (128) to leave room for gateway prefixes.
        assert MAX_TOOL_NAME_LEN == 48

    def test_regexes_compile(self) -> None:
        re.compile(TOOL_NAME_RE)
        re.compile(ACTION_ID_RE)


# ── validate_tool_name ──────────────────────────────────────────────────────


class TestValidateToolName:
    @pytest.mark.parametrize(
        "name",
        [
            "create_sphere",
            "geometry.create_sphere",
            "scene.object.transform",
            "hello-world.greet",
            "CamelCaseTool",
            "a",
            "Z",
            "0",
            "a" * MAX_TOOL_NAME_LEN,
        ],
    )
    def test_accepts_valid_names(self, name: str) -> None:
        validate_tool_name(name)  # does not raise

    def test_rejects_empty(self) -> None:
        with pytest.raises(ValueError):
            validate_tool_name("")

    def test_rejects_over_max_length(self) -> None:
        with pytest.raises(ValueError):
            validate_tool_name("a" * (MAX_TOOL_NAME_LEN + 1))

    @pytest.mark.parametrize("name", ["-tool", ".tool", "_tool"])
    def test_rejects_bad_leading_char(self, name: str) -> None:
        with pytest.raises(ValueError):
            validate_tool_name(name)

    @pytest.mark.parametrize(
        "name",
        [
            "tool/call",
            "ns:tool",
            "tool name",
            "tool,other",
            "tool@host",
            "tool+v2",
            "tool?",
            "tool!",
            "tool#1",
        ],
    )
    def test_rejects_forbidden_chars(self, name: str) -> None:
        with pytest.raises(ValueError):
            validate_tool_name(name)

    @pytest.mark.parametrize("name", ["tôol", "工具", "tool\u00e9"])
    def test_rejects_non_ascii(self, name: str) -> None:
        with pytest.raises(ValueError):
            validate_tool_name(name)

    def test_error_message_mentions_char_for_bad_char(self) -> None:
        with pytest.raises(ValueError, match=r"'/'"):
            validate_tool_name("bad/name")


# ── validate_action_id ──────────────────────────────────────────────────────


class TestValidateActionId:
    @pytest.mark.parametrize(
        "name",
        [
            "scene",
            "create_sphere",
            "scene.get_info",
            "maya.geometry.create_sphere",
            "v2.create",
            "scene.frame_3d",
        ],
    )
    def test_accepts_valid_ids(self, name: str) -> None:
        validate_action_id(name)

    def test_rejects_empty(self) -> None:
        with pytest.raises(ValueError):
            validate_action_id("")

    @pytest.mark.parametrize(
        "name",
        ["Scene.get", "scene.Get", "scene.getInfo"],
    )
    def test_rejects_uppercase(self, name: str) -> None:
        with pytest.raises(ValueError):
            validate_action_id(name)

    @pytest.mark.parametrize("name", ["1scene.get", "scene.1get"])
    def test_rejects_leading_digit(self, name: str) -> None:
        with pytest.raises(ValueError):
            validate_action_id(name)

    @pytest.mark.parametrize("name", [".scene", "scene.", "scene..get"])
    def test_rejects_empty_segment(self, name: str) -> None:
        with pytest.raises(ValueError):
            validate_action_id(name)

    @pytest.mark.parametrize(
        "name",
        ["scene-get", "scene/get", "scene get", "scene@host"],
    )
    def test_rejects_punct(self, name: str) -> None:
        with pytest.raises(ValueError):
            validate_action_id(name)

    def test_rejects_non_ascii(self) -> None:
        with pytest.raises(ValueError):
            validate_action_id("scene.fü")


# ── Cross-checks ────────────────────────────────────────────────────────────


class TestRegexMatchesValidator:
    """The exported regex pattern must agree with the validator on a
    representative sample. We don't chase full equivalence — the validator is
    the authoritative source of truth — but the regex is published as a
    contract for downstream tooling (docs, schema generators), so it must at
    least accept every name the validator accepts and reject everything the
    validator rejects in the sampled set.
    """

    SAMPLES_OK: ClassVar[list[str]] = [
        "geometry.create_sphere",
        "hello-world.greet",
        "CamelCase",
        "a" * MAX_TOOL_NAME_LEN,
    ]
    SAMPLES_BAD: ClassVar[list[str]] = [
        "",
        "_x",
        "-x",
        ".x",
        "bad/name",
        "a" * (MAX_TOOL_NAME_LEN + 1),
        "tôol",
    ]

    def test_tool_name_regex_accepts_valid_samples(self) -> None:
        rx = re.compile(TOOL_NAME_RE)
        for name in self.SAMPLES_OK:
            assert rx.match(name) is not None, f"regex rejected valid sample {name!r}"

    def test_tool_name_regex_rejects_invalid_samples(self) -> None:
        rx = re.compile(TOOL_NAME_RE)
        for name in self.SAMPLES_BAD:
            # Non-ASCII fails the `[A-Za-z0-9]` class — Python regex on str
            # treats `\u00f4` as a literal non-matching char. Over-length
            # matches fail the `{0,47}` quantifier. Empty matches nothing.
            assert rx.match(name) is None, f"regex accepted invalid sample {name!r}"
