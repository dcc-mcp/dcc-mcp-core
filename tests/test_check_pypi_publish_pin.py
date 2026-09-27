"""Detection logic for the org-wide publish-action pin scanner.

The scanner exists because a name-only match cannot tell a pinned commit SHA
from the mutable ``release/v1`` tag. If the detection regex drifts, the scanner
starts reporting "OK" while the guard is silently gone, so the matching rules
are pinned here rather than left to be discovered by a real revert.
"""

from __future__ import annotations

from scripts.check_pypi_publish_pin import HEX40_RE
from scripts.check_pypi_publish_pin import scan_text

PINNED_SHA = "dc37677b2e1c63e2034f94d8a5b11f265b73ba33"


def _refs(text: str) -> list:
    return [ref for _, _, ref in scan_text(text)]


def _step(uses: str) -> str:
    return f"jobs:\n  publish:\n    steps:\n      - uses: {uses}\n"


def test_a_pinned_commit_sha_is_accepted() -> None:
    assert _refs(_step(f"pypa/gh-action-pypi-publish@{PINNED_SHA}")) == [PINNED_SHA]
    assert HEX40_RE.match(PINNED_SHA)


def test_a_trailing_version_comment_does_not_reach_the_ref() -> None:
    """Workflows carry ``# v1.14.2`` for humans; it must not become the ref."""
    line = f"pypa/gh-action-pypi-publish@{PINNED_SHA} # v1.14.2"
    assert _refs(_step(line)) == [PINNED_SHA]


def test_the_mutable_release_tag_is_rejected() -> None:
    """The exact rollback PIP-3715 exists to catch."""
    assert _refs(_step("pypa/gh-action-pypi-publish@release/v1")) == ["release/v1"]
    assert not HEX40_RE.match("release/v1")


def test_a_version_tag_is_rejected() -> None:
    """Tags are mutable too: only a commit SHA is immutable."""
    assert not HEX40_RE.match("v1.14.2")


def test_a_commented_out_uses_line_is_ignored() -> None:
    """A commented-out invocation is documentation, not an invocation."""
    text = "      # - uses: pypa/gh-action-pypi-publish@release/v1\n"
    assert _refs(text) == []


def test_an_expression_ref_is_rejected() -> None:
    """A computed ref cannot be verified as an immutable pin."""
    assert not HEX40_RE.match("${{ env.PUBLISH_ACTION_SHA }}")


def test_multiple_refs_on_one_workflow_are_all_reported() -> None:
    text = (
        f"      - uses: pypa/gh-action-pypi-publish@{PINNED_SHA}\n"
        "      - uses: pypa/gh-action-pypi-publish@release/v1\n"
    )
    assert _refs(text) == [PINNED_SHA, "release/v1"]
