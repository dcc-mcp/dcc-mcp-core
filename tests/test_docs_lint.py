"""Regression tests for the mechanical documentation contract checker."""

from __future__ import annotations

from pathlib import Path

import pytest
from scripts.docs_lint import RepoIndex
from scripts.docs_lint import check_emoji
from scripts.docs_lint import check_links
from scripts.docs_lint import check_structure
from scripts.docs_lint import check_symbols
from scripts.docs_lint import fence_step
from scripts.docs_lint import main


def _rules(findings):
    return sorted(finding["rule"] for finding in findings)


def _index(root):
    return RepoIndex([str(root)], set())


# --------------------------------------------------------------------------- #
# Structure
# --------------------------------------------------------------------------- #


def test_clean_document_has_no_structure_findings():
    text = "# Title\n\n## Section\n\nBody text.\n\n### Sub\n\nMore.\n"
    assert check_structure(text) == []


def test_heading_level_jump_is_an_error():
    text = "# Title\n\n#### Jumped\n"
    assert "structure/heading-level-jump" in _rules(check_structure(text))


def test_duplicate_sibling_heading_warns_but_nested_repeat_does_not():
    siblings = "# T\n\n## Added\n\n## Added\n"
    assert "structure/duplicate-heading" in _rules(check_structure(siblings))

    # A changelog repeats "Added" under every release heading; that is correct.
    nested = "# Changelog\n\n## 1.0.0\n\n### Added\n\n## 0.9.0\n\n### Added\n"
    assert "structure/duplicate-heading" not in _rules(check_structure(nested))


def test_unclosed_code_fence_is_an_error():
    assert "structure/unclosed-code-fence" in _rules(check_structure("# T\n\n```python\nx = 1\n"))


def test_fence_step_tracks_the_opener_marker():
    # A fence opens on a valid marker and closes only on a matching run that is
    # at least as long and carries nothing but whitespace after it.
    assert fence_step("```python", None) == ("```", True)
    assert fence_step("~~~json", None) == ("~~~", True)
    assert fence_step("~~~", "```") == ("```", False)  # wrong character
    assert fence_step("``", "```") == ("```", False)  # too short to be a fence
    assert fence_step("```", "`````") == ("`````", False)  # shorter than the opener
    assert fence_step("`````", "```") == (None, True)  # longer is a valid close
    assert fence_step("``` js", "```") == ("```", False)  # closer must be bare
    assert fence_step("    ```", None) == (None, False)  # indented block, not a fence
    assert fence_step("# Heading", None) == (None, False)


def test_fence_closes_only_on_a_matching_marker():
    # Old behaviour toggled a boolean, so a ~~~ run closed a ``` block and the
    # heading between the two was read as body text.
    text = "# T\n\n```\n~~~\n# Not a heading\n```\n"
    rules = _rules(check_structure(text))
    assert "structure/unclosed-code-fence" not in rules
    assert "structure/multiple-h1" not in rules


def test_fence_closes_only_on_a_bare_or_longer_marker():
    assert check_structure("# T\n\n`````\n```\n# Not a heading\n`````\n") == []
    assert check_structure("# T\n\n```\n``` text\n# Not a heading\n```\n") == []


def test_broken_toc_anchor_is_an_error():
    text = "# Title\n\n## Real Section\n\n[link](#nope)\n"
    assert "structure/broken-toc-anchor" in _rules(check_structure(text))


def test_valid_toc_anchor_passes():
    text = "# Title\n\n## Real Section\n\n[link](#real-section)\n"
    assert "structure/broken-toc-anchor" not in _rules(check_structure(text))


def test_multiple_h1_and_missing_trailing_newline_warn():
    assert "structure/multiple-h1" in _rules(check_structure("# A\n\n# B\n"))
    assert "structure/missing-trailing-newline" in _rules(check_structure("# A\n\nno newline"))


def test_tab_indentation_warns():
    assert "structure/tab-indentation" in _rules(check_structure("# A\n\n\tcode\n"))


# --------------------------------------------------------------------------- #
# Links
# --------------------------------------------------------------------------- #


def test_relative_link_resolution(tmp_path: Path):
    (tmp_path / "docs").mkdir()
    (tmp_path / "docs" / "present.md").write_text("# hi\n", encoding="utf-8")
    index = _index(tmp_path)

    assert check_links("[ok](present.md)", tmp_path / "docs", index) == []
    broken = check_links("[no](missing.md)", tmp_path / "docs", index)
    assert [f["rule"] for f in broken] == ["drift/broken-relative-link"]


def test_extensionless_and_site_root_absolute_links_resolve(tmp_path: Path):
    (tmp_path / "docs" / "guide").mkdir(parents=True)
    (tmp_path / "docs" / "guide" / "skills.md").write_text("# skills\n", encoding="utf-8")
    index = _index(tmp_path)
    base = tmp_path / "docs" / "guide"

    # Site-root-absolute (/guide/skills) and clean URLs (skills) both resolve.
    assert check_links("[a](/guide/skills)", base, index) == []
    assert check_links("[b](skills)", base, index) == []


def test_external_and_scheme_links_are_ignored(tmp_path: Path):
    index = _index(tmp_path)
    text = "[a](https://example.com)\n[b](mailto:x@y.z)\n[c](#anchor)\n[d](mention://issue/1)\n"
    assert check_links(text, tmp_path, index) == []


# --------------------------------------------------------------------------- #
# Emoji
# --------------------------------------------------------------------------- #


def test_emoji_in_heading_is_an_error():
    findings = check_emoji("## Shipping \U0001f680\n", 0.10)
    assert [f["severity"] for f in findings] == ["error"]
    assert findings[0]["rule"] == "emoji/emoji-in-heading"


def test_emoji_inside_code_span_is_not_style():
    assert check_emoji("## `print('\U0001f680')`\n", 0.10) == []


def test_emoji_density_warns_only_above_threshold():
    dense = "\n".join(f"line {i} \U0001f680" for i in range(30))
    # 30 emoji lines diluted into 300 plain lines is 9%, under the 10% default.
    sparse = dense + "\n" + "\n".join(f"plain line {i}" for i in range(300))
    assert "emoji/emoji-density" in _rules(check_emoji(dense, 0.10))
    assert "emoji/emoji-density" not in _rules(check_emoji(sparse, 0.10))


def test_emoji_inside_a_fenced_block_is_not_counted():
    lines = ["# T", "", "~~~", "```"] + ["\U0001f680"] * 30 + ["```", "~~~"]
    assert "emoji/emoji-density" not in _rules(check_emoji("\n".join(lines) + "\n", 0.10))


# --------------------------------------------------------------------------- #
# Drift
# --------------------------------------------------------------------------- #


def _drift_tokens(document, index):
    return [f["message"] for f in check_symbols(document, index)]


def test_existing_path_does_not_report(tmp_path: Path):
    (tmp_path / "scripts").mkdir()
    (tmp_path / "scripts" / "install-cli.sh").write_text("echo hi\n", encoding="utf-8")
    index = _index(tmp_path)
    # The literal path appears in no source file, so only the filesystem can
    # resolve it -- a corpus-only check would report this as stale.
    assert _drift_tokens("Run `scripts/install-cli.sh` first.\n", index) == []


def test_missing_path_reports(tmp_path: Path):
    index = _index(tmp_path)
    found = _drift_tokens("See `references/RECIPES.md`.\n", index)
    assert len(found) == 1 and "references/RECIPES.md" in found[0]


def test_slash_separated_enumerations_are_not_paths(tmp_path: Path):
    index = _index(tmp_path)
    for token in ("save/load/resume", "dcc-mcp-workflow/job-persist-sqlite", "tasks/list"):
        assert _drift_tokens(f"Use `{token}` here.\n", index) == []


def test_flag_matches_underscore_spelling(tmp_path: Path):
    (tmp_path / "config.toml").write_text("ws_port = 8080\n", encoding="utf-8")
    index = _index(tmp_path)
    # Rust and Python CLI layers declare --ws-port as ws_port.
    assert _drift_tokens("Pass `--ws-port`.\n", index) == []


def test_unimplemented_flag_reports(tmp_path: Path):
    (tmp_path / "config.toml").write_text("other = 1\n", encoding="utf-8")
    index = _index(tmp_path)
    found = _drift_tokens("Pass `--not-a-real-flag`.\n", index)
    assert len(found) == 1 and "--not-a-real-flag" in found[0]


def test_identifier_present_in_corpus_passes(tmp_path: Path):
    (tmp_path / "impl.py").write_text("def register_action():\n    pass\n", encoding="utf-8")
    index = _index(tmp_path)
    assert _drift_tokens("Call `register_action()`.\n", index) == []


def test_identifier_absent_from_corpus_reports(tmp_path: Path):
    (tmp_path / "impl.py").write_text("def other():\n    pass\n", encoding="utf-8")
    index = _index(tmp_path)
    found = _drift_tokens("Call `removed_helper()`.\n", index)
    assert len(found) == 1 and "removed_helper" in found[0]


def test_markdown_is_not_part_of_the_drift_corpus(tmp_path: Path):
    # A document must not be able to validate its own claims.
    (tmp_path / "only_doc.md").write_text("Use `ghost_symbol` here.\n", encoding="utf-8")
    index = _index(tmp_path)
    found = _drift_tokens("Use `ghost_symbol` here.\n", index)
    assert len(found) == 1 and "ghost_symbol" in found[0]


def test_fenced_code_blocks_are_not_checked(tmp_path: Path):
    index = _index(tmp_path)
    assert _drift_tokens("```\nghost_symbol\n```\n", index) == []


def test_backtick_run_inside_a_tilde_block_is_content(tmp_path: Path):
    index = _index(tmp_path)
    assert _drift_tokens("~~~\n```\nghost_symbol\n~~~\n", index) == []


# --------------------------------------------------------------------------- #
# Driver / exit codes
# --------------------------------------------------------------------------- #


def test_main_reports_clean_repository(tmp_path: Path):
    (tmp_path / "README.md").write_text("# Title\n\n## Section\n\nBody.\n", encoding="utf-8")
    assert main([str(tmp_path)]) == 0


def test_main_fails_on_error_severity(tmp_path: Path):
    (tmp_path / "README.md").write_text("# T\n\n[dead](missing.md)\n", encoding="utf-8")
    assert main([str(tmp_path)]) == 1


def test_main_fail_on_warning(tmp_path: Path):
    (tmp_path / "README.md").write_text("# A\n\n# B\n", encoding="utf-8")
    assert main([str(tmp_path)]) == 0
    assert main([str(tmp_path), "--fail-on", "warning"]) == 1


def test_main_missing_target_is_usage_error(tmp_path: Path):
    assert main([str(tmp_path / "nope")]) == 2


def test_main_json_output_shape(tmp_path: Path, capsys):
    (tmp_path / "README.md").write_text("# T\n\n[dead](missing.md)\n", encoding="utf-8")
    assert main([str(tmp_path), "--json"]) == 1
    payload = capsys.readouterr().out
    assert '"rule": "drift/broken-relative-link"' in payload


def test_exclude_path_skips_matches(tmp_path: Path):
    (tmp_path / "vendor").mkdir()
    (tmp_path / "vendor" / "README.md").write_text("# T\n\n[dead](missing.md)\n", encoding="utf-8")
    assert main([str(tmp_path)]) == 1
    assert main([str(tmp_path), "--exclude-path", "vendor/"]) == 0


if __name__ == "__main__":
    pytest.main([__file__])
