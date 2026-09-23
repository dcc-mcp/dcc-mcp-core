"""P0 coverage for the hand-rolled SKILL.md frontmatter parser.

``_parse_simple_yaml`` and its helpers (``_path_from_indent``, ``_set_nested``,
``_parse_flow_sequence``) live in
``skills/marketplace-publish-extension/scripts/publish.py`` and parse the
frontmatter of every extension that gets published. One mis-parsed character
silently lands a wrong description, tag list, or DCC list in ``marketplace.json``
and no downstream step asserts otherwise.

Cases marked *characterization* pin behaviour that diverges from the YAML spec.
They document the divergence instead of endorsing it; each one is reported as a
follow-up defect and must be updated together with the parser fix.
"""

from __future__ import annotations

import importlib.util
from pathlib import Path

import pytest

from conftest import REPO_ROOT

_SCRIPT = REPO_ROOT / "skills" / "marketplace-publish-extension" / "scripts" / "publish.py"
_SPEC = importlib.util.spec_from_file_location("marketplace_publish_frontmatter_yaml", _SCRIPT)
assert _SPEC is not None and _SPEC.loader is not None
_PUBLISH = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(_PUBLISH)

_parse_simple_yaml = _PUBLISH._parse_simple_yaml
_parse_flow_sequence = _PUBLISH._parse_flow_sequence
_path_from_indent = _PUBLISH._path_from_indent
_set_nested = _PUBLISH._set_nested


# 1. Plain `key: value` scalars


@pytest.mark.parametrize(
    ("text", "expected"),
    [
        pytest.param("name: maya-tools\n", {"name": "maya-tools"}, id="single-scalar"),
        pytest.param(
            "name: maya-tools\nversion: 1.2.3\nlicense: MIT-0\n",
            {"name": "maya-tools", "version": "1.2.3", "license": "MIT-0"},
            id="multiple-scalars",
        ),
        pytest.param('name: "quoted name"\n', {"name": "quoted name"}, id="double-quoted-value"),
        pytest.param("name: 'quoted name'\n", {"name": "quoted name"}, id="single-quoted-value"),
        pytest.param(
            'compatibility: "Python 3.7+, dcc-mcp-core 0.17+"\n',
            {"compatibility": "Python 3.7+, dcc-mcp-core 0.17+"},
            id="quoted-value-with-punctuation",
        ),
        pytest.param(
            'version: "0.1.0"\n',
            {"version": "0.1.0"},
            id="quoted-version-stays-a-string",
        ),
        pytest.param(
            "allowed-tools: Bash Read Write Edit\n",
            {"allowed-tools": "Bash Read Write Edit"},
            id="space-separated-value-kept-verbatim",
        ),
        pytest.param("dcc-mcp: yes\n", {"dcc-mcp": "yes"}, id="dashed-key"),
        pytest.param("name: a\nname: b\n", {"name": "b"}, id="duplicate-key-last-wins"),
        pytest.param(
            "name: x  # trailing comment\n",
            {"name": "x  # trailing comment"},
            id="characterization-trailing-comment-not-stripped",
        ),
        pytest.param("name:\n", {}, id="key-without-value-creates-no-entry"),
    ],
)
def test_plain_key_value_pairs(text: str, expected: dict) -> None:
    assert _parse_simple_yaml(text) == expected


# 2. Values that contain colons


@pytest.mark.parametrize(
    ("text", "expected"),
    [
        pytest.param(
            "description: Use this: carefully\n",
            {"description": "Use this: carefully"},
            id="colon-inside-value",
        ),
        pytest.param("title: a:b:c\n", {"title": "a:b:c"}, id="repeated-colons"),
        pytest.param(
            "url: https://example.com/docs:latest\n",
            {"url": "https://example.com/docs:latest"},
            id="url-with-colon-and-port-like-suffix",
        ),
        pytest.param(
            "name: maya-tools\ndescription: Use this: carefully\n",
            {"name": "maya-tools", "description": "Use this: carefully"},
            id="colon-value-does-not-disturb-neighbours",
        ),
    ],
)
def test_values_containing_colons(text: str, expected: dict) -> None:
    assert _parse_simple_yaml(text) == expected


# 3. `>-` folded block scalars


@pytest.mark.parametrize(
    ("text", "expected"),
    [
        pytest.param(
            "description: >-\n  Line one\n  Line two\n",
            {"description": "Line one Line two"},
            id="two-line-fold",
        ),
        pytest.param(
            "description: >-\n  Only line\n",
            {"description": "Only line"},
            id="single-line-fold",
        ),
        pytest.param(
            "description: >-\n  Folded text\nname: after\n",
            {"description": "Folded text", "name": "after"},
            id="fold-terminated-by-next-key",
        ),
        pytest.param(
            "name: before\ndescription: >-\n  Folded at eof\n",
            {"name": "before", "description": "Folded at eof"},
            id="fold-finalised-at-eof",
        ),
        pytest.param(
            "metadata:\n  dcc-mcp:\n    search-hint: >-\n      alpha beta\n      gamma\n",
            {"metadata": {"dcc-mcp": {"search-hint": "alpha beta gamma"}}},
            id="fold-inside-nested-block",
        ),
        pytest.param(
            "description: >-\n  Line one\n  Line two\n\n  Line three\n",
            {"description": "Line one Line two  Line three"},
            id="characterization-blank-line-becomes-double-space",
        ),
    ],
)
def test_folded_block_scalars(text: str, expected: dict) -> None:
    assert _parse_simple_yaml(text) == expected


# 4. Flow sequences


@pytest.mark.parametrize(
    ("text", "expected"),
    [
        pytest.param("tags: [a, b, c]\n", {"tags": ["a", "b", "c"]}, id="bare-items"),
        pytest.param(
            'tags: ["marketplace", "extension", "maya"]\n',
            {"tags": ["marketplace", "extension", "maya"]},
            id="quoted-items",
        ),
        pytest.param("tags: [maya]\n", {"tags": ["maya"]}, id="single-item"),
        pytest.param('tags: [a, b, "c"]\n', {"tags": ["a", "b", "c"]}, id="mixed-quoting"),
        pytest.param(
            'tags: ["a, b", c]\n',
            {"tags": ["a, b", "c"]},
            id="double-quoted-item-containing-a-comma",
        ),
        pytest.param(
            "tags: ['a, b', c]\n",
            {"tags": ["a, b", "c"]},
            id="single-quoted-item-containing-a-comma",
        ),
        pytest.param(
            'tags: ["it\'s", x]\n',
            {"tags": ["it's", "x"]},
            id="apostrophe-inside-double-quoted-item",
        ),
        pytest.param("tags: [a:b, c]\n", {"tags": ["a:b", "c"]}, id="item-containing-a-colon"),
        pytest.param("tags: []\n", {"tags": []}, id="empty-sequence"),
        pytest.param("tags: [   ]\n", {"tags": []}, id="whitespace-only-sequence"),
        pytest.param(
            'tags: ["a, b]\n',
            {"tags": ["a, b"]},
            id="characterization-unterminated-quote-swallows-the-rest-of-the-item",
        ),
        pytest.param(
            "tags: [a, b]\nname: x\n",
            {"tags": ["a", "b"], "name": "x"},
            id="sequence-followed-by-scalar",
        ),
    ],
)
def test_flow_sequences(text: str, expected: dict) -> None:
    assert _parse_simple_yaml(text) == expected


# 5. Nested blocks (2-space indent)


@pytest.mark.parametrize(
    ("text", "expected"),
    [
        pytest.param(
            "metadata:\n  dcc-mcp:\n    dcc: maya\n",
            {"metadata": {"dcc-mcp": {"dcc": "maya"}}},
            id="two-level-nesting",
        ),
        pytest.param(
            'metadata:\n  dcc-mcp:\n    dcc: maya\n    version: "1.0"\n    layer: domain\n',
            {"metadata": {"dcc-mcp": {"dcc": "maya", "version": "1.0", "layer": "domain"}}},
            id="dcc-mcp-metadata-block",
        ),
        pytest.param(
            "metadata:\n  dcc-mcp:\n    dcc: maya\n  other:\n    k: v\n",
            {"metadata": {"dcc-mcp": {"dcc": "maya"}, "other": {"k": "v"}}},
            id="sibling-parent-key-dedents-back-one-level",
        ),
        pytest.param(
            "metadata:\n  dcc-mcp:\n    dcc: maya\n  other: x\n",
            {"metadata": {"dcc-mcp": {"dcc": "maya", "other": "x"}}},
            id="characterization-sibling-scalar-dedent-is-swallowed-by-the-open-block",
        ),
        pytest.param(
            "a:\n  b:\n    c:\n      d: leaf\n",
            {"a": {"b": {"c": {"d": "leaf"}}}},
            id="four-level-nesting",
        ),
        pytest.param("a:\n    b: 1\n", {"a": {"b": "1"}}, id="four-space-indent-under-parent"),
        pytest.param(
            "metadata:\n  dcc-mcp:\n    dcc: maya\nlicense: MIT-0\n",
            {"metadata": {"dcc-mcp": {"dcc": "maya", "license": "MIT-0"}}},
            id="characterization-scalar-after-nested-block-is-swallowed",
        ),
        pytest.param(
            "metadata:\n\tdcc-mcp:\n\t\tdcc: maya\n",
            {"dcc-mcp": {"dcc": "maya"}},
            id="characterization-tab-indent-drops-the-metadata-root",
        ),
        pytest.param(
            "items:\n  - a\n  - b\n",
            {},
            id="characterization-block-sequence-items-are-dropped",
        ),
    ],
)
def test_nested_blocks(text: str, expected: dict) -> None:
    assert _parse_simple_yaml(text) == expected


# 6. Empty / comment-only input


@pytest.mark.parametrize(
    "text",
    [
        pytest.param("", id="empty-string"),
        pytest.param("\n", id="single-newline"),
        pytest.param("\n\n\n", id="blank-lines-only"),
        pytest.param("   \n  \n", id="whitespace-only"),
        pytest.param("# just a comment\n", id="comment-only"),
        pytest.param("no colon anywhere\n", id="line-without-colon"),
    ],
)
def test_empty_and_unparseable_input_yields_empty_mapping(text: str) -> None:
    assert _parse_simple_yaml(text) == {}


# 7. CRLF line endings


@pytest.mark.parametrize(
    ("text", "expected"),
    [
        pytest.param(
            "name: maya-tools\r\nversion: 1.2.3\r\n",
            {"name": "maya-tools", "version": "1.2.3"},
            id="crlf-scalars",
        ),
        pytest.param(
            "name: a\r\nversion: 1\n",
            {"name": "a", "version": "1"},
            id="mixed-line-endings",
        ),
        pytest.param(
            "description: >-\r\n  Line one\r\n  Line two\r\n",
            {"description": "Line one Line two"},
            id="crlf-folded-scalar",
        ),
        pytest.param(
            "metadata:\r\n  dcc-mcp:\r\n    dcc: maya\r\n",
            {"metadata": {"dcc-mcp": {"dcc": "maya"}}},
            id="crlf-nested-block",
        ),
        pytest.param(
            'tags: [a, "b, c"]\r\n',
            {"tags": ["a", "b, c"]},
            id="crlf-flow-sequence",
        ),
    ],
)
def test_crlf_line_endings(text: str, expected: dict) -> None:
    assert _parse_simple_yaml(text) == expected


# Helper-level unit coverage


@pytest.mark.parametrize(
    ("inner", "expected"),
    [
        pytest.param("a, b, c", ["a", "b", "c"], id="bare-items"),
        pytest.param('"a", "b"', ["a", "b"], id="quoted-items"),
        pytest.param('"a, b", c', ["a, b", "c"], id="quoted-comma"),
        pytest.param("", [], id="empty-inner"),
        pytest.param("   ", [], id="blank-inner"),
        pytest.param("a,,b", ["a", "b"], id="empty-item-skipped"),
        pytest.param("  a  ,  b  ", ["a", "b"], id="surrounding-whitespace-trimmed"),
        pytest.param("'a, b'", ["a, b"], id="single-quoted-comma"),
    ],
)
def test_parse_flow_sequence(inner: str, expected: list) -> None:
    assert _parse_flow_sequence(inner) == expected


def test_path_from_indent_descends_and_dedents() -> None:
    assert _path_from_indent({}, 0, "top", []) == ["top"]
    assert _path_from_indent({}, 2, "child", ["top"]) == ["top", "child"]
    assert _path_from_indent({}, 4, "grandchild", ["top", "child"]) == ["top", "child", "grandchild"]
    assert _path_from_indent({}, 2, "sibling", ["top", "child"]) == ["top", "sibling"]
    assert _path_from_indent({}, 0, "other", ["top", "child"]) == ["other"]


def test_set_nested_creates_intermediate_dicts_and_overwrites_leaves() -> None:
    root: dict = {}
    _set_nested(root, ["a", "b", "c"], "leaf")
    assert root == {"a": {"b": {"c": "leaf"}}}

    _set_nested(root, ["a", "b", "c"], "replaced")
    assert root["a"]["b"]["c"] == "replaced"

    _set_nested(root, ["a", "sibling"], 1)
    assert root["a"]["sibling"] == 1
    assert root["a"]["b"] == {"c": "replaced"}
