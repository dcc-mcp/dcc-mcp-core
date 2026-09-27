# Release workflow digest drift check — shared kit

Binds `.github/workflows/release.yml` to an approved snapshot, so an unreviewed
edit to a workflow that holds PyPI Trusted Publishing credentials (`id-token:
write`) fails CI with a visible digest diff instead of landing silently between
two release tags.

It is a **drift check, not an approval gate**: the snapshot is an ordinary
tracked file refreshed by an ordinary pull request. The previous trust-root
approval path deadlocked on single-maintainer repositories because no
independent approver existed; this restores tamper evidence and an audit trail
without re-creating that deadlock.

**Do not configure it as a required status check.** It is evidence, not a
merge gate.

## Files

| File | Installed as |
| --- | --- |
| `check_release_workflow_digest.py` | `scripts/ci/check_release_workflow_digest.py` |
| `test_release_workflow_integrity.py` | `tests/test_release_workflow_integrity.py` |
| (generated from the workflow) | `scripts/ci/approved_release_workflow.yml` |

Both shipped files are byte-identical in every repository. They contain no
repository-specific content: `ROOT` is `Path(__file__).resolve().parents[2]` in
the checker and `parents[1]` in the test, and the test loads the checker from
its path with `importlib` rather than importing a package. That is what makes
the kit a copy instead of a port.

## Install into a new repository

```bash
python scripts/ci/release_workflow_integrity/install_release_workflow_integrity.py \
    --target ../dcc-mcp-unreal
```

The installer copies both files, writes the snapshot from the target's
`.github/workflows/release.yml`, and verifies the snapshot it just wrote, so a
repository cannot land with a red check. Existing files are kept unless
`--force` is passed.

## What to change when moving to a new repository

In the normal case: **nothing**. The kit is location-driven. The three things
that do vary are all handled by the installer or by existing configuration:

1. **Repository root.** Derived from each file's own location. Nothing to edit.
2. **Release workflow path.** Default `.github/workflows/release.yml`
   (repository-root relative). `--workflow <relative/path>` makes the installer
   snapshot a differently located file, but the checker resolves its own
   `RELEASE_WORKFLOW` constant, so a repository that keeps the file elsewhere
   must change that one line in the installed copy as well.
3. **CI collection.** `pytest tests/` collects the suite with no extra
   configuration, because the test loads the checker from its path. Two
   caveats:
   - PyYAML must be importable in the test environment. Where it is not, the
     module skips through `pytest.importorskip` instead of failing collection.
     Add PyYAML to the test dependencies, or run the suite in a job that
     installs it explicitly (see `release-workflow-integrity` in
     `dcc-mcp-core`'s `ci.yml`).
   - The suite is ruff-formatted and Python 3.7 syntax clean; keep it that way
     if a repository lints `tests/` or runs a py37 syntax gate.

## Refreshing the snapshot after an intentional workflow change

```bash
python scripts/ci/check_release_workflow_digest.py --update-snapshot
git add scripts/ci/approved_release_workflow.yml
```

Commit the refresh in the **same pull request** as the workflow change, so
reviewers see both halves of one decision. Do not use a commit type that
release-please reads as a bump signal — `chore(ci)` and `chore(lock)` are
verified safe; `refactor` is not.

## What the digest ignores and what it catches

Ignored (cosmetic): comments, blank lines, mapping key order, CRLF versus LF,
quoting style.

Caught (meaning): any structural change — a new permission, a re-pinned action,
a dropped publish assertion — and, inside a `run:` block, any byte change at
all, because a block scalar is a shell script whose whitespace is content
(a trailing space after a `\` line continuation, or a blank line inside a
heredoc, both change what bash executes).

`tests/test_release_workflow_integrity.py` proves both directions: real drift
is caught and cosmetic drift is not. Both halves are required — one without the
other is either a false-alarm generator or no check at all.

## Keeping the kit copies in sync

`dcc-mcp-core` carries this directory as the upstream copy and its own
`tests/test_release_workflow_integrity_kit.py` asserts that the installed files
match these templates byte for byte, so a local edit to one repository cannot
silently fork the kit.
