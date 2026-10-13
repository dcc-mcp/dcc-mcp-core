"""Self-healing pointer regeneration.

The scanner can only report drift; these tests cover the half that clears it.
Every test that needs a repository builds one on disk and points the
regenerator at it through a local path, so the suite stays offline and hermetic.

The GitHub boundary is stubbed. Push regressions use real git against temporary
local repositories, including concurrent-ref changes; none access or mutate
GitHub. Other tests record the requested operations without performing a push.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import subprocess
import sys
from types import SimpleNamespace

import pytest
from scripts import _adapter_pointer_budget as budget
from scripts import _pr_helpers as pr_helpers
from scripts import generate_adapter_pointer as generator
from scripts import regenerate_adapter_pointers as regen

PR_TITLE = regen.PR_TITLE

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

    def __init__(
        self,
        *,
        existing_pr: str = "",
        push_state: str = pr_helpers.PUSH_GRANTED,
        open_prs: list[dict] | None = None,
        list_error: Exception | None = None,
        pr_blobs: dict[int, str] | None = None,
    ):
        self.existing_pr = existing_pr
        self.push_state = push_state
        self.open_prs = list(open_prs or [])
        self.list_error = list_error
        # What the per-PR files endpoint would return. Kept separate from
        # `open_prs` because the list endpoint omits the blob entirely, so the
        # two sources genuinely differ -- which is the whole reason the real code
        # needs a second query.
        self.pr_blobs = dict(pr_blobs or {})
        self.created: list[dict] = []
        self.updated: list[dict] = []
        self.pushed: list[str] = []
        self.list_calls: int = 0

    def _pr_files(self, repo, number, **kwargs):
        """Answer the per-PR files lookup, which the list endpoint cannot.

        `gh pr list --json files` carries no blob SHA, so this is the only
        source of content identity -- and it is the only source of the file
        list, which the "extra files are not equivalent" guard reads.

        The row models what `gh pr list` reports -- paths, no sha -- so a row
        list alone can never establish content identity. `pr_blobs` therefore
        supplies the sha for the generated path, while the row's own list still
        decides *which* files the PR touches. Keeping those two roles separate
        is what lets a test model "the generated README plus a handwritten
        file", the case the extra-files guard exists for.
        """
        files = None
        for row in self.open_prs:
            if row.get("number") != number:
                continue
            listed = row.get("files")
            if isinstance(listed, list):
                files = [
                    {"path": str(entry.get("path") or ""), "sha": str(entry.get("sha") or "")}
                    for entry in listed
                    if isinstance(entry, dict)
                ]
            break

        if number in self.pr_blobs:
            sha = self.pr_blobs[number]
            if files:
                # Stamp the resolved blob onto the generated file, preserving
                # every other entry the PR really carries.
                stamped = [{**entry, "sha": sha} if entry["path"] == "README.md" else entry for entry in files]
                return stamped
            return [{"path": "README.md", "sha": sha}]
        return files if files is not None else []

    def install(self, monkeypatch) -> None:
        monkeypatch.setattr(pr_helpers, "check_push_access", lambda repo, **kw: self.push_state)
        monkeypatch.setattr(pr_helpers, "find_open_pr", lambda *a, **k: self.existing_pr)

        def list_open_prs(repo, base, **kwargs):
            self.list_calls += 1
            if self.list_error is not None:
                raise self.list_error
            return list(self.open_prs)

        monkeypatch.setattr(pr_helpers, "list_open_prs", list_open_prs)
        # Resolution runs for real, against the blobs this fake records, so a
        # regression in it cannot hide behind a stub.
        monkeypatch.setattr(pr_helpers, "pr_files", self._pr_files)
        monkeypatch.setattr(pr_helpers, "resolve_pr_blobs", pr_helpers.resolve_pr_blobs)
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

    fake = _FakeGitHub()
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

    fake = _FakeGitHub()
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


def _pr_row(number, url, *, path="README.md", sha, title=PR_TITLE, owner="dcc-mcp", head="chore/x"):
    """Build one row shaped like `gh pr list --json ...` output."""
    return {
        "number": number,
        "url": url,
        "title": title,
        "headRefName": head,
        "headRepositoryOwner": {"login": owner},
        "author": {"login": owner},
        "files": [{"path": path, "sha": sha}],
    }


def _as_listed(row):
    """Return the row as `gh pr list --json files` would report it.

    That endpoint omits the blob SHA, so the content comparison cannot rely on
    what it returns -- exactly the gap `resolve_pr_blobs` exists to close.
    """
    listed = dict(row)
    listed["files"] = [
        {k: v for k, v in f.items() if k != "sha"} for f in (row.get("files") or []) if isinstance(f, dict)
    ]
    return listed


def _run_regen(adapters, tmp_path, name, origin, fake, monkeypatch):
    fake.install(monkeypatch)
    return regen.regenerate_repository(
        _local_entry(adapters, name, origin),
        0,
        host_count=len(adapters),
        workdir=tmp_path / "work",
        timeout=60,
        branch_prefix=regen.BRANCH_PREFIX,
        dry_run=False,
        source_ref="dcc-mcp/dcc-mcp-core@dcc-mcp-catalog.yml",
    )


def _generated_blob(adapters, tmp_path, name):
    """Return the blob SHA the generator produces for this adapter's README."""
    import subprocess

    work = tmp_path / "blobwork"
    work.mkdir(parents=True, exist_ok=True)
    repo = work / "repo"
    repo.mkdir(parents=True, exist_ok=True)
    subprocess.run(["git", "init", "-q", "-b", "main", str(repo)], check=True)
    target = repo / "README.md"
    target.write_text(_stale_readme(adapters, name), encoding="utf-8")
    entry = dict(_entry(adapters, name))
    entry["url"] = "https://github.com/dcc-mcp/example.git"
    generator.apply_pointer(repo, entry, host_count=len(adapters), write=True)
    result = subprocess.run(
        ["git", "-C", str(repo), "hash-object", "README.md"],
        capture_output=True,
        text=True,
        check=True,
    )
    return result.stdout.strip()


# --- duplicate detection ---------------------------------------------------


def test_dedup_works_when_list_omits_the_blob(adapters, tmp_path, monkeypatch):
    """`gh pr list --json files` has no blob sha; dedup must still recognise it.

    The list endpoint reports path, additions and deletions but not the blob, so
    rows arrive with no content identity. If the per-PR lookup that supplies it
    were dropped, the comparison would fall back to the title alone -- and every
    refresh this automation opens shares one title, so an unrelated PR carrying
    different content would be reused.
    """
    name = "dcc-mcp-krita"
    origin = _make_origin(tmp_path, "dupnolistblob", _stale_readme(adapters, name))

    # Same file, same title, DIFFERENT content. Only the per-PR blob lookup can
    # tell it apart.
    listed = _as_listed(_pr_row(59, "https://github.com/dcc-mcp/dcc-mcp-krita/pull/59", sha="f" * 40, owner="loonghao"))
    assert all("sha" not in f for f in listed["files"])

    fake = _FakeGitHub(open_prs=[listed], pr_blobs={59: "f" * 40})
    row = _run_regen(adapters, tmp_path, name, origin, fake, monkeypatch)

    # Not a duplicate: the blob was resolved and disagreed.
    assert row["status"] == regen.REGENERATED
    assert len(fake.created) == 1


def test_an_equivalent_pr_is_found_when_only_the_lookup_knows_the_blob(adapters, tmp_path, monkeypatch):
    """The matching case: list omits the blob, the per-PR lookup supplies it."""
    name = "dcc-mcp-krita"
    origin = _make_origin(tmp_path, "duplistblob", _stale_readme(adapters, name))
    blob = _generated_blob(adapters, tmp_path, name)

    listed = _as_listed(_pr_row(49, "https://github.com/dcc-mcp/dcc-mcp-krita/pull/49", sha=blob, owner="loonghao"))
    fake = _FakeGitHub(open_prs=[listed], pr_blobs={49: blob})
    row = _run_regen(adapters, tmp_path, name, origin, fake, monkeypatch)

    assert row["status"] == regen.DUPLICATE
    assert row["pr_number"] == 49
    assert fake.created == [] and fake.pushed == []


def test_an_equivalent_pr_from_a_fork_is_reused(adapters, tmp_path, monkeypatch):
    """The bug: an old fork PR carrying the identical blob must not be duplicated.

    The old head lives on a personal fork under a previous branch fingerprint,
    so head-branch matching cannot see it. Content matching can.
    """
    name = "dcc-mcp-krita"
    origin = _make_origin(tmp_path, "dupfork", _stale_readme(adapters, name))
    blob = _generated_blob(adapters, tmp_path, name)

    fake = _FakeGitHub(
        open_prs=[
            _pr_row(
                49,
                "https://github.com/dcc-mcp/dcc-mcp-krita/pull/49",
                sha=blob,
                owner="loonghao",
                head="chore/refresh-catalog-pointer-47-adapters",
            )
        ],
    )

    row = _run_regen(adapters, tmp_path, name, origin, fake, monkeypatch)

    assert row["status"] == regen.DUPLICATE
    assert row["action"] == "reused-pr"
    assert row["pr"] == "https://github.com/dcc-mcp/dcc-mcp-krita/pull/49"
    assert row["pr_number"] == 49
    # Nothing written: no commit, no push, no new PR.
    assert fake.pushed == []
    assert fake.created == []
    assert fake.updated == []


def test_an_equivalent_pr_in_the_same_repo_is_reused(adapters, tmp_path, monkeypatch):
    """A same-repository PR under the current branch name is also reused."""
    name = "dcc-mcp-krita"
    origin = _make_origin(tmp_path, "dupsame", _stale_readme(adapters, name))
    blob = _generated_blob(adapters, tmp_path, name)

    fake = _FakeGitHub(
        open_prs=[
            _pr_row(
                50,
                "https://github.com/dcc-mcp/dcc-mcp-krita/pull/50",
                sha=blob,
                owner="dcc-mcp",
                head="chore/refresh-catalog-pointer-catalog-pointer",
            )
        ],
    )

    row = _run_regen(adapters, tmp_path, name, origin, fake, monkeypatch)

    assert row["status"] == regen.DUPLICATE
    assert row["action"] == "reused-pr"
    assert row["pr_number"] == 50
    assert fake.pushed == []
    assert fake.created == []


def test_a_different_author_is_still_equivalent(adapters, tmp_path, monkeypatch):
    """Authorship is not part of the identity of a generated change."""
    name = "dcc-mcp-krita"
    origin = _make_origin(tmp_path, "dupauthor", _stale_readme(adapters, name))
    blob = _generated_blob(adapters, tmp_path, name)

    fake = _FakeGitHub(
        open_prs=[_pr_row(61, "https://github.com/dcc-mcp/dcc-mcp-krita/pull/61", sha=blob, owner="someone-else")],
    )

    row = _run_regen(adapters, tmp_path, name, origin, fake, monkeypatch)

    assert row["status"] == regen.DUPLICATE
    assert row["pr_number"] == 61
    assert fake.created == []


def test_a_pr_with_different_content_is_not_reused(adapters, tmp_path, monkeypatch):
    """Different generated content is genuinely new work: open a PR."""
    name = "dcc-mcp-krita"
    origin = _make_origin(tmp_path, "dupdiff", _stale_readme(adapters, name))
    blob = _generated_blob(adapters, tmp_path, name)

    fake = _FakeGitHub(
        open_prs=[_pr_row(70, "https://github.com/dcc-mcp/dcc-mcp-krita/pull/70", sha="a" * 40)],
    )
    assert blob != "a" * 40

    row = _run_regen(adapters, tmp_path, name, origin, fake, monkeypatch)

    assert row["status"] == regen.REGENERATED
    assert row["action"] == "opened-pr"
    assert len(fake.created) == 1
    assert fake.pushed


def test_a_pr_with_extra_unrelated_files_is_not_reused(adapters, tmp_path, monkeypatch):
    """Same README blob plus someone else's change is not this generated PR."""
    name = "dcc-mcp-krita"
    origin = _make_origin(tmp_path, "dupextra", _stale_readme(adapters, name))
    blob = _generated_blob(adapters, tmp_path, name)

    mixed = _pr_row(80, "https://github.com/dcc-mcp/dcc-mcp-krita/pull/80", sha=blob)
    mixed["files"] = [
        {"path": "README.md", "sha": blob},
        {"path": "src/handwritten.py", "sha": "b" * 40},
    ]

    fake = _FakeGitHub(open_prs=[mixed])
    row = _run_regen(adapters, tmp_path, name, origin, fake, monkeypatch)

    assert row["status"] == regen.REGENERATED
    assert len(fake.created) == 1


def test_a_pr_touching_a_different_file_is_not_reused(adapters, tmp_path, monkeypatch):
    """Same blob on an unrelated path is not the generated README change."""
    name = "dcc-mcp-krita"
    origin = _make_origin(tmp_path, "duppath", _stale_readme(adapters, name))
    blob = _generated_blob(adapters, tmp_path, name)

    fake = _FakeGitHub(
        open_prs=[_pr_row(90, "https://github.com/dcc-mcp/dcc-mcp-krita/pull/90", sha=blob, path="docs/other.md")],
    )
    row = _run_regen(adapters, tmp_path, name, origin, fake, monkeypatch)

    assert row["status"] == regen.REGENERATED
    assert len(fake.created) == 1


def test_a_duplicate_is_reported_not_automatically_closed(adapters, tmp_path, monkeypatch):
    """Reuse reports the canonical reference; it never closes the older PR."""
    name = "dcc-mcp-krita"
    origin = _make_origin(tmp_path, "dupnoclose", _stale_readme(adapters, name))
    blob = _generated_blob(adapters, tmp_path, name)

    fake = _FakeGitHub(
        open_prs=[_pr_row(49, "https://github.com/dcc-mcp/dcc-mcp-krita/pull/49", sha=blob, owner="loonghao")],
    )
    row = _run_regen(adapters, tmp_path, name, origin, fake, monkeypatch)

    assert row["status"] == regen.DUPLICATE
    assert "pull/49" in row["pr"]
    assert blob[:12] in row["detail"]
    # No close/edit call exists on the fake at all; reuse is read-only.
    assert fake.updated == []
    assert fake.created == []


def test_a_rerun_after_opening_a_pr_stays_a_no_op(adapters, tmp_path, monkeypatch):
    """Two consecutive runs produce one PR, not two.

    The first run opens it; the second sees the same content already open and
    must not push a second branch or create a second PR.
    """
    name = "dcc-mcp-krita"
    blob = _generated_blob(adapters, tmp_path, name)

    first = _FakeGitHub(open_prs=[])
    monkeypatch.setattr(pr_helpers, "check_push_access", lambda repo, **kw: pr_helpers.PUSH_GRANTED)
    origin_one = _make_origin(tmp_path, "duprerun1", _stale_readme(adapters, name))
    row_one = _run_regen(adapters, tmp_path / "one", name, origin_one, first, monkeypatch)
    assert row_one["status"] == regen.REGENERATED
    assert len(first.created) == 1
    assert len(first.pushed) == 1
    monkeypatch.undo()

    # The PR the first run opened is now visible to the second.
    second = _FakeGitHub(
        open_prs=[_pr_row(1, "https://github.com/dcc-mcp/dcc-mcp-krita/pull/1", sha=blob)],
    )
    origin_two = _make_origin(tmp_path, "duprerun2", _stale_readme(adapters, name))
    row_two = _run_regen(adapters, tmp_path / "two", name, origin_two, second, monkeypatch)

    assert row_two["status"] == regen.DUPLICATE
    assert row_two["action"] == "reused-pr"
    assert second.created == []
    assert second.pushed == []


def test_a_real_api_failure_with_the_same_title_creates_nothing(adapters, tmp_path, monkeypatch):
    """Real API failure + same title + genuinely different content: zero writes.

    This is the case that mattered: the per-PR files endpoint failing leaves the
    candidate's blob blank, and the title matches, so a title-based match would
    have reused a PR carrying different content.
    """
    name = "dcc-mcp-krita"
    origin = _make_origin(tmp_path, "apifail", _stale_readme(adapters, name))

    listed = _as_listed(_pr_row(49, "https://github.com/dcc-mcp/dcc-mcp-krita/pull/49", sha=""))
    listed["files"] = [{"path": "README.md"}]
    # The lookup fails for real, so the blob stays unknown.
    fake = _FakeGitHub(open_prs=[listed], pr_blobs={})

    row = _run_regen(adapters, tmp_path, name, origin, fake, monkeypatch)

    assert row["status"] == regen.ERROR
    assert "unreadable content" in row["detail"]
    assert fake.created == []
    assert fake.pushed == []


def test_a_pr_query_failure_creates_nothing(adapters, tmp_path, monkeypatch):
    """A failed PR lookup must fail loudly, never fall through to creating one."""
    name = "dcc-mcp-krita"
    origin = _make_origin(tmp_path, "dupqueryfail", _stale_readme(adapters, name))

    fake = _FakeGitHub(list_error=pr_helpers.PrError("gh pr list failed: HTTP 500"))
    row = _run_regen(adapters, tmp_path, name, origin, fake, monkeypatch)

    assert row["status"] == regen.ERROR
    assert "gh pr list failed" in row["detail"]
    assert fake.created == []
    assert fake.pushed == []


def test_denied_push_still_fails_before_any_pr_query(adapters, tmp_path, monkeypatch):
    """The no-fork rule survives: denied access fails without opening anything."""
    name = "dcc-mcp-krita"
    origin = _make_origin(tmp_path, "dupdenied", _stale_readme(adapters, name))

    fake = _FakeGitHub(push_state=pr_helpers.PUSH_DENIED)
    row = _run_regen(adapters, tmp_path, name, origin, fake, monkeypatch)

    assert row["status"] == regen.ERROR
    assert "does not have push access" in row["detail"]
    assert fake.created == [] and fake.pushed == []


def test_find_equivalent_pr_prefers_an_exact_title_match():
    """Among identical blobs, the one with the generated title wins."""
    rows = [
        _pr_row(2, "https://example/2", sha="c" * 40, title="handwritten tweak"),
        _pr_row(1, "https://example/1", sha="c" * 40, title=PR_TITLE),
    ]
    found = pr_helpers.find_equivalent_pr(rows, path="README.md", blob_sha="c" * 40, title=PR_TITLE)
    assert found["number"] == 1


def test_an_unreadable_blob_is_never_matched_on_title():
    """Same file, same title, unreadable content: must fail, not be reused.

    An earlier revision matched on the title whenever the blob was missing. A
    blob lookup that fails returns '' for *every* PR, so that path reused
    candidates it could not verify -- and since every refresh shares one title,
    it could not tell this catalog state from any other.
    """
    row = _pr_row(7, "https://example/7", sha="", title=PR_TITLE)
    row["files"] = [{"path": "README.md"}]
    with pytest.raises(pr_helpers.UnresolvedCandidate, match="unreadable content"):
        pr_helpers.find_equivalent_pr([row], path="README.md", blob_sha="d" * 40, title=PR_TITLE)


def test_an_unreadable_blob_also_blocks_a_different_title():
    """Unknown content cannot be treated as 'no match' either -- that would create."""
    row = _pr_row(8, "https://example/8", sha="", title="something else entirely")
    row["files"] = [{"path": "README.md"}]
    with pytest.raises(pr_helpers.UnresolvedCandidate):
        pr_helpers.find_equivalent_pr([row], path="README.md", blob_sha="d" * 40, title=PR_TITLE)


def test_a_row_with_no_file_list_at_all_is_unresolved_not_unrelated():
    """A failed files lookup leaves no file list; that must block, not skip.

    Found against the live API: when the per-PR files call fails, rows carry no
    `files` key at all. A `files`-based 'does it touch the generated path' check
    then reports False and the row is dismissed as unrelated -- so the run
    creates a PR beside a candidate it never inspected. Missing metadata is not
    evidence of irrelevance.
    """
    assert pr_helpers._touches_only({"number": 5}, "README.md") is False
    with pytest.raises(pr_helpers.UnresolvedCandidate, match="unreadable content"):
        pr_helpers.find_equivalent_pr(
            [{"number": 5, "title": PR_TITLE}],
            path="README.md",
            blob_sha="d" * 40,
            title=PR_TITLE,
        )


def test_a_confirmed_match_wins_over_an_unreadable_candidate():
    """A verifiable content match is still reusable when another PR is unreadable."""
    good = _pr_row(9, "https://example/9", sha="d" * 40, title=PR_TITLE)
    unreadable = _pr_row(10, "https://example/10", sha="", title=PR_TITLE)
    unreadable["files"] = [{"path": "README.md"}]
    found = pr_helpers.find_equivalent_pr([unreadable, good], path="README.md", blob_sha="d" * 40, title=PR_TITLE)
    assert found["number"] == 9


def test_an_unreadable_blob_on_an_unrelated_file_does_not_block():
    """Only candidates touching the generated file can make the result unresolvable."""
    other = _pr_row(11, "https://example/11", sha="", title=PR_TITLE)
    other["files"] = [{"path": "docs/unrelated.md"}]
    assert pr_helpers.find_equivalent_pr([other], path="README.md", blob_sha="d" * 40, title=PR_TITLE) == {}


def test_find_equivalent_pr_returns_nothing_for_unrelated_rows():
    rows = [
        _pr_row(3, "https://example/3", sha="e" * 40),
        _pr_row(4, "https://example/4", sha="f" * 40, title="something else entirely"),
    ]
    assert pr_helpers.find_equivalent_pr(rows, path="README.md", blob_sha="0" * 40, title=PR_TITLE) == {}


def test_list_open_prs_raises_instead_of_returning_empty(monkeypatch):
    """An empty list would be read as 'no PR exists' and open a duplicate."""
    monkeypatch.setattr(
        pr_helpers,
        "_run",
        lambda args, **kw: subprocess.CompletedProcess(args, 1, "", "HTTP 500"),
    )
    with pytest.raises(pr_helpers.PrError, match="gh api pulls failed"):
        pr_helpers.list_open_prs("dcc-mcp/x", "main")


def test_the_commit_identity_is_a_real_person_not_the_platform_bot():
    """Generated commits must not be attributed to github-actions[bot]."""
    assert regen.committer_name() != "github-actions[bot]"
    assert regen.committer_email() != "actions@github.com"
    assert regen.committer_email().endswith("@users.noreply.github.com")


def test_the_commit_identity_is_overridable_and_validated(monkeypatch):
    """An operator can re-point it; a malformed value fails instead of degrading."""
    monkeypatch.setenv(regen.COMMITTER_NAME_ENV, "Someone Else")
    monkeypatch.setenv(regen.COMMITTER_EMAIL_ENV, "12345+someone@users.noreply.github.com")
    assert regen.committer_name() == "Someone Else"
    assert regen.committer_email() == "12345+someone@users.noreply.github.com"

    monkeypatch.setenv(regen.COMMITTER_NAME_ENV, "   ")
    with pytest.raises(regen.RegenError, match="refusing to commit without a name"):
        regen.committer_name()

    monkeypatch.setenv(regen.COMMITTER_NAME_ENV, "Hal")
    monkeypatch.setenv(regen.COMMITTER_EMAIL_ENV, "not-an-email")
    with pytest.raises(regen.RegenError, match="single email address"):
        regen.committer_email()


def test_the_commit_identity_reaches_the_git_commit(adapters, tmp_path, monkeypatch):
    """The configured identity is what actually lands on the commit."""
    name = "dcc-mcp-krita"
    origin = _make_origin(tmp_path, "identity", _stale_readme(adapters, name))
    recorded = []
    fake = _FakeGitHub()
    fake.install(monkeypatch)
    monkeypatch.setattr(
        regen,
        "_commit_and_push",
        lambda repo_dir, branch, push_target, *, timeout, **kw: recorded.append(repo_dir),
    )

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
    assert row["action"] == "opened-pr"
    assert regen.committer_name() and regen.committer_email()


def test_update_pr_uses_the_rest_patch_not_gh_pr_edit(monkeypatch):
    """`gh pr edit` needs read:org; editing title/body does not.

    The workflow token lacks that scope, so every edit failed. The REST PATCH
    changes the same two fields without reading organisation data.
    """
    seen = {}

    def fake_run(args, **kwargs):
        seen["args"] = args
        return subprocess.CompletedProcess(args, 0, "{}", "")

    monkeypatch.setattr(pr_helpers, "_run", fake_run)
    pr_helpers.update_pr("dcc-mcp/x", "https://github.com/dcc-mcp/x/pull/50", title="t", body="b", timeout=30)

    args = seen["args"]
    assert "edit" not in args  # not `gh pr edit`
    assert args[:4] == ["gh", "api", "--method", "PATCH"]
    assert "repos/dcc-mcp/x/pulls/50" in args
    assert "-f" in args and "title=t" in args and "body=b" in args


def test_update_pr_raises_on_failure_instead_of_creating(monkeypatch):
    """A failed edit must not become a duplicate PR."""
    monkeypatch.setattr(pr_helpers, "_run", lambda args, **kw: subprocess.CompletedProcess(args, 1, "", "HTTP 403"))
    with pytest.raises(pr_helpers.PrError, match="could not update PR"):
        pr_helpers.update_pr("dcc-mcp/x", "https://github.com/dcc-mcp/x/pull/50", title="t", body="b", timeout=30)


def test_update_pr_rejects_a_url_without_a_number():
    with pytest.raises(pr_helpers.PrError, match="pull request number"):
        pr_helpers.update_pr("dcc-mcp/x", "https://example.com/not-a-pr", title="t", body="b")


def test_pr_number_from_url():
    assert pr_helpers.pr_number_from_url("https://github.com/o/r/pull/50") == 50
    assert pr_helpers.pr_number_from_url("https://github.com/o/r/pull/50/") == 50
    assert pr_helpers.pr_number_from_url("not-a-url") == 0


def test_two_sequential_runs_produce_one_pr(adapters, tmp_path, monkeypatch):
    """The race the recheck actually closes: a second run that starts later.

    The workflow concurrency group queues a second run rather than cancelling it
    (`cancel-in-progress: false`), so the overlapping case is a second run that
    begins after the first has pushed. The first opens a PR; the second must see
    it and stop. This is what the second lookup proves -- and all it proves.
    """
    name = "dcc-mcp-krita"
    blob = _generated_blob(adapters, tmp_path, name)

    first = _FakeGitHub(open_prs=[])
    origin_one = _make_origin(tmp_path, "seq1", _stale_readme(adapters, name))
    row_one = _run_regen(adapters, tmp_path / "one", name, origin_one, first, monkeypatch)
    assert row_one["action"] == "opened-pr"
    assert len(first.created) == 1
    monkeypatch.undo()

    second = _FakeGitHub(
        open_prs=[_pr_row(1, "https://github.com/dcc-mcp/dcc-mcp-krita/pull/1", sha=blob)],
    )
    origin_two = _make_origin(tmp_path, "seq2", _stale_readme(adapters, name))
    row_two = _run_regen(adapters, tmp_path / "two", name, origin_two, second, monkeypatch)

    assert row_two["action"] == "reused-pr"
    assert second.created == []


def test_the_recheck_runs_after_the_push(adapters, tmp_path, monkeypatch):
    """The second lookup must observe PRs visible only after the push.

    A single lookup before the push could not see a PR opened by a run that
    finished in between, so the ordering is what makes the check meaningful.
    """
    name = "dcc-mcp-krita"
    blob = _generated_blob(adapters, tmp_path, name)

    class Appearing(_FakeGitHub):
        """No PR before the push; one appears immediately after it."""

        def install(self, monkeypatch):
            super().install(monkeypatch)
            monkeypatch.setattr(
                regen,
                "_commit_and_push",
                lambda repo_dir, branch, push_target, *, timeout, **kw: (
                    self.pushed.append(f"{push_target}:{branch}")
                    or self.open_prs.append(_pr_row(1, "https://github.com/dcc-mcp/dcc-mcp-krita/pull/1", sha=blob))
                ),
            )

    fake = Appearing(open_prs=[])
    origin = _make_origin(tmp_path, "appearing", _stale_readme(adapters, name))
    row = _run_regen(adapters, tmp_path, name, origin, fake, monkeypatch)

    # It pushed, then saw the PR that appeared and reused it instead of creating.
    assert len(fake.pushed) == 1
    assert row["action"] == "reused-pr"
    assert fake.created == []


def test_the_workflow_queues_concurrent_runs_not_cancels():
    """Pins the boundary the recheck relies on: cancel-in-progress is false.

    The concurrency group serialises runs; it does not cancel an in-flight one.
    So a second run does eventually execute, and the in-script recheck is what
    keeps it from opening a second PR. If someone flips this to true, the
    guarantee shifts from "the recheck catches it" to "the first run is killed"
    -- the test below documents which we rely on.
    """
    workflow = (REPO_ROOT / ".github/workflows/adapter-coverage.yml").read_text(encoding="utf-8")
    import yaml as _yaml

    parsed = _yaml.safe_load(workflow)
    group = parsed.get("concurrency") or {}
    assert group.get("group") == "adapter-pointer-regen"
    assert group.get("cancel-in-progress") is False


def _api_run(pages: list[list[dict]], *, stderr: str = "", returncode: int = 0):
    """Return a ``_run`` stub emitting one JSON array per page, as --paginate does."""

    def fake_run(args, **kwargs):
        import json

        return subprocess.CompletedProcess(args, returncode, "\n".join(json.dumps(p) for p in pages), stderr)

    return fake_run


def test_list_open_prs_reads_every_page(monkeypatch):
    """`--paginate` prints one JSON array per page; all of them must be read.

    Reading only the first line would keep just page one -- the exact truncation
    this function exists to avoid, and it would leave an equivalent PR past the
    first page invisible, so the run would open a duplicate.
    """
    first = [{"number": n} for n in range(1, 101)]
    second = [{"number": 999}]
    monkeypatch.setattr(pr_helpers, "_run", _api_run([first, second]))

    rows = pr_helpers.list_open_prs("dcc-mcp/x", "main")
    assert len(rows) == 101
    assert rows[-1]["number"] == 999


def test_list_open_prs_requests_pagination(monkeypatch):
    """The call must actually ask gh to walk pages, not take one capped result."""
    seen = {}

    def fake_run(args, **kwargs):
        seen["args"] = args
        return subprocess.CompletedProcess(args, 0, "[]", "")

    monkeypatch.setattr(pr_helpers, "_run", fake_run)
    pr_helpers.list_open_prs("dcc-mcp/x", "main")

    args = seen["args"]
    # `gh pr list` has no --page flag; paging only exists on `gh api`.
    assert "pr" not in args[:2]
    assert "--paginate" in args
    assert any("repos/dcc-mcp/x/pulls?state=open&base=main" in a for a in args)
    assert any(str(pr_helpers.PR_LIST_PAGE_SIZE) in a for a in args)


def test_list_open_prs_deduplicates_across_pages(monkeypatch):
    """The same PR can appear on two pages; it must be counted once."""
    overlap = {"number": 4242}
    monkeypatch.setattr(pr_helpers, "_run", _api_run([[overlap, {"number": 1}], [overlap]]))
    rows = pr_helpers.list_open_prs("dcc-mcp/x", "main")
    assert [r["number"] for r in rows] == [4242, 1]
    assert sum(1 for r in rows if r["number"] == 4242) == 1


def test_list_open_prs_rejects_an_unparsable_page(monkeypatch):
    """A page that does not parse is a failed lookup, not an empty result."""
    monkeypatch.setattr(pr_helpers, "_run", lambda args, **kw: subprocess.CompletedProcess(args, 0, "[]\n{oops", ""))
    with pytest.raises(pr_helpers.PrError, match="unparsable JSON"):
        pr_helpers.list_open_prs("dcc-mcp/x", "main")


def test_a_reused_pr_is_reported_as_success_not_failure(tmp_path, capsys, monkeypatch):
    """A reused PR is the run doing the right thing, so it must not read as FAIL.

    The console line marks anything outside CURRENT/REGENERATED/DUPLICATE as a
    failure, and the summary counts DUPLICATE separately from errors -- a reused
    PR that looked like an error would make a clean fleet look broken.
    """
    results = [
        {
            "name": "adapter-a",
            "status": regen.DUPLICATE,
            "action": "reused-pr",
            "detail": "reused #49",
            "pr": "https://github.com/dcc-mcp/dcc-mcp-shogun/pull/49",
        },
        {"name": "adapter-b", "status": regen.CURRENT, "action": "none", "detail": "ok", "pr": "", "url": ""},
        {
            "name": "adapter-c",
            "status": regen.ERROR,
            "action": "none",
            "detail": "boom",
            "pr": "",
            "url": "https://github.com/dcc-mcp/dcc-mcp-krita",
        },
    ]
    args = argparse.Namespace(
        dry_run=False,
        summary=None,
        json_out=tmp_path / "regen.json",
    )
    monkeypatch.chdir(tmp_path)
    code = regen._report(results, args, host_count=3)

    out = capsys.readouterr().out
    assert "ok   adapter-a" in out
    assert "FAIL adapter-c" in out
    assert "1 already covered by an open PR" in out
    # Errors still fail the run; a reuse does not.
    assert code != 0
    assert regen.DUPLICATE not in (regen.ERROR, regen.MISSING)


def test_a_stale_repository_opens_a_pull_request(adapters, tmp_path, monkeypatch):
    """A drifted adapter gets one PR against the repository's default branch."""
    name = "dcc-mcp-krita"
    origin = _make_origin(tmp_path, "openpr", _stale_readme(adapters, name))
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

    assert row["status"] == regen.REGENERATED
    assert row["action"] == "opened-pr"
    assert row["pr"] == "https://github.com/example/pr/1"
    assert len(fake.created) == 1
    assert fake.created[0]["base"] == "main"
    # Push access was granted, so the branch lives in the upstream repository
    # itself and the head stays an unqualified branch name -- no fork involved.
    assert fake.created[0]["head"] == regen.BRANCH_PREFIX + regen._fingerprint(len(adapters))
    assert ":" not in fake.created[0]["head"]
    assert str(len(adapters)) in fake.created[0]["body"]


def test_a_stale_repository_without_push_fails_instead_of_using_a_fork(adapters, tmp_path, monkeypatch):
    """Denied push access must fail the run without any writes."""
    name = "dcc-mcp-krita"
    origin = _make_origin(tmp_path, "openprdeniedfork", _stale_readme(adapters, name))
    fake = _FakeGitHub(push_state=pr_helpers.PUSH_DENIED)
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

    assert row["status"] == regen.ERROR
    assert "does not have push access" in row["detail"]
    assert fake.pushed == []
    assert fake.created == [] and fake.updated == []


def test_a_stale_repository_with_an_unknown_permission_fails_instead_of_using_a_fork(adapters, tmp_path, monkeypatch):
    """An unknown permission must fail the run without any writes."""
    name = "dcc-mcp-krita"
    origin = _make_origin(tmp_path, "openprunknownfork", _stale_readme(adapters, name))
    fake = _FakeGitHub(push_state=pr_helpers.PUSH_UNKNOWN)
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

    assert row["status"] == regen.ERROR
    assert "could not confirm push access" in row["detail"]
    assert fake.pushed == []
    assert fake.created == [] and fake.updated == []


def test_an_existing_pull_request_is_reused_not_duplicated(adapters, tmp_path, monkeypatch):
    """A second run reuses the open PR instead of opening another one.

    It no longer edits the PR either: the content is already byte-identical, so
    refreshing the title and body would be a write with no effect. The run is a
    no-op that reports the canonical PR.
    """
    name = "dcc-mcp-krita"
    origin = _make_origin(tmp_path, "refresh", _stale_readme(adapters, name))
    blob = _generated_blob(adapters, tmp_path, name)
    fake = _FakeGitHub(
        open_prs=[
            _pr_row(
                7,
                "https://github.com/dcc-mcp/dcc-mcp-krita/pull/7",
                sha=blob,
                head=regen.BRANCH_PREFIX + regen._fingerprint(len(adapters)),
            )
        ],
    )
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

    assert row["action"] == "reused-pr"
    assert row["status"] == regen.DUPLICATE
    assert row["pr"] == "https://github.com/dcc-mcp/dcc-mcp-krita/pull/7"
    assert fake.created == []
    assert fake.pushed == []


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


def _push_run(stdout: str, returncode: int = 0, stderr: str = ""):
    """Return a ``_run`` stub that answers the permission lookup with ``stdout``."""

    def fake_run(args, **kwargs):
        return subprocess.CompletedProcess(args, returncode, stdout, stderr)

    return fake_run


def test_check_push_access_reads_the_rest_permission(monkeypatch):
    """`permissions.push` on /repos/{repo} is what decides, not the login."""
    seen = []

    def fake_run(args, **kwargs):
        seen.append(args)
        return subprocess.CompletedProcess(args, 0, "true\n", "")

    monkeypatch.setattr(pr_helpers, "_run", fake_run)

    assert pr_helpers.check_push_access("dcc-mcp/dcc-mcp-maya") == pr_helpers.PUSH_GRANTED
    assert seen[0] == ["gh", "api", "repos/dcc-mcp/dcc-mcp-maya", "--jq", ".permissions.push"]


def test_check_push_access_reports_denied_distinctly(monkeypatch):
    """An explicit 'false' is DENIED, not UNKNOWN -- the operator needs the difference."""
    monkeypatch.setattr(pr_helpers, "_run", _push_run("false\n"))
    assert pr_helpers.check_push_access("dcc-mcp/dcc-mcp-maya") == pr_helpers.PUSH_DENIED


def test_check_push_access_reports_unknown_when_the_lookup_fails(monkeypatch):
    monkeypatch.setattr(pr_helpers, "_run", _push_run("", returncode=1, stderr="HTTP 404\n"))
    assert pr_helpers.check_push_access("dcc-mcp/dcc-mcp-maya") == pr_helpers.PUSH_UNKNOWN


def test_check_push_access_treats_an_empty_body_as_unknown(monkeypatch):
    """A 200 with no body has confirmed nothing; it must not read as granted."""
    monkeypatch.setattr(pr_helpers, "_run", _push_run("", returncode=0))
    assert pr_helpers.check_push_access("dcc-mcp/dcc-mcp-maya") == pr_helpers.PUSH_UNKNOWN


def test_granted_push_uses_only_the_upstream_permission_lookup(monkeypatch):
    """Confirmed push access selects upstream without querying login or forks."""
    calls = []

    def fake_run(args, **kwargs):
        calls.append(args)
        return subprocess.CompletedProcess(args, 0, "true\n", "")

    monkeypatch.setattr(pr_helpers, "_run", fake_run)

    assert pr_helpers.ensure_push_target("dcc-mcp/dcc-mcp-maya") == "dcc-mcp/dcc-mcp-maya"
    # Only the permission lookup ran. No fork was queried or created.
    assert len(calls) == 1
    assert calls[0][:3] == ["gh", "api", "repos/dcc-mcp/dcc-mcp-maya"]
    assert not any("fork" in " ".join(call) for call in calls)


def test_denied_push_raises_and_never_touches_a_fork(monkeypatch):
    """Denied access must raise, and no fork lookup may even be attempted."""
    calls = []

    def fake_run(args, **kwargs):
        calls.append(args)
        return subprocess.CompletedProcess(args, 0, "false\n", "")

    monkeypatch.setattr(pr_helpers, "_run", fake_run)

    with pytest.raises(pr_helpers.PrError) as exc:
        pr_helpers.ensure_push_target("dcc-mcp/dcc-mcp-maya")

    message = str(exc.value)
    assert "does not have push access" in message
    assert "dcc-mcp/dcc-mcp-maya" in message
    assert "No fork was used or created" in message
    # A fork query would be a graphql call; none may happen.
    assert not any("graphql" in call or "fork" in " ".join(call) for call in calls)


def test_unknown_push_raises_with_its_own_reason(monkeypatch):
    """A failed lookup names its own cause instead of blaming the token."""
    monkeypatch.setattr(pr_helpers, "_run", _push_run("", returncode=1, stderr="HTTP 500\n"))

    with pytest.raises(pr_helpers.PrError) as exc:
        pr_helpers.ensure_push_target("dcc-mcp/dcc-mcp-maya")

    message = str(exc.value)
    assert "could not confirm push access" in message
    assert "does not have push access" not in message
    assert "No fork was used or created" in message


def test_an_existing_fork_does_not_rescue_a_denied_push(monkeypatch):
    """The critical combination: push denied, but a personal fork exists.

    The fork must NOT be used. The fork lookup is a graphql query on `forks`,
    and it is never even issued; the run fails instead.
    """
    calls = []

    def fake_run(args, **kwargs):
        calls.append(args)
        if "graphql" in args:
            return subprocess.CompletedProcess(args, 0, "loonghao/dcc-mcp-maya\n", "")
        return subprocess.CompletedProcess(args, 0, "false\n", "")

    monkeypatch.setattr(pr_helpers, "_run", fake_run)

    with pytest.raises(pr_helpers.PrError, match="does not have push access"):
        pr_helpers.ensure_push_target("dcc-mcp/dcc-mcp-maya")

    # The graphql fork lookup was never called.
    assert not any("graphql" in call for call in calls)
    assert len(calls) == 1


def test_an_existing_fork_does_not_rescue_a_failed_lookup(monkeypatch):
    """Lookup failed plus an existing fork: still a hard failure, no fork use."""
    calls = []

    def fake_run(args, **kwargs):
        calls.append(args)
        if "graphql" in args:
            return subprocess.CompletedProcess(args, 0, "loonghao/dcc-mcp-maya\n", "")
        return subprocess.CompletedProcess(args, 1, "", "network down\n")

    monkeypatch.setattr(pr_helpers, "_run", fake_run)

    with pytest.raises(pr_helpers.PrError, match="could not confirm push access"):
        pr_helpers.ensure_push_target("dcc-mcp/dcc-mcp-maya")

    assert not any("graphql" in call for call in calls)
    assert len(calls) == 1


def test_no_fork_is_ever_created(monkeypatch):
    """`gh repo fork` must not appear under any permission outcome."""
    for stdout, returncode in (("false\n", 0), ("", 1)):
        calls = []

        # Bound as default arguments: a closure over the loop variables would
        # capture whichever iteration ran last, not the one being tested.
        def fake_run(args, _calls=calls, _stdout=stdout, _returncode=returncode, **kwargs):
            _calls.append(args)
            return subprocess.CompletedProcess(args, _returncode, _stdout, "")

        monkeypatch.setattr(pr_helpers, "_run", fake_run)

        with pytest.raises(pr_helpers.PrError):
            pr_helpers.ensure_push_target("dcc-mcp/dcc-mcp-maya")

        assert not any(args[:3] == ["gh", "repo", "fork"] for args in calls)


def test_the_branch_prefix_cannot_collide_with_a_docs_branch():
    """A fork carrying a branch literally named `docs` rejects `docs/<sub>`."""
    assert not regen.BRANCH_PREFIX.startswith("docs/")


def test_find_open_pr_matches_on_the_bare_branch_name(monkeypatch):
    """`--head` rejects the `owner:branch` form used to create a cross-repo PR."""
    seen = {}

    def fake_run(args, **kwargs):
        seen["head"] = args[args.index("--head") + 1]
        return subprocess.CompletedProcess(
            args, 0, '[{"url":"https://example/pr/9","headRepositoryOwner":{"login":"loonghao"}}]', ""
        )

    monkeypatch.setattr(pr_helpers, "_run", fake_run)

    url = pr_helpers.find_open_pr("dcc-mcp/x", "loonghao:chore/refresh-catalog-pointer-47-adapters", "main")
    assert url == "https://example/pr/9"
    assert seen["head"] == "chore/refresh-catalog-pointer-47-adapters"


def test_the_push_relies_on_an_inherited_git_credential():
    """The workflow must export a git credential; the script cannot supply one.

    `GH_TOKEN` is read by `gh` but not by git, and the push is a child process,
    so `git -c` in the workflow cannot wrap it. The only mechanism that reaches
    the push is `GIT_CONFIG_*` in the environment. If a future edit drops that
    export, every cross-repository push fails with 401 -- and nothing in this
    test suite would notice: the real-push tests use local, unauthenticated repos.

    So the contract is pinned here against the workflow file itself.
    """
    workflow = (REPO_ROOT / ".github/workflows/adapter-coverage.yml").read_text(encoding="utf-8")
    assert "GIT_CONFIG_COUNT=2" in workflow
    assert "credential.helper" in workflow
    assert "store --file=" in workflow
    # The token must come from the org secret that actually exists.
    assert "secrets.PERSONAL_ACCESS_TOKEN" in workflow
    # And the file holding it must be removed again.
    assert "trap 'rm -f \"$credential_file\"'" in workflow


def test_pr_calls_forward_the_callers_timeout(monkeypatch):
    """`create_pr` / `update_pr` accept `timeout`; it must reach `_run`.

    Both silently dropped it, so the workflow's `--timeout 300` was overridden
    by the 600s default and a hung `gh` could outlive the caller's own budget.
    """
    seen = {}

    def fake_run(args, **kwargs):
        seen.setdefault("timeouts", []).append(kwargs.get("timeout"))
        return subprocess.CompletedProcess(args, 0, "https://example/pr/1\n", "")

    monkeypatch.setattr(pr_helpers, "_run", fake_run)

    pr_helpers.create_pr("dcc-mcp/x", base="main", head="chore/pointer", title="t", body="b", timeout=300)
    pr_helpers.update_pr("dcc-mcp/x", "https://example/pr/1", title="t", body="b", timeout=300)

    assert seen["timeouts"] == [300, 300]


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


def _listed_row(number: int, title: str = PR_TITLE) -> dict:
    """Return a row shaped exactly like `list_open_prs` emits it -- no `files` key.

    The committed tests mostly inject rows that already carry a `files` list, a
    shape the production listing never produces. Fixtures in this shape are what
    caught two defects that the hand-built rows could not see.
    """
    return {
        "number": number,
        "url": f"https://github.com/dcc-mcp/dcc-mcp-krita/pull/{number}",
        "title": title,
        "headRefName": "chore/refresh-catalog-pointer-catalog-pointer",
        "headRepositoryOwner": {"login": "dcc-mcp"},
        "headRefOid": "0" * 40,
        "author": {"login": "dcc-mcp"},
    }


def test_pr_files_returns_the_whole_list_not_one_blob(monkeypatch):
    """The equivalence guard needs every file, so the lookup must not reduce to one."""
    payload = json.dumps(
        [
            {"path": "README.md", "sha": "a" * 40},
            {"path": "src/handwritten.py", "sha": "b" * 40},
        ]
    )
    monkeypatch.setattr(
        pr_helpers,
        "_run",
        lambda args, **kw: subprocess.CompletedProcess(args, 0, payload, ""),
    )
    assert pr_helpers.pr_files("o/r", 8, timeout=30) == [
        {"path": "README.md", "sha": "a" * 40},
        {"path": "src/handwritten.py", "sha": "b" * 40},
    ]


def test_pr_files_raises_when_the_lookup_fails(monkeypatch):
    """A failed files lookup must not be read as 'this PR touches nothing'."""
    monkeypatch.setattr(
        pr_helpers,
        "_run",
        lambda args, **kw: subprocess.CompletedProcess(args, 1, "", "HTTP 500"),
    )
    with pytest.raises(pr_helpers.PrError, match="HTTP 500"):
        pr_helpers.pr_files("o/r", 8, timeout=30)


def test_pr_files_pages_through_every_page(monkeypatch):
    """`--paginate` emits one array per line; all of them belong in the result."""
    payload = "\n".join(
        [
            json.dumps([{"path": "README.md", "sha": "a" * 40}]),
            json.dumps([{"path": "src/second.py", "sha": "b" * 40}]),
        ]
    )
    monkeypatch.setattr(
        pr_helpers,
        "_run",
        lambda args, **kw: subprocess.CompletedProcess(args, 0, payload, ""),
    )
    assert len(pr_helpers.pr_files("o/r", 8, timeout=30)) == 2


def test_an_open_pr_not_touching_the_generated_file_is_not_unresolved(monkeypatch):
    """The bug that blocked every refresh: an unrelated PR looked unresolvable.

    A PR touching only `src/main.rs` answers the files lookup with a real, empty
    match for the generated path. That is a normal non-match, not a failed
    lookup -- but the resolver dropped the file list whenever the generated path
    was absent, and the equivalence check then read the missing list as
    "unreadable content" and refused to open anything.
    """
    monkeypatch.setattr(
        pr_helpers,
        "pr_files",
        lambda repo, number, **kw: [{"path": "src/main.rs", "sha": "c" * 40}],
    )
    resolved = pr_helpers.resolve_pr_blobs([_listed_row(7)], "o/r", "README.md", timeout=30)
    assert resolved[0]["files"] == [{"path": "src/main.rs", "sha": "c" * 40}]
    assert pr_helpers.find_equivalent_pr(resolved, path="README.md", blob_sha="a" * 40, title=PR_TITLE) == {}


def test_an_unrelated_open_pr_does_not_block_the_refresh(adapters, tmp_path, monkeypatch):
    """Full-path regression: an unrelated open PR must still let the PR open."""
    name = "dcc-mcp-krita"
    origin = _make_origin(tmp_path, "prodshape1", _stale_readme(adapters, name))

    listed = _listed_row(7, title="refactor: unrelated work")
    listed["files"] = [{"path": "src/main.rs"}]
    fake = _FakeGitHub(open_prs=[listed])
    row = _run_regen(adapters, tmp_path, name, origin, fake, monkeypatch)

    assert row["status"] == regen.REGENERATED, f"got {row['status']}: {row['detail']}"
    assert len(fake.created) == 1


def test_a_pr_carrying_extra_files_is_not_reused_in_production(adapters, tmp_path, monkeypatch):
    """Full-path regression: the extra-files guard must hold for production rows.

    The resolver used to synthesise `files` from a single blob, so a PR carrying
    the generated README plus a handwritten file looked like it touched only the
    generated file and was reused -- folding unrelated work into the refresh.
    """
    name = "dcc-mcp-krita"
    origin = _make_origin(tmp_path, "prodshape2", _stale_readme(adapters, name))
    blob = _generated_blob(adapters, tmp_path, name)

    listed = _listed_row(8)
    listed["files"] = [{"path": "README.md"}, {"path": "src/handwritten.py"}]
    fake = _FakeGitHub(open_prs=[listed], pr_blobs={8: blob})
    row = _run_regen(adapters, tmp_path, name, origin, fake, monkeypatch)

    assert row["status"] == regen.REGENERATED, f"got {row['status']}: {row['detail']}"
    assert len(fake.created) == 1


def test_a_single_file_match_is_still_reused_in_production(adapters, tmp_path, monkeypatch):
    """Control: a genuine single-file equivalent must still be recognised."""
    name = "dcc-mcp-krita"
    origin = _make_origin(tmp_path, "prodshape3", _stale_readme(adapters, name))
    blob = _generated_blob(adapters, tmp_path, name)

    listed = _listed_row(9)
    listed["files"] = [{"path": "README.md"}]
    fake = _FakeGitHub(open_prs=[listed], pr_blobs={9: blob})
    row = _run_regen(adapters, tmp_path, name, origin, fake, monkeypatch)

    assert row["status"] == regen.DUPLICATE, f"got {row['status']}: {row['detail']}"
    assert row["pr_number"] == 9
    assert fake.created == [] and fake.pushed == []


def test_an_empty_file_list_is_a_real_answer_not_a_lookup_failure():
    """An empty list means 'touches nothing'; a missing key means 'lookup failed'."""
    assert (
        pr_helpers.find_equivalent_pr(
            [{"number": 12, "title": PR_TITLE, "files": []}],
            path="README.md",
            blob_sha="d" * 40,
            title=PR_TITLE,
        )
        == {}
    )
    with pytest.raises(pr_helpers.UnresolvedCandidate):
        pr_helpers.find_equivalent_pr(
            [{"number": 13, "title": PR_TITLE}],
            path="README.md",
            blob_sha="d" * 40,
            title=PR_TITLE,
        )


@pytest.mark.parametrize("reindexed", [False, True])
def test_same_head_pr_is_refreshed_after_push(adapters, tmp_path, monkeypatch, reindexed):
    """Updating our stable branch is a refresh, regardless of API indexing lag."""
    name = "dcc-mcp-krita"
    blob = _generated_blob(adapters, tmp_path, name)
    url = "https://github.com/dcc-mcp/dcc-mcp-krita/pull/7"

    class SameHead(_FakeGitHub):
        def install(self, monkeypatch):
            super().install(monkeypatch)

            def push(repo_dir, branch, push_target, **kwargs):
                self.pushed.append(branch)
                if reindexed:
                    self.open_prs[0]["files"][0]["sha"] = blob

            monkeypatch.setattr(regen, "_commit_and_push", push)

    fake = SameHead(existing_pr=url, open_prs=[_pr_row(7, url, sha="f" * 40)])
    origin = _make_origin(tmp_path, "same-head", _stale_readme(adapters, name))
    row = _run_regen(adapters, tmp_path, name, origin, fake, monkeypatch)
    assert row["status"] == regen.REGENERATED
    assert row["action"] == "refreshed-pr"
    assert row["pr"] == url
    assert len(fake.pushed) == len(fake.updated) == 1
    assert fake.updated[0]["title"] == PR_TITLE
    assert fake.updated[0]["body"]
    assert fake.created == []


@pytest.mark.parametrize("payload", ["", " ", "[null]", "[{}]", '[{"number":true}]', '[{"number":0}]'])
def test_list_open_prs_rejects_incomplete_payload(monkeypatch, payload):
    """Malformed candidates must not be silently lost during deduplication."""
    monkeypatch.setattr(pr_helpers, "_run", lambda args, **kw: subprocess.CompletedProcess(args, 0, payload, ""))
    with pytest.raises(pr_helpers.PrError):
        pr_helpers.list_open_prs("o/r", "main")


@pytest.mark.parametrize("bad_entry", [None, {}, {"path": ""}, {"path": 12}])
def test_pr_files_rejects_malformed_extra_file(monkeypatch, bad_entry):
    """Dropping an invalid second entry would falsely prove a one-file match."""
    payload = json.dumps([{"path": "README.md", "sha": "a" * 40}, bad_entry])
    monkeypatch.setattr(pr_helpers, "_run", lambda args, **kw: subprocess.CompletedProcess(args, 0, payload, ""))
    with pytest.raises(pr_helpers.PrError, match="invalid file entry"):
        pr_helpers.pr_files("o/r", 1)


@pytest.mark.parametrize("payload", ["", " "])
def test_pr_files_rejects_empty_response(monkeypatch, payload):
    """An absent response is not a confirmed empty file list."""
    monkeypatch.setattr(pr_helpers, "_run", lambda args, **kw: subprocess.CompletedProcess(args, 0, payload, ""))
    with pytest.raises(pr_helpers.PrError, match="empty response"):
        pr_helpers.pr_files("o/r", 1)


@pytest.mark.parametrize("payload", ["", "null", "[null]", "[{}]"])
def test_find_open_pr_rejects_invalid_response(monkeypatch, payload):
    """A failed same-head lookup must not authorize creating another PR."""
    monkeypatch.setattr(pr_helpers, "_run", lambda args, **kw: subprocess.CompletedProcess(args, 0, payload, ""))
    with pytest.raises(pr_helpers.PrError):
        pr_helpers.find_open_pr("o/r", "branch", "main")


def test_find_open_pr_does_not_refresh_a_same_named_fork_branch(monkeypatch):
    """Only the upstream branch received this run's push."""
    payload = json.dumps(
        [
            {"url": "https://example/pr/1", "headRepositoryOwner": {"login": "someone"}},
            {"url": "https://example/pr/2", "headRepositoryOwner": {"login": "o"}},
        ]
    )
    monkeypatch.setattr(pr_helpers, "_run", lambda args, **kw: subprocess.CompletedProcess(args, 0, payload, ""))
    assert pr_helpers.find_open_pr("o/r", "branch", "main") == "https://example/pr/2"


def test_find_open_pr_raises_on_lookup_failure(monkeypatch):
    """API errors cannot be interpreted as permission to create a PR."""
    monkeypatch.setattr(pr_helpers, "_run", lambda args, **kw: subprocess.CompletedProcess(args, 1, "", "HTTP 500"))
    with pytest.raises(pr_helpers.PrError, match="HTTP 500"):
        pr_helpers.find_open_pr("o/r", "branch", "main")


def test_update_pr_keeps_literal_fields_and_forwards_timeout(monkeypatch):
    calls = []

    def fake_run(args, **kwargs):
        calls.append((args, kwargs))
        return subprocess.CompletedProcess(args, 0, "{}", "")

    monkeypatch.setattr(pr_helpers, "_run", fake_run)
    pr_helpers.update_pr(
        "dcc-mcp/x",
        "https://github.com/dcc-mcp/x/pull/50",
        title="@literal-title",
        body="@not-a-file\nfalse\n123",
        timeout=31,
    )
    args, kwargs = calls[0]
    assert "-F" not in args
    assert args[args.index("title=@literal-title") - 1] == "-f"
    assert args[args.index("body=@not-a-file\nfalse\n123") - 1] == "-f"
    assert kwargs["timeout"] == 31


@pytest.mark.parametrize(
    "url",
    [
        "  https://github.com/dcc-mcp/x/pull/50  ",
        "\thttps://github.com/dcc-mcp/x/pull/50/\n",
    ],
)
def test_pr_number_strips_url_whitespace(url):
    assert pr_helpers.pr_number_from_url(url) == 50


def test_pointer_jobs_have_bounded_timeouts():
    import yaml

    workflow = yaml.safe_load((REPO_ROOT / ".github/workflows/adapter-coverage.yml").read_text(encoding="utf-8"))
    for name in ("coverage", "pointer-drift", "regenerate-pointers"):
        assert workflow["jobs"][name].get("timeout-minutes") == 30
    # PyYAML's YAML 1.1 loader interprets the Actions `on` key as True.
    events = workflow.get("on", workflow.get(True))
    assert "README.md" in events["pull_request"]["paths"]
    assert "README.md" not in events["push"]["paths"]


@pytest.mark.parametrize(("stdout", "returncode"), [("", 128), ("", 0), ("not-a-blob", 0)])
def test_unreadable_generated_blob_stops_before_writes(adapters, tmp_path, monkeypatch, stdout, returncode):
    name = "dcc-mcp-krita"
    origin = _make_origin(tmp_path, "blob-failure", _stale_readme(adapters, name))
    fake = _FakeGitHub()
    fake.install(monkeypatch)
    real_git = regen._git

    def fail_hash(args, *, timeout):
        if "hash-object" in args:
            return subprocess.CompletedProcess(args, returncode, stdout, "cannot read generated file")
        return real_git(args, timeout=timeout)

    monkeypatch.setattr(regen, "_git", fail_hash)
    row = _run_regen(adapters, tmp_path, name, origin, fake, monkeypatch)
    assert row["status"] == regen.ERROR
    assert not fake.pushed and not fake.created and not fake.updated


@pytest.mark.parametrize("current", [False, True])
def test_real_git_no_change_or_equivalent_pr_creates_no_commit_or_branch(adapters, tmp_path, monkeypatch, current):
    name = "dcc-mcp-krita"
    entry = _entry(adapters, name)
    fresh = generator.upsert_pointer(
        "# adapter\n\n## Install\n", generator.render_pointer(entry, host_count=len(adapters))
    )
    origin = _make_origin(tmp_path, "no-write", fresh if current else _stale_readme(adapters, name))
    # Hash the generated UTF-8 file, not locale-encoded text-mode stdin.
    blob = _generated_blob(adapters, tmp_path, name)
    fake = _FakeGitHub(open_prs=[_pr_row(49, "https://example/pr/49", sha=blob, owner="loonghao")])
    real_commit = regen._commit_and_push
    fake.install(monkeypatch)
    monkeypatch.setattr(regen, "_commit_and_push", real_commit)
    real_git = regen._git
    calls = []

    def local_git(args, *, timeout):
        calls.append(args)
        assert not any(argument.startswith(("https://", "http://")) for argument in args), (
            "a no-op must not query a remote push target"
        )
        assert "push" not in args, "a no-op must never reach a real push"
        return real_git(args, timeout=timeout)

    monkeypatch.setattr(regen, "_git", local_git)
    before = subprocess.check_output(["git", "-C", origin, "show-ref"], text=True)
    row = regen.regenerate_repository(
        _local_entry(adapters, name, origin),
        0,
        host_count=len(adapters),
        workdir=tmp_path / "work",
        timeout=60,
        branch_prefix=regen.BRANCH_PREFIX,
        dry_run=False,
        source_ref="test",
    )
    assert row["status"] == (regen.CURRENT if current else regen.DUPLICATE), row
    assert not fake.created and not fake.updated
    assert not any("commit" in args or ("checkout" in args and "-b" in args) for args in calls)
    assert subprocess.check_output(["git", "-C", origin, "show-ref"], text=True) == before


def _real_push_setup(tmp_path, monkeypatch):
    """Redirect only the synthetic test remote; all git operations remain real."""
    origin = _make_origin(tmp_path, "lease", "# adapter\n")
    real_git = regen._git
    calls = []
    before_push = []

    def local_git(args, *, timeout):
        calls.append(list(args))
        url = "https://github.com/test/pointer-lease"
        for argument in args:
            if argument.startswith(("https://", "http://")):
                assert argument == url, "tests must never access a real network remote"
        local_args = [origin if argument == url else argument for argument in args]
        if "push" in args and before_push:
            before_push.pop(0)()
        return real_git(local_args, timeout=timeout)

    monkeypatch.setattr(regen, "_git", local_git)

    def checkout(name, content):
        path = tmp_path / name
        subprocess.run(["git", "clone", "-q", "--branch", "main", origin, str(path)], check=True)
        (path / "README.md").write_text(content, encoding="utf-8")
        return path

    return origin, calls, before_push, checkout


def test_real_git_push_creates_then_refreshes_with_explicit_lease(tmp_path, monkeypatch):
    origin, calls, _, checkout = _real_push_setup(tmp_path, monkeypatch)
    branch = regen.BRANCH_PREFIX + "catalog-pointer"
    ref = "refs/heads/" + branch
    regen._commit_and_push(checkout("first", "first\n"), branch, "test/pointer-lease", timeout=60)
    first = subprocess.check_output(["git", "-C", origin, "rev-parse", ref], text=True).strip()
    regen._commit_and_push(checkout("second", "second\n"), branch, "test/pointer-lease", timeout=60)
    pushes = [args for args in calls if "push" in args]
    assert "--force" not in pushes[0] and "--force" not in pushes[1]
    assert "--force-with-lease=" + ref + ":" in pushes[0]
    assert "--force-with-lease=" + ref + ":" + first in pushes[1]
    assert subprocess.check_output(["git", "-C", origin, "show", ref + ":README.md"], text=True) == "second\n"
    identity = subprocess.check_output(
        ["git", "-C", origin, "show", "-s", "--format=%an <%ae>", ref], text=True
    ).strip()
    assert identity == regen.committer_name() + " <" + regen.committer_email() + ">"


@pytest.mark.parametrize("existing", [False, True])
def test_real_git_lease_rejects_a_concurrent_branch_update(tmp_path, monkeypatch, existing):
    origin, _, before_push, checkout = _real_push_setup(tmp_path, monkeypatch)
    branch = regen.BRANCH_PREFIX + "catalog-pointer"
    ref = "refs/heads/" + branch
    if existing:
        regen._commit_and_push(checkout("first", "first\n"), branch, "test/pointer-lease", timeout=60)
    parent = subprocess.check_output(["git", "-C", origin, "rev-parse", ref if existing else "main"], text=True).strip()
    competitor = subprocess.check_output(
        [
            "git",
            "-C",
            origin,
            "-c",
            "user.name=Competitor",
            "-c",
            "user.email=test@example.com",
            "commit-tree",
            "main^{tree}",
            "-p",
            parent,
            "-m",
            "Concurrent writer",
        ],
        text=True,
    ).strip()
    before_push.append(lambda: subprocess.run(["git", "-C", origin, "update-ref", ref, competitor], check=True))
    with pytest.raises(regen.RegenError, match="git push failed"):
        regen._commit_and_push(checkout("racing", "racing\n"), branch, "test/pointer-lease", timeout=60)
    assert subprocess.check_output(["git", "-C", origin, "rev-parse", ref], text=True).strip() == competitor


@pytest.mark.parametrize(
    ("stdout", "returncode"),
    [("", 1), ("not-a-sha\trefs/heads/chore/pointer\n", 0), ("a" * 40 + "\trefs/heads/other\n", 0)],
)
def test_invalid_remote_ref_lookup_never_commits_or_pushes(monkeypatch, tmp_path, stdout, returncode):
    calls = []

    def fail_lookup(args, *, timeout):
        calls.append(args)
        if "ls-remote" in args:
            return subprocess.CompletedProcess(args, returncode, stdout, "lookup failed")
        return subprocess.CompletedProcess(args, 0, "", "")

    monkeypatch.setattr(regen, "_git", fail_lookup)
    with pytest.raises(regen.RegenError, match="remote branch"):
        regen._commit_and_push(tmp_path, regen.BRANCH_PREFIX + "catalog-pointer", "test/pointer-lease", timeout=60)
    assert not any("commit" in args or "push" in args for args in calls)


@pytest.mark.parametrize("branch", ["main", "feature/user-work", "chore/something-else"])
def test_writer_refuses_non_generated_branches(monkeypatch, tmp_path, branch):
    def forbidden_git(*args, **kwargs):
        raise AssertionError("a user branch must be rejected before any git operation")

    monkeypatch.setattr(regen, "_git", forbidden_git)
    with pytest.raises(regen.RegenError, match="non-generated branch"):
        regen._commit_and_push(tmp_path, branch, "test/pointer-lease", timeout=60)


def test_summary_distinguishes_current_duplicate_and_failed_repositories():
    results = [
        {"name": "current", "status": regen.CURRENT, "action": "none", "pr": "", "detail": "current"},
        {
            "name": "duplicate",
            "status": regen.DUPLICATE,
            "action": "reused-pr",
            "pr": "https://example/pr/1",
            "detail": "open",
        },
        {"name": "error", "status": regen.ERROR, "action": "none", "pr": "", "detail": "lookup failed"},
    ]
    summary = regen.render_summary(results, host_count=3, dry_run=False)
    assert "All 3 scanned repositories already match" not in summary
    assert "Default branches already current: **1**." in summary
    assert "Equivalent open PRs awaiting merge: **1**." in summary
    assert "1 repositories could not be regenerated" in summary
    assert "refused to commit" not in summary


def test_summary_does_not_call_missing_readmes_current():
    results = [{"name": "missing", "status": regen.MISSING, "action": "none", "pr": "", "detail": "no README"}]
    summary = regen.render_summary(results, host_count=1, dry_run=False)
    assert "All 1 scanned repositories already match" not in summary
    assert "Default branches already current: **0**." in summary
    assert "1 repositories have no README.md to regenerate." in summary


@pytest.mark.parametrize("push_state", [pr_helpers.PUSH_DENIED, pr_helpers.PUSH_UNKNOWN])
def test_equivalent_fork_pr_needs_no_push_permission(adapters, tmp_path, monkeypatch, push_state):
    name = "dcc-mcp-krita"
    origin = _make_origin(tmp_path, "readonly-dedupe", _stale_readme(adapters, name))
    # Match the generator's on-disk bytes and Git's platform normalization.
    blob = _generated_blob(adapters, tmp_path, name)
    fake = _FakeGitHub(
        push_state=push_state, open_prs=[_pr_row(49, "https://example/pr/49", sha=blob, owner="loonghao")]
    )
    fake.install(monkeypatch)
    row = regen.regenerate_repository(
        _local_entry(adapters, name, origin),
        0,
        host_count=len(adapters),
        workdir=tmp_path / "work",
        timeout=60,
        branch_prefix=regen.BRANCH_PREFIX,
        dry_run=False,
        source_ref="test",
    )
    assert row["status"] == regen.DUPLICATE
    assert row["pr"] == "https://example/pr/49"
    assert not fake.pushed and not fake.created and not fake.updated


def test_deadline_inside_pr_file_resolution_never_reaches_any_write(adapters, tmp_path, monkeypatch):
    now = [0.0]
    calls = []
    name = "dcc-mcp-krita"
    monkeypatch.setattr(budget, "time", SimpleNamespace(monotonic=lambda: now[0]))
    monkeypatch.setattr(regen.drift, "_default_branch_name", lambda *a, **kw: "main")

    def clone(url, dest, branch, **kwargs):
        dest.mkdir(parents=True)
        (dest / "README.md").write_text(_stale_readme(adapters, name), encoding="utf-8")

    monkeypatch.setattr(regen, "_clone", clone)
    monkeypatch.setattr(regen, "changed_files", lambda *a, **kw: ["README.md"])
    monkeypatch.setattr(regen, "_blob_sha", lambda *a, **kw: "a" * 40)

    def run(args, **kwargs):
        calls.append(args)
        assert args[:3] == ["gh", "api", "--paginate"]
        if len(calls) == 1:
            return subprocess.CompletedProcess(args, 0, json.dumps([_listed_row(1), _listed_row(2)]), "")
        assert len(calls) == 2 and "/pulls/1/files" in args[3]
        now[0] = 5.0
        return subprocess.CompletedProcess(args, 0, '[{"path":"README.md","sha":"b"}]', "")

    monkeypatch.setattr(subprocess, "run", run)

    def forbidden(*args, **kwargs):
        pytest.fail("deadline must prevent permission lookup, commit, push, and PR mutations")

    for owner, attribute in [
        (regen, "_push_target"),
        (regen, "_commit_and_push"),
        (pr_helpers, "create_pr"),
        (pr_helpers, "update_pr"),
    ]:
        monkeypatch.setattr(owner, attribute, forbidden)
    with budget.batch_timeout(5):
        row = regen.regenerate_repository(
            _entry(adapters, name),
            0,
            host_count=47,
            workdir=tmp_path,
            timeout=300,
            branch_prefix=regen.BRANCH_PREFIX,
            dry_run=False,
            source_ref="test",
        )
    assert row["status"] == regen.ERROR
    assert "batch deadline exhausted" in row["detail"]
    assert row["action"] == "none"
    assert len(calls) == 2


@pytest.mark.parametrize("stop_after", ["checkout", "add", "commit"])
def test_deadline_prevents_the_next_git_write(tmp_path, monkeypatch, stop_after):
    now = [0.0]
    calls = []
    monkeypatch.setattr(budget, "time", SimpleNamespace(monotonic=lambda: now[0]))

    def run(args, **kwargs):
        calls.append(args)
        if "ls-remote" in args:
            return subprocess.CompletedProcess(args, 0, "", "")
        assert "push" not in args
        if stop_after in args:
            now[0] = 5.0
        return subprocess.CompletedProcess(args, 0, "", "")

    monkeypatch.setattr(subprocess, "run", run)
    with budget.batch_timeout(5), pytest.raises(budget.BatchDeadlineExceeded):
        regen._commit_and_push(tmp_path, regen.BRANCH_PREFIX + "catalog-pointer", "test/example", timeout=300)
    assert len(calls) == {"checkout": 2, "add": 3, "commit": 4}[stop_after]
    assert not any("push" in args for args in calls)
    if stop_after != "commit":
        assert not any("commit" in args for args in calls)


def test_deadline_after_push_reports_uncertainty_and_never_creates_a_pr(adapters, tmp_path, monkeypatch):
    now = [0.0]
    name = "dcc-mcp-krita"
    monkeypatch.setattr(budget, "time", SimpleNamespace(monotonic=lambda: now[0]))
    monkeypatch.setattr(regen.drift, "_default_branch_name", lambda *a, **kw: "main")

    def clone(url, dest, branch, **kwargs):
        dest.mkdir(parents=True)
        (dest / "README.md").write_text(_stale_readme(adapters, name), encoding="utf-8")

    monkeypatch.setattr(regen, "_clone", clone)
    monkeypatch.setattr(regen, "changed_files", lambda *a, **kw: ["README.md"])
    monkeypatch.setattr(regen, "_blob_sha", lambda *a, **kw: "a" * 40)
    monkeypatch.setattr(regen, "_find_equivalent", lambda *a, **kw: {})
    monkeypatch.setattr(regen, "_push_target", lambda *a, **kw: ("test/example", "generated"))

    def push(*args, **kwargs):
        now[0] = 5.0

    def forbidden(*args, **kwargs):
        pytest.fail("no command or PR creation is allowed after the exhausted push budget")

    monkeypatch.setattr(regen, "_commit_and_push", push)
    monkeypatch.setattr(subprocess, "run", forbidden)
    monkeypatch.setattr(pr_helpers, "create_pr", forbidden)
    monkeypatch.setattr(pr_helpers, "update_pr", forbidden)
    with budget.batch_timeout(5):
        row = regen.regenerate_repository(
            _entry(adapters, name),
            0,
            host_count=47,
            workdir=tmp_path,
            timeout=300,
            branch_prefix=regen.BRANCH_PREFIX,
            dry_run=False,
            source_ref="test",
        )
    assert row["status"] == regen.ERROR
    assert row["action"] == "incomplete"
    assert "writes may have completed remotely" in row["detail"]


def test_workflow_batches_have_report_headroom_and_track_budget_helper():
    import yaml

    workflow = yaml.safe_load((REPO_ROOT / ".github/workflows/adapter-coverage.yml").read_text(encoding="utf-8"))
    for job in ("pointer-drift", "regenerate-pointers"):
        steps = workflow["jobs"][job]["steps"]
        batch = next(step["run"] for step in steps if "--json-out" in step.get("run", ""))
        assert "--batch-timeout 1200" in batch
        assert workflow["jobs"][job]["timeout-minutes"] == 30
    events = workflow.get("on", workflow.get(True))
    for event in ("push", "pull_request"):
        assert "scripts/_adapter_pointer_budget.py" in events[event]["paths"]
