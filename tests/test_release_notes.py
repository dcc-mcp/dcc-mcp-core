"""Release-notes cross-check gate contract tests."""

from __future__ import annotations

import importlib.util
from pathlib import Path
import subprocess
import sys

import pytest

from conftest import REPO_ROOT

SCRIPT_PATH = REPO_ROOT / "scripts" / "ci" / "check_release_notes.py"
RELEASE_PR_GUARD_WORKFLOW = REPO_ROOT / ".github" / "workflows" / "release-please-pr-guard.yml"
RELEASE_WORKFLOW = REPO_ROOT / ".github" / "workflows" / "release.yml"
HIDDEN_TYPES = frozenset({"style", "chore", "test", "ci", "build"})

UNTYPED_SHA = "521df1fa2156d308b240228fd8ff4c11199bc0c9"
POST_NOTES_SHAS = (
    "c43691079dbbc696f9e07bcac10e1596a50970eb",
    "dc0e6c213d2b86b145110aba713b57c96f9f5853",
)


def _load_checker_module():
    spec = importlib.util.spec_from_file_location("check_release_notes", SCRIPT_PATH)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    # dataclasses resolves its string annotations through sys.modules, so the
    # module must be registered before its dataclasses are created.
    sys.modules["check_release_notes"] = module
    spec.loader.exec_module(module)
    return module


def _config(root: Path, sections: list[dict]) -> Path:
    import json

    path = root / "release-please-config.json"
    path.write_text(json.dumps({"changelog-sections": sections}), encoding="utf-8")
    return path


def _default_config(root: Path) -> Path:
    return _config(
        root,
        [
            {"type": "feat", "section": "Features", "hidden": False},
            {"type": "fix", "section": "Bug Fixes", "hidden": False},
            {"type": "style", "section": "Styles", "hidden": True},
            {"type": "chore", "section": "Miscellaneous Chores", "hidden": True},
            {"type": "test", "section": "Tests", "hidden": True},
            {"type": "ci", "section": "Continuous Integration", "hidden": True},
            {"type": "build", "section": "Build System", "hidden": True},
        ],
    )


def _tag_present(name: str) -> bool:
    completed = subprocess.run(
        ["git", "-C", str(REPO_ROOT), "rev-parse", "-q", "--verify", name],
        capture_output=True,
        text=True,
    )
    return completed.returncode == 0


# ── type parsing ────────────────────────────────────────────────────────────


@pytest.mark.parametrize(
    ("subject", "expected"),
    [
        ("feat(catalog): add sop_version", "feat"),
        ("fix(gateway)!: drop rows", "fix"),
        ("docs: correct the caveats", "docs"),
        ("ci(release): run release-please on a daily batch window (#2501)", "ci"),
        ("Publish adapter-install-sop-v2 and freeze the v1 schema artifact", None),
        ("", None),
        ("521df1fa Publish adapter-install-sop-v2", None),
    ],
)
def test_commit_type_reads_the_conventional_prefix(subject: str, expected: str | None) -> None:
    checker = _load_checker_module()

    assert checker.commit_type(subject) == expected


@pytest.mark.parametrize(
    ("subject", "expected"),
    [
        ("chore(main): release 0.20.34 (#2505)", "0.20.34"),
        ("chore(release): release 1.2.3", "1.2.3"),
        ("chore(main): release 0.20.34", "0.20.34"),
        ("fix(gateway): report broken pages (#2548)", None),
        ("chore(main): refresh lockfiles", None),
    ],
)
def test_is_release_commit_reads_the_published_version(subject: str, expected: str | None) -> None:
    checker = _load_checker_module()

    assert checker.is_release_commit(subject) == expected


# ── hidden-type filtering ───────────────────────────────────────────────────


def test_load_hidden_types_matches_the_repository_config() -> None:
    checker = _load_checker_module()

    assert checker.load_hidden_types(REPO_ROOT / "release-please-config.json") == HIDDEN_TYPES


def test_load_hidden_types_rejects_a_config_without_hidden_sections(tmp_path: Path) -> None:
    checker = _load_checker_module()
    config = _config(tmp_path, [{"type": "feat", "section": "Features", "hidden": False}])

    with pytest.raises(checker.ReleaseNotesError):
        checker.load_hidden_types(config)


def test_load_hidden_types_rejects_a_missing_config(tmp_path: Path) -> None:
    checker = _load_checker_module()

    with pytest.raises(checker.ReleaseNotesError):
        checker.load_hidden_types(tmp_path / "release-please-config.json")


def test_hidden_type_commits_are_skipped_instead_of_reported() -> None:
    checker = _load_checker_module()
    commit = checker.Commit(
        sha="f2704a9a5e2f557e462cc40edca2dfc86bad38c7",
        subject="test(docs-lint): match the resume symbol on word boundaries",
    )

    report = checker.check_commits([commit], "", HIDDEN_TYPES)

    assert report.hidden == (commit,)
    assert report.failures == ()


def test_release_commit_is_skipped() -> None:
    checker = _load_checker_module()
    commit = checker.Commit(
        sha="4f9114cb6b4fe71bb9c56ea7a4d139e44a592525", subject="chore(main): release 0.20.34 (#2505)"
    )

    report = checker.check_commits([commit], "", HIDDEN_TYPES)

    assert report.release_commits == (commit,)
    assert report.failures == ()


# ── sha matching ────────────────────────────────────────────────────────────


def test_is_documented_matches_the_full_sha_inside_a_commit_url() -> None:
    checker = _load_checker_module()
    notes = "* **catalog:** add sop_version ([b3180a3](https://github.com/o/r/commit/b3180a3941415e67520b10a5ad1eb4a625deea2b))"

    assert checker.is_documented("b3180a3941415e67520b10a5ad1eb4a625deea2b", notes) is True


def test_is_documented_matches_a_short_sha_label() -> None:
    checker = _load_checker_module()
    notes = "* **fix(skills):** bound list_skills pages ([#2547](https://github.com/o/r/pull/2547)) ([dc0e6c2](https://github.com/o/r/commit/dc0e6c213d2b86b145110aba713b57c96f9f5853))"

    assert checker.is_documented("dc0e6c213d2b86b145110aba713b57c96f9f5853", notes) is True


def test_is_documented_rejects_an_absent_sha() -> None:
    checker = _load_checker_module()

    assert (
        checker.is_documented("c43691079dbbc696f9e07bcac10e1596a50970eb", "* **catalog:** add sop_version ([b3180a3])")
        is False
    )


def test_is_documented_rejects_a_prefix_shorter_than_the_minimum() -> None:
    checker = _load_checker_module()

    # Six characters is below MIN_ABBREV, so an unrelated note must not match.
    assert checker.is_documented("c43691079dbbc696f9e07bcac10e1596a50970eb", "see c43691 for details") is False


def test_is_documented_rejects_a_non_hex_sha() -> None:
    checker = _load_checker_module()

    with pytest.raises(checker.ReleaseNotesError):
        checker.is_documented("not-a-sha", "notes")


# ── notes extraction ────────────────────────────────────────────────────────


def test_changelog_section_stops_at_the_next_version() -> None:
    checker = _load_checker_module()
    text = (
        "# Changelog\n\n"
        "## [0.20.34](https://github.com/o/r/compare/v0.20.33...v0.20.34) (2026-09-22)\n\n"
        "### Features\n\n* **catalog:** add sop_version ([b3180a3])\n\n"
        "## [0.20.33](https://github.com/o/r/compare/v0.20.32...v0.20.33) (2026-09-19)\n\n"
        "### Bug Fixes\n\n* **server:** bound translate waits ([8099504])\n"
    )

    section = checker.changelog_section(text, "0.20.34")

    assert section is not None
    assert "b3180a3" in section
    assert "8099504" not in section


def test_changelog_section_returns_none_for_an_unknown_version(tmp_path: Path) -> None:
    checker = _load_checker_module()

    assert checker.changelog_section("# Changelog\n\n## [0.20.33](https://example.com)\n", "9.9.9") is None


def test_previous_version_reads_the_compare_link() -> None:
    checker = _load_checker_module()
    text = "## [0.20.34](https://github.com/o/r/compare/v0.20.33...v0.20.34) (2026-09-22)\n"

    assert checker.previous_version(text, "0.20.34") == "v0.20.33"


def test_newest_version_reads_the_first_heading() -> None:
    checker = _load_checker_module()
    text = (
        "# Changelog\n\n"
        "## [0.20.34](https://github.com/o/r/compare/v0.20.33...v0.20.34) (2026-09-22)\n\n"
        "## [0.20.33](https://github.com/o/r/compare/v0.20.32...v0.20.33) (2026-09-19)\n"
    )

    assert checker.newest_version(text) == "0.20.34"


def test_newest_version_returns_none_for_a_changelog_without_versions() -> None:
    checker = _load_checker_module()

    assert checker.newest_version("# Changelog\n\nNo releases yet.\n") is None


# ── failure classification ──────────────────────────────────────────────────


def test_untyped_commit_is_reported_as_mechanism_a() -> None:
    checker = _load_checker_module()
    commit = checker.Commit(sha=UNTYPED_SHA, subject="Publish adapter-install-sop-v2 and freeze the v1 schema artifact")

    report = checker.check_commits([commit], "", HIDDEN_TYPES, version="0.20.34")

    assert report.untyped == (commit,)
    assert report.undocumented == ()
    errors = checker.format_errors(report)
    assert len(errors) == 1
    assert "[A:" in errors[0]
    assert "521df1fa" in errors[0]
    assert "0.20.34" in errors[0]


def test_documented_commit_is_not_reported() -> None:
    checker = _load_checker_module()
    commit = checker.Commit(
        sha="b3180a3941415e67520b10a5ad1eb4a625deea2b",
        subject="feat(catalog): add sop_version and sop_schema_digest to CatalogInstall (#2518)",
    )
    notes = "* **catalog:** add sop_version ([b3180a3](https://github.com/o/r/commit/b3180a3941415e67520b10a5ad1eb4a625deea2b))"

    report = checker.check_commits([commit], notes, HIDDEN_TYPES, version="0.20.34")

    assert report.checked == (commit,)
    assert report.failures == ()


def test_visible_commit_missing_from_notes_is_reported_as_mechanism_b() -> None:
    checker = _load_checker_module()
    commit = checker.Commit(
        sha=POST_NOTES_SHAS[0],
        subject="fix(gateway): report broken list_skills pages instead of dropping rows (#2548)",
    )
    notes = "* **catalog:** add sop_version ([b3180a3])\n"

    report = checker.check_commits([commit], notes, HIDDEN_TYPES, version="0.20.34")

    assert report.undocumented == (commit,)
    errors = checker.format_errors(report)
    assert len(errors) == 1
    assert "[B:" in errors[0]
    assert "c4369107" in errors[0]
    assert "PR #2548" in errors[0]


def _write_git_repo(tmp_path: Path, subject: str) -> None:
    """Create a two-commit repository whose tip carries ``subject``."""
    subprocess.run(["git", "init", "-q"], cwd=str(tmp_path), check=True, capture_output=True)
    subprocess.run(["git", "config", "user.email", "ci@example.com"], cwd=str(tmp_path), check=True)
    subprocess.run(["git", "config", "user.name", "CI"], cwd=str(tmp_path), check=True)
    (tmp_path / "file.txt").write_text("seed\n", encoding="utf-8")
    subprocess.run(["git", "add", "file.txt"], cwd=str(tmp_path), check=True)
    subprocess.run(["git", "commit", "-q", "-m", "chore: seed"], cwd=str(tmp_path), check=True)
    (tmp_path / "file.txt").write_text("seed\nmore\n", encoding="utf-8")
    subprocess.run(["git", "add", "file.txt"], cwd=str(tmp_path), check=True)
    subprocess.run(["git", "commit", "-q", "-m", subject], cwd=str(tmp_path), check=True)


def test_read_commits_decodes_non_ascii_subjects_as_utf8(tmp_path: Path) -> None:
    """Subjects hold typographic characters; decoding must not follow the locale.

    A non-UTF-8 console turned the em dash in ``8a861522`` into a
    UnicodeDecodeError whose exit code was indistinguishable from a real notes
    gap, so read_commits pins UTF-8 instead of inheriting the host encoding.
    """
    checker = _load_checker_module()
    _write_git_repo(tmp_path, "feat(verification): behavior verification contract \u2014 state export")

    commits = checker.read_commits(tmp_path, "HEAD~1", "HEAD")

    assert len(commits) == 1
    assert commits[0].subject.endswith("contract \u2014 state export")


def test_commit_pr_label_falls_back_when_no_pr_is_referenced() -> None:
    checker = _load_checker_module()

    assert checker.Commit(sha=UNTYPED_SHA, subject="Publish adapter-install-sop-v2").pr_label == "no PR reference"


def test_report_failures_orders_mechanism_a_before_b() -> None:
    checker = _load_checker_module()
    untyped = checker.Commit(sha=UNTYPED_SHA, subject="Publish adapter-install-sop-v2")
    undocumented = checker.Commit(
        sha=POST_NOTES_SHAS[1],
        subject="fix(skills): bound list_skills pages so multi-DCC context stays flat (#2547)",
    )

    report = checker.check_commits([undocumented, untyped], "", HIDDEN_TYPES, version="0.20.34")

    assert report.failures == (untyped, undocumented)


def test_main_reports_errors_and_returns_failure(tmp_path: Path, capsys) -> None:
    checker = _load_checker_module()
    _config(
        tmp_path,
        [
            {"type": "feat", "section": "Features", "hidden": False},
            {"type": "chore", "section": "Miscellaneous Chores", "hidden": True},
        ],
    )
    (tmp_path / "CHANGELOG.md").write_text(
        "## [0.20.34](https://github.com/o/r/compare/v0.20.33...v0.20.34) (2026-09-22)\n\n### Features\n\n"
        "* **catalog:** add sop_version ([b3180a3])\n",
        encoding="utf-8",
    )
    notes = tmp_path / "notes.md"
    notes.write_text(
        "## [0.20.34](https://github.com/o/r/compare/v0.20.33...v0.20.34) (2026-09-22)\n\n"
        "### Features\n\n* **catalog:** add sop_version ([b3180a3])\n",
        encoding="utf-8",
    )
    # A private repository with a single untyped commit and no release commit.
    subprocess.run(["git", "init", "-q"], cwd=str(tmp_path), check=True, capture_output=True)
    subprocess.run(["git", "config", "user.email", "ci@example.com"], cwd=str(tmp_path), check=True)
    subprocess.run(["git", "config", "user.name", "CI"], cwd=str(tmp_path), check=True)
    (tmp_path / "file.txt").write_text("seed\n", encoding="utf-8")
    subprocess.run(["git", "add", "file.txt"], cwd=str(tmp_path), check=True)
    subprocess.run(["git", "commit", "-q", "-m", "feat(catalog): add sop_version"], cwd=str(tmp_path), check=True)
    subprocess.run(["git", "tag", "v0.20.33"], cwd=str(tmp_path), check=True)
    (tmp_path / "file.txt").write_text("seed\nmore\n", encoding="utf-8")
    subprocess.run(["git", "add", "file.txt"], cwd=str(tmp_path), check=True)
    subprocess.run(["git", "commit", "-q", "-m", "Publish adapter-install-sop-v2"], cwd=str(tmp_path), check=True)
    subprocess.run(["git", "tag", "v0.20.34"], cwd=str(tmp_path), check=True)

    assert checker.main(["--root", str(tmp_path), "--version", "0.20.34", "--notes-file", str(notes)]) == 1
    assert "[A:untyped-commit]" in capsys.readouterr().err


# ── repository replays ──────────────────────────────────────────────────────


@pytest.mark.skipif(not _tag_present("v0.20.34"), reason="v0.20.34 tag is not available in this checkout")
def test_replay_0_20_34_reports_the_three_known_gaps(capsys) -> None:
    checker = _load_checker_module()

    exit_code = checker.main(
        ["--root", str(REPO_ROOT), "--prev-tag", "v0.20.33", "--release-tag", "v0.20.34", "--version", "0.20.34"]
    )
    captured = capsys.readouterr()

    assert exit_code == 1
    errors = captured.err
    # Mechanism A: the untyped squash title release-please dropped entirely.
    assert "521df1fa" in errors
    assert errors.count("[A:untyped-commit]") == 1
    # Mechanism B: both commits that landed after the notes were regenerated.
    assert "c436910" in errors
    assert "dc0e6c2" in errors
    assert errors.count("[B:post-notes-commit]") == 2


@pytest.mark.skipif(not _tag_present("v0.20.34"), reason="v0.20.34 tag is not available in this checkout")
def test_replay_0_20_34_stays_quiet_about_hidden_types(capsys) -> None:
    checker = _load_checker_module()
    commits = checker.read_commits(REPO_ROOT, "v0.20.33", "v0.20.34")
    section = checker.changelog_section((REPO_ROOT / "CHANGELOG.md").read_text(encoding="utf-8"), "0.20.34")
    assert section is not None
    report = checker.check_commits(
        commits, section, checker.load_hidden_types(REPO_ROOT / "release-please-config.json")
    )

    assert len(report.hidden) == 13

    checker.main(
        ["--root", str(REPO_ROOT), "--prev-tag", "v0.20.33", "--release-tag", "v0.20.34", "--version", "0.20.34"]
    )
    errors = capsys.readouterr().err

    for commit in report.hidden:
        assert commit.short_sha not in errors


@pytest.mark.skipif(not _tag_present("v0.20.33"), reason="v0.20.33 tag is not available in this checkout")
def test_replay_0_20_33_is_a_clean_baseline() -> None:
    checker = _load_checker_module()

    assert (
        checker.main(
            ["--root", str(REPO_ROOT), "--prev-tag", "v0.20.32", "--release-tag", "v0.20.33", "--version", "0.20.33"]
        )
        == 0
    )


@pytest.mark.skipif(not _tag_present("v0.20.23"), reason="v0.20.23 tag is not available in this checkout")
def test_replay_0_20_23_is_clean_across_a_wide_window(capsys) -> None:
    """Noise guard: a 90-commit window with 8 hidden commits must stay silent.

    The 0.20.32..0.20.33 baseline carries a single user-visible commit, so it
    cannot detect a gate that simply reports everything it sees. This window is
    the real false-positive guard.
    """
    checker = _load_checker_module()

    assert (
        checker.main(
            ["--root", str(REPO_ROOT), "--prev-tag", "v0.20.22", "--release-tag", "v0.20.23", "--version", "0.20.23"]
        )
        == 0
    )

    commits = checker.read_commits(REPO_ROOT, "v0.20.22", "v0.20.23")
    section = checker.changelog_section((REPO_ROOT / "CHANGELOG.md").read_text(encoding="utf-8"), "0.20.23")
    assert section is not None
    report = checker.check_commits(
        commits, section, checker.load_hidden_types(REPO_ROOT / "release-please-config.json")
    )

    assert len(report.hidden) == 8
    assert len(report.checked) == 90
    assert report.failures == ()


@pytest.mark.skipif(not _tag_present("v0.20.34"), reason="v0.20.34 tag is not available in this checkout")
def test_release_body_addendum_clears_the_mechanism_b_gaps(tmp_path: Path, capsys) -> None:
    checker = _load_checker_module()
    addendum = tmp_path / "release-body.md"
    addendum.write_text(
        (
            (REPO_ROOT / "CHANGELOG.md").read_text(encoding="utf-8").split("## [0.20.33]")[0]
            + f"## Release notes addendum\n\n"
            f"* fix(skills) ([#2547](https://github.com/o/r/pull/2547)) ([dc0e6c2](https://github.com/o/r/commit/{POST_NOTES_SHAS[1]}))\n"
            f"* fix(gateway) ([#2548](https://github.com/o/r/pull/2548)) ([c436910](https://github.com/o/r/commit/{POST_NOTES_SHAS[0]}))\n"
        ),
        encoding="utf-8",
    )

    exit_code = checker.main(
        [
            "--root",
            str(REPO_ROOT),
            "--prev-tag",
            "v0.20.33",
            "--release-tag",
            "v0.20.34",
            "--version",
            "0.20.34",
            "--notes-file",
            str(addendum),
        ]
    )
    errors = capsys.readouterr().err

    # Only the untyped title remains: an addendum documents a post-notes commit
    # but cannot retroactively give it a conventional-commit type.
    assert exit_code == 1
    assert "[B:post-notes-commit]" not in errors
    assert "[A:untyped-commit]" in errors


# ── workflow wiring ─────────────────────────────────────────────────────────


def test_release_pr_guard_runs_the_notes_gate() -> None:
    workflow = RELEASE_PR_GUARD_WORKFLOW.read_text(encoding="utf-8")

    assert "scripts/ci/check_release_notes.py" in workflow


def test_release_workflow_runs_the_notes_gate() -> None:
    workflow = RELEASE_WORKFLOW.read_text(encoding="utf-8")

    assert "check_release_notes.py" in workflow

    # The py37 grammar guarantee lives in the `py37 syntax check` CI lane, which
    # compiles scripts/ and tests/ with a real Python 3.7 interpreter. It is not
    # re-checked here: ast.parse's feature_version only exists on Python 3.8+,
    # so asserting it from inside the suite would fail on the py37 lane itself.
