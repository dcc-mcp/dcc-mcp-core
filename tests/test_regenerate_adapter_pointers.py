"""Self-healing pointer regeneration.

The scanner can only report drift; these tests cover the half that clears it.
Every test that needs a repository builds one on disk and points the
regenerator at it through a local path, so the suite stays offline and hermetic.

The tests that would push or open a pull request do not do either. They stub the
``gh`` boundary and assert on what the regenerator *asked* for, which is the
only thing this repository can verify without touching 47 real ones.
"""

from __future__ import annotations

import json
from pathlib import Path
import subprocess
import sys

import pytest
from scripts import _pr_helpers as pr_helpers
from scripts import generate_adapter_pointer as generator
from scripts import regenerate_adapter_pointers as regen

REPO_ROOT = Path(__file__).resolve().parent.parent
CATALOG = REPO_ROOT / "dcc-mcp-catalog.yml"


@pytest.fixture(scope="module")
def catalog():
    return generator.load_catalog(CATALOG)


@pytest.fixture(scope="module")
def adapters(catalog):
    return generator.adapter_entries(catalog)


def _make_origin(tmp_path: Path, name: str, readme: str, *, branch: str = "main") -> str:
    """Create a git repository whose HEAD is ``branch`` and return its path."""
    origin = tmp_path / f"origin-{name}"
    origin.mkdir()
    (origin / "README.md").write_bytes(readme.encode("utf-8"))
    subprocess.run(["git", "init", "-q", "-b", branch, str(origin)], check=True)
    subprocess.run(["git", "-C", str(origin), "add", "README.md"], check=True)
    subprocess.run(
        [
            "git",
            "-C",
            str(origin),
            "-c",
            "user.email=tests@example.com",
            "-c",
            "user.name=Tests",
            "commit",
            "-q",
            "-m",
            "readme",
        ],
        check=True,
    )
    return str(origin)


def _entry(adapters, name):
    return next(e for e in adapters if e["name"] == name)


def _local_entry(adapters, name, origin_path: str) -> dict:
    """Build a catalog entry whose URL is a local path, so cloning stays offline."""
    entry = dict(_entry(adapters, name))
    entry["url"] = origin_path
    return entry


def _stale_readme(adapters, name: str) -> str:
    """Return a README carrying a pointer block that quotes an outdated host count."""
    entry = _entry(adapters, name)
    return generator.upsert_pointer("# adapter\n\n## Install\n", generator.render_pointer(entry, host_count=38))


class _FakeGitHub:
    """Record the ``gh`` and push calls the regenerator makes instead of making them."""

    def __init__(self, *, login: str = "bot", fork: str = "", existing_pr: str = ""):
        self.login = login
        self.fork = fork
        self.existing_pr = existing_pr
        self.created: list[dict] = []
        self.updated: list[dict] = []
        self.pushed: list[str] = []

    def install(self, monkeypatch) -> None:
        monkeypatch.setattr(pr_helpers, "authenticated_login", lambda **kw: self.login)
        monkeypatch.setattr(pr_helpers, "ensure_fork", lambda upstream, **kw: self.fork or upstream)
        monkeypatch.setattr(pr_helpers, "find_open_pr", lambda *a, **k: self.existing_pr)
        monkeypatch.setattr(
            pr_helpers,
            "create_pr",
            lambda repo, *, base, head, title, body, **kw: (
                self.created.append({"repo": repo, "base": base, "head": head, "title": title, "body": body})
                or "https://github.com/example/pr/1"
            ),
        )
        monkeypatch.setattr(
            pr_helpers,
            "update_pr",
            lambda repo, url, *, title, body, **kw: self.updated.append(
                {"repo": repo, "url": url, "title": title, "body": body}
            ),
        )
        monkeypatch.setattr(
            regen,
            "_commit_and_push",
            lambda repo_dir, branch, push_target, *, timeout, **kw: self.pushed.append(f"{push_target}:{branch}"),
        )


# --- guard rail ------------------------------------------------------------


def test_unrelated_working_tree_changes_are_refused(adapters, tmp_path, monkeypatch):
    """A repository that changed more than README.md is left alone, not committed.

    This is the regression net for the guard rail the PR description calls a core
    safety property, so it has to actually reach the guard. An earlier revision
    asserted `ERROR or REGENERATED`, which passed both before and after the guard
    was disabled -- it was verifying nothing.
    """
    name = "dcc-mcp-krita"
    origin = _make_origin(tmp_path, "unrelated", _stale_readme(adapters, name))

    # The clone must be dirty with a file this script did not create, and it has
    # to be dirty *before* the guard runs `git status`. So the seed is injected
    # right after the clone rather than at the push step.
    original_clone = regen._clone
    seeded = []

    def clone_then_seed(url, dest, branch, *, timeout):
        original_clone(url, dest, branch, timeout=timeout)
        (dest / "unrelated.txt").write_text("not created by the generator\n", encoding="utf-8")
        seeded.append(dest)

    fake = _FakeGitHub(login="bot", fork="bot/dcc-mcp-krita")
    fake.install(monkeypatch)
    monkeypatch.setattr(regen, "_clone", clone_then_seed)

    row = regen.regenerate_repository(
        _local_entry(adapters, name, origin),
        0,
        host_count=len(adapters),
        workdir=tmp_path / "work",
        timeout=60,
        branch_prefix=regen.BRANCH_PREFIX,
        dry_run=False,
        source_ref="dcc-mcp/dcc-mcp-core@dcc-mcp-catalog.yml",
    )

    assert row["status"] == regen.ERROR
    assert "unrelated" in row["detail"]
    # Nothing may be pushed and no PR opened when the guard fires.
    assert fake.pushed == []
    assert fake.created == [] and fake.updated == []


def test_the_guard_rail_test_really_covers_the_guard(adapters, tmp_path, monkeypatch):
    """Metatest: widening the guard's allow-list must flip the test above.

    Without this, the guard-rail test could silently rot back into an assertion
    that passes either way. It is cheap: it only re-runs one repository.
    """
    source = Path(regen.__file__).read_text(encoding="utf-8")
    assert "if unexpected:" in source

    name = "dcc-mcp-krita"
    origin = _make_origin(tmp_path, "metatest", _stale_readme(adapters, name))

    original_clone = regen._clone

    def clone_then_seed(url, dest, branch, *, timeout):
        original_clone(url, dest, branch, timeout=timeout)
        (dest / "unrelated.txt").write_text("not created by the generator\n", encoding="utf-8")

    fake = _FakeGitHub(login="bot", fork="bot/dcc-mcp-krita")
    fake.install(monkeypatch)
    monkeypatch.setattr(regen, "_clone", clone_then_seed)

    # With the guard's allow-list widened to accept the seeded file, the same run
    # must NOT report an error -- the opposite of what the real test asserts.
    monkeypatch.setattr(regen, "ALLOWED_CHANGED_FILES", ("README.md", "unrelated.txt"))
    row = regen.regenerate_repository(
        _local_entry(adapters, name, origin),
        0,
        host_count=len(adapters),
        workdir=tmp_path / "work",
        timeout=60,
        branch_prefix=regen.BRANCH_PREFIX,
        dry_run=False,
        source_ref="dcc-mcp/dcc-mcp-core@dcc-mcp-catalog.yml",
    )
    assert row["status"] != regen.ERROR


def test_dry_run_touches_nothing_and_reports_what_it_would_do(adapters, tmp_path):
    """A dry run writes no files, pushes nothing and opens no PR."""
    name = "dcc-mcp-krita"
    readme = _stale_readme(adapters, name)
    origin = _make_origin(tmp_path, "dryrun", readme)
    before = (Path(origin) / "README.md").read_bytes()

    fake = _FakeGitHub()
    row = regen.regenerate_repository(
        _local_entry(adapters, name, origin),
        0,
        host_count=len(adapters),
        workdir=tmp_path / "work",
        timeout=60,
        branch_prefix=regen.BRANCH_PREFIX,
        dry_run=True,
        source_ref="dcc-mcp/dcc-mcp-core@dcc-mcp-catalog.yml",
    )

    assert row["status"] == regen.REGENERATED
    assert row["action"] == "would-open-pr"
    assert (Path(origin) / "README.md").read_bytes() == before
    assert fake.pushed == []


# --- PR lifecycle ----------------------------------------------------------


def test_a_stale_repository_opens_a_pull_request(adapters, tmp_path, monkeypatch):
    """A drifted adapter gets one PR against the repository's default branch."""
    name = "dcc-mcp-krita"
    origin = _make_origin(tmp_path, "openpr", _stale_readme(adapters, name))
    fake = _FakeGitHub(login="bot", fork="bot/dcc-mcp-krita")
    fake.install(monkeypatch)

    row = regen.regenerate_repository(
        _local_entry(adapters, name, origin),
        0,
        host_count=len(adapters),
        workdir=tmp_path / "work",
        timeout=60,
        branch_prefix=regen.BRANCH_PREFIX,
        dry_run=False,
        source_ref="dcc-mcp/dcc-mcp-core@dcc-mcp-catalog.yml",
    )

    assert row["status"] == regen.REGENERATED
    assert row["action"] == "opened-pr"
    assert row["pr"] == "https://github.com/example/pr/1"
    assert len(fake.created) == 1
    assert fake.created[0]["base"] == "main"
    # A fork is pushed to, and the PR head is qualified with the fork's owner so
    # GitHub can match a cross-repository branch.
    assert fake.created[0]["head"].startswith("bot:")
    assert str(len(adapters)) in fake.created[0]["body"]


def test_an_existing_pull_request_is_refreshed_not_duplicated(adapters, tmp_path, monkeypatch):
    """A second run updates the open PR instead of opening another one."""
    name = "dcc-mcp-krita"
    origin = _make_origin(tmp_path, "refresh", _stale_readme(adapters, name))
    fake = _FakeGitHub(login="bot", fork="bot/dcc-mcp-krita", existing_pr="https://github.com/example/pr/7")
    fake.install(monkeypatch)

    row = regen.regenerate_repository(
        _local_entry(adapters, name, origin),
        0,
        host_count=len(adapters),
        workdir=tmp_path / "work",
        timeout=60,
        branch_prefix=regen.BRANCH_PREFIX,
        dry_run=False,
        source_ref="dcc-mcp/dcc-mcp-core@dcc-mcp-catalog.yml",
    )

    assert row["action"] == "updated-pr"
    assert row["pr"] == "https://github.com/example/pr/7"
    assert fake.created == []
    assert len(fake.updated) == 1


def test_a_current_repository_opens_nothing(adapters, tmp_path, monkeypatch):
    """The steady state: an adapter that already matches produces no PR."""
    name = "dcc-mcp-krita"
    entry = _entry(adapters, name)
    readme = generator.upsert_pointer("# adapter\n", generator.render_pointer(entry, host_count=len(adapters)))
    origin = _make_origin(tmp_path, "current", readme)
    fake = _FakeGitHub()
    fake.install(monkeypatch)

    row = regen.regenerate_repository(
        _local_entry(adapters, name, origin),
        0,
        host_count=len(adapters),
        workdir=tmp_path / "work",
        timeout=60,
        branch_prefix=regen.BRANCH_PREFIX,
        dry_run=False,
        source_ref="dcc-mcp/dcc-mcp-core@dcc-mcp-catalog.yml",
    )

    assert row["status"] == regen.CURRENT
    assert row["action"] == "none"
    assert fake.created == [] and fake.updated == [] and fake.pushed == []


def test_the_branch_name_is_stable_across_runs(adapters):
    """The same catalog state yields the same branch, so a rerun refreshes one PR."""
    first = pr_helpers.branch_slug(regen.BRANCH_PREFIX, regen._fingerprint(len(adapters)))
    second = pr_helpers.branch_slug(regen.BRANCH_PREFIX, regen._fingerprint(len(adapters)))
    assert first == second
    assert " " not in first and "//" not in first


def test_the_branch_name_survives_a_catalog_count_change():
    """A catalog change must not move the branch, or it orphans the open PR.

    An earlier revision keyed the branch on the host count, so 47 -> 48 opened a
    second PR beside the first and left the old batch open with nothing closing
    it. The branch is one per repository for the life of the automation.
    """
    assert regen._fingerprint(47) == regen._fingerprint(48)
    assert regen._fingerprint(47) == regen._fingerprint(99)
    # And the rendered PR body still promises the refresh-in-place behaviour the
    # stable branch is what makes true.
    assert "refreshes this PR instead of opening another one" in _body_for_host_count(47)


def _body_for_host_count(host_count: int) -> str:
    return regen.render_pr_body({"name": "dcc-mcp-example"}, host_count=host_count, source_ref="dcc-mcp/core@x.yml")


# --- pure helpers ----------------------------------------------------------


def test_repo_slug_strips_the_url_prefixes():
    assert regen._repo_slug("https://github.com/dcc-mcp/dcc-mcp-maya.git") == "dcc-mcp/dcc-mcp-maya"
    assert regen._repo_slug("https://github.com/dcc-mcp/dcc-mcp-maya") == "dcc-mcp/dcc-mcp-maya"
    assert regen._repo_slug("git@github.com:dcc-mcp/dcc-mcp-maya.git") == "dcc-mcp/dcc-mcp-maya"


def test_branch_slug_is_ref_safe():
    assert pr_helpers.branch_slug("docs/x-", "a b/c!!") == "docs/x-a-b-c"


def test_repo_owner_reads_the_owner_half():
    assert pr_helpers.repo_owner("dcc-mcp/dcc-mcp-maya") == "dcc-mcp"
    assert pr_helpers.repo_owner("nope") == ""


def test_an_existing_fork_is_not_an_error(monkeypatch):
    """`gh repo fork` exits non-zero with 'already exists'; that is success here."""
    calls = []

    def fake_run(args, **kwargs):
        calls.append(args)
        if args[:3] == ["gh", "repo", "fork"]:
            return subprocess.CompletedProcess(args, 1, "", "loonghao/x already exists\n")
        return subprocess.CompletedProcess(args, 0, "loonghao/dcc-mcp-maya\n", "")

    monkeypatch.setattr(pr_helpers, "authenticated_login", lambda **kw: "loonghao")
    monkeypatch.setattr(pr_helpers, "_run", fake_run)

    assert pr_helpers.ensure_fork("dcc-mcp/dcc-mcp-maya") == "loonghao/dcc-mcp-maya"
    fork_call = calls[0]
    # `--clone=false`, not `--clone false`: the spaced form is read as a clone
    # destination named "false".
    assert "--clone=false" in fork_call


def test_a_fork_failure_that_is_not_already_exists_still_raises(monkeypatch):
    def fake_run(args, **kwargs):
        return subprocess.CompletedProcess(args, 1, "", "permission denied\n")

    monkeypatch.setattr(pr_helpers, "authenticated_login", lambda **kw: "loonghao")
    monkeypatch.setattr(pr_helpers, "_run", fake_run)

    with pytest.raises(pr_helpers.PrError, match="gh repo fork failed"):
        pr_helpers.ensure_fork("dcc-mcp/dcc-mcp-maya")


def test_the_branch_prefix_cannot_collide_with_a_docs_branch():
    """A fork carrying a branch literally named `docs` rejects `docs/<sub>`."""
    assert not regen.BRANCH_PREFIX.startswith("docs/")


def test_find_open_pr_matches_on_the_bare_branch_name(monkeypatch):
    """`--head` rejects the `owner:branch` form used to create a cross-repo PR."""
    seen = {}

    def fake_run(args, **kwargs):
        seen["head"] = args[args.index("--head") + 1]
        return subprocess.CompletedProcess(args, 0, '[{"url":"https://example/pr/9"}]', "")

    monkeypatch.setattr(pr_helpers, "_run", fake_run)

    url = pr_helpers.find_open_pr("dcc-mcp/x", "loonghao:chore/refresh-catalog-pointer-47-adapters", "main")
    assert url == "https://example/pr/9"
    assert seen["head"] == "chore/refresh-catalog-pointer-47-adapters"


def test_an_unrelated_change_is_not_allowed():
    assert regen.ALLOWED_CHANGED_FILES == ("README.md",)


# --- CLI -------------------------------------------------------------------


def test_cli_dry_run_reports_pending_work_with_exit_1(adapters, tmp_path):
    """A dry run with work to do exits 1, so a schedule can tell 'would act' from 'clean'."""
    name = "dcc-mcp-krita"
    origin = _make_origin(tmp_path, "clidr", _stale_readme(adapters, name))
    entry = _local_entry(adapters, name, origin)
    monkeypatch = pytest.MonkeyPatch()
    monkeypatch.setattr(regen, "generator", regen.generator)
    monkeypatch.setattr(
        regen.generator,
        "load_catalog",
        lambda path: {"entries": [entry]},
    )

    code = regen.main(
        [
            "--dry-run",
            "--json-out",
            str(tmp_path / "regen.json"),
            "--workdir",
            str(tmp_path / "cliwork"),
        ]
    )
    monkeypatch.undo()

    assert code == 1
    payload = json.loads((tmp_path / "regen.json").read_text(encoding="utf-8"))
    assert payload["dry_run"] is True
    assert payload["results"][0]["status"] == regen.REGENERATED


def test_cli_rejects_an_unknown_adapter_name(tmp_path):
    assert regen.main(["--only", "no-such-adapter"]) == 2
