"""Regression tests for the mechanical documentation contract checker."""

from __future__ import annotations

import json
from pathlib import Path

import pytest
from scripts.docs_lint import PLAYBOOK_MANIFEST_DEFAULT
from scripts.docs_lint import RepoIndex
from scripts.docs_lint import check_emoji
from scripts.docs_lint import check_links
from scripts.docs_lint import check_playbook_coverage
from scripts.docs_lint import check_structure
from scripts.docs_lint import check_symbols
from scripts.docs_lint import fence_step
from scripts.docs_lint import load_playbook_manifest
from scripts.docs_lint import main
from scripts.docs_lint import playbook_matcher
from scripts.docs_lint import resume_matcher

REPO_ROOT = Path(__file__).resolve().parents[1]


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
# Content / playbook coverage
# --------------------------------------------------------------------------- #


def _write(root, name, text):
    """Write ``text`` to ``root/name``, creating parent directories."""
    path = root / name
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")
    return path


def _covered_repo(root, manifest="AGENTS.md\nllms.txt\n"):
    """Build a tree whose manifest entries all satisfy the coverage contract.

    The manifest goes to the canonical default location, so these fixtures
    exercise the same path `docs_lint.py` resolves on its own -- a manifest
    parked anywhere else would leave the default-path code untested.
    """
    _write(
        root,
        "AGENTS.md",
        "# Agents\n\n## Iteration Playbook\n\nReuse the script, then recover with `workflows_resume`.\n",
    )
    _write(root, "llms.txt", "- `workflows_resume` — recover an interrupted run\n\nSee the iteration playbook.\n")
    _write(root, PLAYBOOK_MANIFEST_DEFAULT, manifest)
    return root


def test_manifest_entries_that_cover_the_playbook_pass(tmp_path: Path):
    _covered_repo(tmp_path)
    entries, error = load_playbook_manifest(tmp_path / PLAYBOOK_MANIFEST_DEFAULT)
    assert error is None
    assert entries == ["AGENTS.md", "llms.txt"]
    assert check_playbook_coverage(tmp_path, entries) == {}


def test_missing_marker_and_missing_resume_are_separate_errors(tmp_path: Path):
    _write(tmp_path, "AGENTS.md", "# Agents\n\nReuse scripts. Recover with `workflows_resume`.\n")
    _write(tmp_path, "llms.txt", "## Iteration Playbook\n\nReuse the materialized file.\n")
    report = check_playbook_coverage(tmp_path, ["AGENTS.md", "llms.txt"])

    assert _rules(report[(tmp_path / "AGENTS.md").as_posix()]) == ["content/playbook-coverage-missing-marker"]
    assert _rules(report[(tmp_path / "llms.txt").as_posix()]) == ["content/playbook-coverage-missing-resume"]


def test_resume_matcher_rejects_near_miss_identifiers():
    # Substring matching let `workflows_resume_old` and `not_workflows_resume`
    # satisfy the gate: a renamed, prefixed, or suffixed symbol is not the tool
    # an agent can call, and passing it here is the false negative the rule
    # exists to prevent. The punctuation docs actually wrap the symbol in is
    # not a word character, so those forms must still count.
    matcher = resume_matcher("workflows_resume")
    for text in (
        "recover with `workflows_resume`",
        'call "workflows_resume" to continue',
        "see workflows_resume.",
        "run workflows_resume() now",
        # CJK is a Unicode word character, so \w boundaries would reject a
        # bilingual sentence that names the tool correctly. The docs in this
        # repo are bilingual; that is a worse failure than letting an exotic
        # `\u03b1workflows_resume` through.
        "\u8bf7\u8c03\u7528workflows_resume\u6765\u6062\u590d",
        "\uff08workflows_resume\uff09",
    ):
        assert matcher.search(text), text
    for text in (
        "workflows_resume_old",
        "not_workflows_resume",
        "xworkflows_resume",
        "workflows_resumex",
        "v2_workflows_resume",
    ):
        assert not matcher.search(text), text


def test_near_miss_resume_symbols_do_not_satisfy_coverage(tmp_path: Path):
    # End-to-end form of the above: the finding is reported, not swallowed.
    _write(tmp_path, "old.md", "## Iteration Playbook\n\nRecover with `workflows_resume_old`.\n")
    _write(tmp_path, "prefixed.md", "## Iteration Playbook\n\nRecover with not_workflows_resume.\n")
    report = check_playbook_coverage(tmp_path, ["old.md", "prefixed.md"])
    for name in ("old.md", "prefixed.md"):
        assert _rules(report[(tmp_path / name).as_posix()]) == ["content/playbook-coverage-missing-resume"]


def test_entry_without_either_signal_reports_both(tmp_path: Path):
    _write(tmp_path, "AGENTS.md", "# Agents\n\nNothing to see here.\n")
    report = check_playbook_coverage(tmp_path, ["AGENTS.md"])
    assert _rules(report[(tmp_path / "AGENTS.md").as_posix()]) == [
        "content/playbook-coverage-missing-marker",
        "content/playbook-coverage-missing-resume",
    ]


def test_manifest_entry_pointing_at_a_missing_file_is_an_error(tmp_path: Path):
    report = check_playbook_coverage(tmp_path, ["docs/guide/agents-reference.md"])
    rules = _rules(report[(tmp_path / "docs/guide/agents-reference.md").as_posix()])
    assert rules == ["content/playbook-coverage-missing-file"]


def test_manifest_ignores_comments_and_blank_lines(tmp_path: Path):
    _covered_repo(tmp_path, manifest="# agent-facing entry points\n\nAGENTS.md\n\nllms.txt\n")
    entries, error = load_playbook_manifest(tmp_path / PLAYBOOK_MANIFEST_DEFAULT)
    assert error is None
    assert entries == ["AGENTS.md", "llms.txt"]
    assert check_playbook_coverage(tmp_path, entries) == {}


def test_coverage_grows_by_editing_the_manifest_alone(tmp_path: Path):
    # The point of the manifest: a new entry point joins the gate with one
    # line of data and no change to the rule implementation.
    _covered_repo(tmp_path, manifest="AGENTS.md\nllms.txt\n")
    _write(tmp_path, "AI_AGENT_GUIDE.md", "# Guide\n\nNo playbook here.\n")
    assert check_playbook_coverage(tmp_path, ["AGENTS.md", "llms.txt"]) == {}

    _write(tmp_path, PLAYBOOK_MANIFEST_DEFAULT, "AGENTS.md\nllms.txt\nAI_AGENT_GUIDE.md\n")
    entries, _ = load_playbook_manifest(tmp_path / PLAYBOOK_MANIFEST_DEFAULT)
    report = check_playbook_coverage(tmp_path, entries)
    assert list(report) == [(tmp_path / "AI_AGENT_GUIDE.md").as_posix()]


def test_files_outside_the_manifest_are_never_checked(tmp_path: Path):
    _covered_repo(tmp_path, manifest="AGENTS.md\n")
    _write(tmp_path, "CHANGELOG.md", "# Changelog\n\nNo playbook, no resume tool, and that is fine.\n")
    entries, _ = load_playbook_manifest(tmp_path / PLAYBOOK_MANIFEST_DEFAULT)
    assert check_playbook_coverage(tmp_path, entries) == {}


def test_marker_matching_ignores_case_and_separator():
    matcher = playbook_matcher("iteration playbook")
    for text in (
        "Iteration Playbook",
        "iteration playbook",
        "Iteration-Playbook",
        "iteration_playbook",
        "the ITERATION\nplaybook section",
    ):
        assert matcher.search(text), text
    assert not matcher.search("reuse materialized scripts")


def test_unreadable_manifest_is_reported_not_swallowed(tmp_path: Path):
    entries, error = load_playbook_manifest(tmp_path / "missing.txt")
    assert entries == []
    assert error


def test_shipped_manifest_names_files_that_exist():
    # Guards path drift: renaming an agent-facing entry point must update the
    # manifest, otherwise the gate silently protects a file nobody reads.
    entries, error = load_playbook_manifest(REPO_ROOT / PLAYBOOK_MANIFEST_DEFAULT)
    assert error is None
    assert entries, "the shipped playbook manifest must not be empty"
    missing = [entry for entry in entries if not (REPO_ROOT / entry).is_file()]
    assert missing == []


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


def test_main_playbook_only_passes_on_covered_manifest(tmp_path: Path):
    _covered_repo(tmp_path)
    manifest = str(tmp_path / PLAYBOOK_MANIFEST_DEFAULT)
    assert main([str(tmp_path), "--playbook-only", "--playbook-manifest", manifest]) == 0


def test_main_playbook_only_fails_on_uncovered_entry(tmp_path: Path):
    _covered_repo(tmp_path)
    _write(tmp_path, "llms.txt", "- `workflows_run` — start a run\n")
    manifest = str(tmp_path / PLAYBOOK_MANIFEST_DEFAULT)
    assert main([str(tmp_path), "--playbook-only", "--playbook-manifest", manifest]) == 1


def test_main_playbook_only_skips_per_file_rules(tmp_path: Path):
    # Scoping is what lets CI gate the manifest without inheriting every
    # pre-existing structure/link finding in the tree.
    _covered_repo(tmp_path)
    _write(tmp_path, "broken.md", "# T\n\n[dead](missing.md)\n")
    manifest = str(tmp_path / PLAYBOOK_MANIFEST_DEFAULT)
    assert main([str(tmp_path), "--playbook-manifest", manifest]) == 1
    assert main([str(tmp_path), "--playbook-only", "--playbook-manifest", manifest]) == 0


def test_main_reports_a_missing_explicit_manifest(tmp_path: Path):
    _write(tmp_path, "README.md", "# T\n\nBody.\n")
    assert main([str(tmp_path), "--playbook-manifest", str(tmp_path / "gone.txt")]) == 1


def test_main_without_a_manifest_skips_the_coverage_pass(tmp_path: Path):
    # The default manifest is resolved against the lint root, so an unrelated
    # tree with no manifest is not held to a coverage set it never opted into.
    _write(tmp_path, "AGENTS.md", "# T\n\nNo playbook.\n")
    assert main([str(tmp_path)]) == 0


def test_full_run_merges_coverage_and_per_file_findings(tmp_path: Path, capsys):
    # A manifest entry is also an ordinary Markdown file, so its coverage
    # findings and its structure/link findings must both survive: the per-file
    # loop used to assign report[path] = findings and drop the coverage half.
    # CI cannot catch that regression -- --playbook-only skips the loop.
    _covered_repo(tmp_path)
    _write(tmp_path, "AGENTS.md", "# Agents\n\nNothing here.\n\n[dead](missing.md)\n")
    assert main([str(tmp_path), "--json"]) == 1
    payload = json.loads(capsys.readouterr().out)
    rules = sorted(f["rule"] for f in payload["findings"] if f["file"].endswith("AGENTS.md"))
    assert rules == [
        "content/playbook-coverage-missing-marker",
        "content/playbook-coverage-missing-resume",
        "drift/broken-relative-link",
    ]


def test_default_manifest_is_resolved_against_the_lint_root(tmp_path: Path):
    # Both directions matter: the default path is found relative to the lint
    # root, and a manifest located there is actually enforced. Asserting only
    # the clean case would still pass if the manifest were never read.
    _covered_repo(tmp_path)
    assert main([str(tmp_path)]) == 0

    _write(tmp_path, "AGENTS.md", "# Agents\n\nNothing about the playbook here.\n")
    assert main([str(tmp_path)]) == 1


if __name__ == "__main__":
    pytest.main([__file__])
