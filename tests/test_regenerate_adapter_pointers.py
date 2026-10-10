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

    def __init__(
        self,
        *,
        login: str = "bot",
        fork: str = "",
        existing_pr: str = "",
        push_state: str = pr_helpers.PUSH_GRANTED,
    ):
        self.login = login
        self.fork = fork
        self.existing_pr = existing_pr
        self.push_state = push_state
        self.created: list[dict] = []
        self.updated: list[dict] = []
        self.pushed: list[str] = []

    def install(self, monkeypatch) -> None:
        monkeypatch.setattr(pr_helpers, "authenticated_login", lambda **kw: self.login)
        monkeypatch.setattr(pr_helpers, "check_push_access", lambda repo, **kw: self.push_state)
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

    fake = _FakeGitHub(login="bot")
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

    fake = _FakeGitHub(login="bot")
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
    fake = _FakeGitHub(login="bot")
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
    """Denied push access must fail the run, even when a personal fork exists."""
    name = "dcc-mcp-krita"
    origin = _make_origin(tmp_path, "openprdeniedfork", _stale_readme(adapters, name))
    # A fork is deliberately supplied: the point is that it must NOT be used.
    fake = _FakeGitHub(login="bot", fork="bot/dcc-mcp-krita", push_state=pr_helpers.PUSH_DENIED)
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
    # Nothing was pushed, and certainly not to the fork.
    assert fake.pushed == []
    assert not any("bot/dcc-mcp-krita" in pushed for pushed in fake.pushed)
    assert fake.created == [] and fake.updated == []


def test_a_stale_repository_with_an_unknown_permission_fails_instead_of_using_a_fork(adapters, tmp_path, monkeypatch):
    """A failed permission lookup must fail the run, even when a fork exists."""
    name = "dcc-mcp-krita"
    origin = _make_origin(tmp_path, "openprunknownfork", _stale_readme(adapters, name))
    fake = _FakeGitHub(login="bot", fork="bot/dcc-mcp-krita", push_state=pr_helpers.PUSH_UNKNOWN)
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


def test_an_existing_pull_request_is_refreshed_not_duplicated(adapters, tmp_path, monkeypatch):
    """A second run updates the open PR instead of opening another one."""
    name = "dcc-mcp-krita"
    origin = _make_origin(tmp_path, "refresh", _stale_readme(adapters, name))
    fake = _FakeGitHub(login="bot", existing_pr="https://github.com/example/pr/7")
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


def test_an_org_member_with_push_uses_the_upstream_repo(monkeypatch):
    """The whole point: login != owner but push is granted, so no fork."""
    calls = []

    def fake_run(args, **kwargs):
        calls.append(args)
        return subprocess.CompletedProcess(args, 0, "true\n", "")

    monkeypatch.setattr(pr_helpers, "_run", fake_run)
    monkeypatch.setattr(pr_helpers, "authenticated_login", lambda **kw: "loonghao")

    assert pr_helpers.ensure_push_target("dcc-mcp/dcc-mcp-maya") == "dcc-mcp/dcc-mcp-maya"
    # Only the permission lookup ran. No fork was queried or created.
    assert len(calls) == 1
    assert calls[0][:3] == ["gh", "api", "repos/dcc-mcp/dcc-mcp-maya"]
    assert not any("fork" in " ".join(call) for call in calls)


def test_the_owner_with_push_uses_the_upstream_repo(monkeypatch):
    """Ownership alone is not the test any more, but push access still short-circuits."""
    monkeypatch.setattr(pr_helpers, "_run", _push_run("true\n"))
    monkeypatch.setattr(pr_helpers, "authenticated_login", lambda **kw: "dcc-mcp")

    assert pr_helpers.ensure_push_target("dcc-mcp/dcc-mcp-maya") == "dcc-mcp/dcc-mcp-maya"


def test_denied_push_raises_and_never_touches_a_fork(monkeypatch):
    """Denied access must raise, and no fork lookup may even be attempted."""
    calls = []

    def fake_run(args, **kwargs):
        calls.append(args)
        return subprocess.CompletedProcess(args, 0, "false\n", "")

    monkeypatch.setattr(pr_helpers, "_run", fake_run)
    monkeypatch.setattr(pr_helpers, "authenticated_login", lambda **kw: "loonghao")

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
    monkeypatch.setattr(pr_helpers, "authenticated_login", lambda **kw: "loonghao")

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
    monkeypatch.setattr(pr_helpers, "authenticated_login", lambda **kw: "loonghao")

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
    monkeypatch.setattr(pr_helpers, "authenticated_login", lambda **kw: "loonghao")

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
        monkeypatch.setattr(pr_helpers, "authenticated_login", lambda **kw: "loonghao")

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
        return subprocess.CompletedProcess(args, 0, '[{"url":"https://example/pr/9"}]', "")

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
    test suite would notice, because no test performs a real push.

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
