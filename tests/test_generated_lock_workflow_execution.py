"""Execute the generated-lock workflow's command boundaries with local fixtures."""

from __future__ import annotations

import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
from types import SimpleNamespace

import pytest

from conftest import REPO_ROOT
from dcc_mcp_core import yaml_loads
from test_uv_lock_consistency import _trusted_validator_source

WORKFLOW_PATH = REPO_ROOT / ".github" / "workflows" / "release-please-lock-sync.yml"


def _workflow_steps():
    workflow = yaml_loads(WORKFLOW_PATH.read_text(encoding="utf-8"))
    return workflow["jobs"]["sync-cargo-metadata"]["steps"]


def _logical_commands(step):
    return [
        line.strip()
        for line in step.get("run", "").replace("\\\n", "").splitlines()
        if line.strip() and not line.lstrip().startswith("#")
    ]


def _clean_environment(root: Path) -> dict[str, str]:
    git = shutil.which("git")
    assert git is not None
    path = [str(Path(git).parent), str(Path(sys.executable).parent)]
    env = {key: os.environ[key] for key in ("SYSTEMROOT", "WINDIR") if key in os.environ}
    if os.name == "nt":
        path.extend([str(Path(env["SYSTEMROOT"]) / "System32"), env["SYSTEMROOT"]])
    else:
        path.extend(os.defpath.split(os.pathsep))
    env.update(
        {
            "PATH": os.pathsep.join(path),
            "HOME": str(root),
            "USERPROFILE": str(root),
            "TEMP": str(root),
            "TMP": str(root),
            "GIT_CONFIG_NOSYSTEM": "1",
            "GIT_CONFIG_GLOBAL": os.devnull,
            "GIT_TERMINAL_PROMPT": "0",
            "GIT_ALLOW_PROTOCOL": "file",
            "PYTHONDONTWRITEBYTECODE": "1",
        }
    )
    return env


def _git(cwd: Path, env: dict[str, str], *args: str) -> str:
    result = subprocess.run(
        [
            "git",
            "-c",
            "user.name=loonghao",
            "-c",
            "user.email=hal.long@outlook.com",
            "-c",
            "commit.gpgsign=false",
            *args,
        ],
        cwd=cwd,
        env=env,
        capture_output=True,
        text=True,
        timeout=30,
    )
    assert result.returncode == 0, result.stdout + result.stderr
    return result.stdout.rstrip("\r\n")


@pytest.fixture
def push_repository(tmp_path: Path):
    env = _clean_environment(tmp_path)
    bare = tmp_path / "remote repository.git"
    work = tmp_path / "candidate checkout"
    _git(tmp_path, env, "init", "--bare", "-q", str(bare))
    _git(tmp_path, env, "init", "-q", str(work))
    (work / "Cargo.lock").write_bytes(b"baseline\n")
    _git(work, env, "add", "Cargo.lock")
    _git(work, env, "commit", "-qm", "test: seed lock fixture")
    _git(work, env, "push", "-q", str(bare), "HEAD:main")
    expected = _git(work, env, "rev-parse", "HEAD")
    (work / "Cargo.lock").write_bytes(b"generated\n")
    _git(work, env, "commit", "-qam", "test: generate lock fixture")
    candidate = _git(work, env, "rev-parse", "HEAD")
    marker = tmp_path / "candidate-hook-ran"
    hook = work / ".git" / "hooks" / "pre-push"
    hook.write_bytes(f"#!/bin/sh\nprintf 'synthetic' > '{marker.as_posix()}'\nexit 1\n".encode())
    hook.chmod(0o755)
    credential_file = tmp_path / "empty credential store"
    credential_file.write_bytes(b"")
    return SimpleNamespace(
        root=tmp_path,
        env=env,
        bare=bare,
        work=work,
        expected=expected,
        candidate=candidate,
        marker=marker,
        credential_file=credential_file,
    )


def _workflow_push_invocation(repository):
    step = next(step for step in _workflow_steps() if step.get("name") == "Push fixed generated lock commit")
    commands = [command for command in _logical_commands(step) if command.startswith("env -u http_proxy ")]
    assert len(commands) == 1
    argv = shlex.split(commands[0])
    assert argv.pop(0) == "env"
    env = dict(repository.env)
    while argv[0] == "-u":
        argv.pop(0)
        env.pop(argv.pop(0), None)
    assert argv[0] == "git"
    # Only the destination and quoted fixture values change; Git option order
    # is taken verbatim from the actual workflow, not rebuilt by this test.
    assert argv[-2] == "https://github.com/${GITHUB_REPOSITORY}.git"
    argv[-2] = str(repository.bare)
    values = {
        "${credential_file}": str(repository.credential_file),
        "${HEAD_REF}": "main",
        "${EXPECTED_HEAD_SHA}": repository.expected,
    }
    for index, argument in enumerate(argv):
        for variable, value in values.items():
            argument = argument.replace(variable, value)
        assert "$" not in argument
        argv[index] = argument
    return argv, env


def test_workflow_push_updates_expected_branch_without_candidate_hook(push_repository) -> None:
    repository = push_repository
    argv, env = _workflow_push_invocation(repository)
    result = subprocess.run(argv, cwd=repository.work, env=env, capture_output=True, text=True, timeout=30)

    assert result.returncode == 0, result.stdout + result.stderr
    assert _git(repository.bare, env, "rev-parse", "refs/heads/main") == repository.candidate
    assert _git(repository.work, env, "ls-remote", str(repository.bare), "refs/heads/main") == (
        f"{repository.candidate}\trefs/heads/main"
    )
    assert not repository.marker.exists()


def test_workflow_push_hook_control_detects_omitted_no_verify(push_repository) -> None:
    repository = push_repository
    argv, env = _workflow_push_invocation(repository)
    argv.remove("--no-verify")
    result = subprocess.run(argv, cwd=repository.work, env=env, capture_output=True, text=True, timeout=30)

    assert result.returncode == 1, result.stdout + result.stderr
    assert repository.marker.read_bytes() == b"synthetic"
    assert _git(repository.bare, env, "rev-parse", "refs/heads/main") == repository.expected


def test_workflow_push_rejects_stale_lease_without_remote_mutation(push_repository) -> None:
    repository = push_repository
    other = repository.root / "concurrent checkout"
    _git(repository.root, repository.env, "clone", "-q", "--branch", "main", str(repository.bare), str(other))
    _git(other, repository.env, "commit", "--allow-empty", "-qm", "test: advance remote fixture")
    _git(other, repository.env, "push", "-q", str(repository.bare), "HEAD:main")
    advanced = _git(repository.bare, repository.env, "rev-parse", "refs/heads/main")
    argv, env = _workflow_push_invocation(repository)
    result = subprocess.run(argv, cwd=repository.work, env=env, capture_output=True, text=True, timeout=30)

    assert result.returncode == 1, result.stdout + result.stderr
    assert "stale info" in result.stderr
    assert _git(repository.bare, env, "rev-parse", "refs/heads/main") == advanced
    assert not repository.marker.exists()


def _workflow_validator_invocations():
    invocations = []
    for step in _workflow_steps():
        for command in _logical_commands(step):
            if "trusted-lock-validator show" not in command:
                continue
            pipeline = shlex.split(command)
            assert pipeline.count("|") == 1
            separator = pipeline.index("|")
            source, argv = pipeline[:separator], pipeline[separator + 1 :]
            removed = []
            if source[:3] == ["env", "-u", "PUSH_TOKEN"]:
                source = source[3:]
            assert source == [
                "git",
                "-C",
                "../trusted-lock-validator",
                "show",
                "$TRUSTED_VALIDATOR_REF:scripts/ci/generated_lock_sync.py",
            ]
            if argv[:3] == ["env", "-u", "PUSH_TOKEN"]:
                removed.append("PUSH_TOKEN")
                argv = argv[3:]
            assert argv[0] == "python"
            invocations.append((step["name"] + "/" + argv[-1], argv, removed))
    assert len(invocations) == 7
    return invocations


def _shadow_fixture(root: Path, vector: str):
    candidate = root / "candidate"
    candidate.mkdir()
    env = _clean_environment(root)
    if vector == "cwd":
        shadow_root = candidate
    else:
        shadow_root = root / "pythonpath"
        shadow_root.mkdir()
        env["PYTHONPATH"] = str(shadow_root)
    (shadow_root / "argparse.py").write_bytes(b'raise SystemExit("SYNTHETIC_WORKFLOW_IMPORT_SHADOW")\n')
    return candidate, env


@pytest.mark.parametrize("invocation", _workflow_validator_invocations(), ids=lambda invocation: invocation[0])
@pytest.mark.parametrize("vector", ["cwd", "pythonpath"])
def test_workflow_validator_ignores_candidate_import_shadow(tmp_path: Path, invocation, vector: str) -> None:
    source = _trusted_validator_source()
    candidate, env = _shadow_fixture(tmp_path, vector)
    _, argv, removed = invocation
    for name in removed:
        env.pop(name, None)
    # Keep each real subcommand, but stop at argparse before any generation,
    # network, identity checks, or writes can occur.
    result = subprocess.run(
        [sys.executable, *argv[1:], "--help"],
        cwd=candidate,
        env=env,
        input=source,
        capture_output=True,
        text=True,
        timeout=30,
    )

    assert result.returncode == 0, result.stdout + result.stderr
    assert "usage:" in result.stdout and "verify-commit" in result.stdout
    assert result.stderr == ""


@pytest.mark.parametrize("vector", ["cwd", "pythonpath"])
def test_workflow_validator_shadow_control_detects_omitted_isolation(tmp_path: Path, vector: str) -> None:
    source = _trusted_validator_source()
    candidate, env = _shadow_fixture(tmp_path, vector)
    _, argv, _ = _workflow_validator_invocations()[0]
    assert "-I" in argv
    ordinary = [argument for argument in argv[1:] if argument != "-I"]
    result = subprocess.run(
        [sys.executable, *ordinary, "--help"],
        cwd=candidate,
        env=env,
        input=source,
        capture_output=True,
        text=True,
        timeout=30,
    )

    assert result.returncode == 1
    assert result.stdout == ""
    assert result.stderr.strip() == "SYNTHETIC_WORKFLOW_IMPORT_SHADOW"


def test_workflow_validator_verifies_real_diff_with_candidate_shadow(push_repository) -> None:
    source = _trusted_validator_source()
    repository = push_repository
    (repository.work / "argparse.py").write_bytes(b'raise SystemExit("SYNTHETIC_WORKFLOW_IMPORT_SHADOW")\n')
    _git(repository.work, repository.env, "add", "argparse.py")
    _git(repository.work, repository.env, "commit", "-qm", "test: track candidate import fixture")
    lock = repository.work / "Cargo.lock"
    lock.write_bytes(b"regenerated\n")
    status = _git(repository.work, repository.env, "status", "--porcelain=v1", "--untracked-files=all")
    assert status == " M Cargo.lock"
    _, argv, _ = next(
        invocation for invocation in _workflow_validator_invocations() if invocation[1][-1] == "verify-diff"
    )
    result = subprocess.run(
        [sys.executable, *argv[1:]],
        cwd=repository.work,
        env=repository.env,
        input=source,
        capture_output=True,
        text=True,
        timeout=30,
    )

    assert result.returncode == 0, result.stdout + result.stderr
    assert lock.read_bytes() == b"regenerated\n"
    assert _git(repository.work, repository.env, "status", "--porcelain=v1", "--untracked-files=all") == status
    assert not repository.marker.exists()


RESOLVE_STEP = "Resolve current pull request head"
RESOLVED_HEAD_EXPRESSION = "${{ env.PR_HEAD_SHA || github.event.pull_request.head.sha }}"
RESOLVED_LEASE_EXPRESSION = "${{ env.EXPECTED_HEAD_SHA || github.event.pull_request.head.sha }}"


def _workflow_step(name):
    return next(step for step in _workflow_steps() if step.get("name") == name)


def test_workflow_resolves_live_head_immediately_before_generation() -> None:
    """The surviving concurrency winner can carry a stale event head SHA."""
    step = _workflow_step(RESOLVE_STEP)
    assert step["working-directory"] == "pull-request"
    run = step["run"]
    assert 'gh api "repos/$GITHUB_REPOSITORY/pulls/$PR_NUMBER" --jq .head.sha' in run
    assert 'git fetch --no-tags origin "refs/heads/$PR_HEAD_REF"' in run
    assert 'git checkout --detach "$live_sha"' in run
    assert "printf 'PR_HEAD_SHA=%s\\n' \"$live_sha\"" in run
    assert "printf 'EXPECTED_HEAD_SHA=%s\\n' \"$live_sha\"" in run
    assert '>> "$GITHUB_ENV"' in run
    assert "exit 1" in run
    assert step["env"]["EVENT_HEAD_SHA"] == "${{ github.event.pull_request.head.sha }}"
    assert step["env"]["PR_HEAD_REF"] == "${{ github.event.pull_request.head.ref }}"
    assert step["env"]["PR_NUMBER"] == "${{ github.event.pull_request.number }}"
    assert "PUSH_TOKEN" not in step.get("env", {})
    names = [step.get("name") for step in _workflow_steps()]
    assert names.index(RESOLVE_STEP) == names.index("Sync generated lock metadata") - 1


def test_workflow_identity_steps_use_resolved_head_sha() -> None:
    """Validation and the fixed lease must bind the resolved head, not the event."""
    revalidate = _workflow_step("Revalidate pull request identity and generated diff")
    assert revalidate["env"]["PR_HEAD_SHA"] == RESOLVED_HEAD_EXPRESSION
    push = _workflow_step("Push fixed generated lock commit")
    assert push["env"]["PR_HEAD_SHA"] == RESOLVED_HEAD_EXPRESSION
    assert push["env"]["EXPECTED_HEAD_SHA"] == RESOLVED_LEASE_EXPRESSION


def _resolve_fixture(tmp_path: Path):
    """Model the failure: event head stale, branch already force-pushed on."""
    env = _clean_environment(tmp_path)
    bare = tmp_path / "resolve remote.git"
    work = tmp_path / "stale checkout"
    other = tmp_path / "release automation"
    _git(tmp_path, env, "init", "--bare", "-q", str(bare))
    _git(tmp_path, env, "init", "-q", str(work))
    _git(work, env, "checkout", "-q", "-b", "main")
    (work / "Cargo.lock").write_bytes(b"stale\n")
    _git(work, env, "add", "Cargo.lock")
    _git(work, env, "commit", "-qm", "test: stale release head")
    _git(work, env, "push", "-q", str(bare), "HEAD:main")
    _git(work, env, "remote", "add", "origin", str(bare))
    stale = _git(work, env, "rev-parse", "HEAD")
    _git(tmp_path, env, "clone", "-q", "--branch", "main", str(bare), str(other))
    (other / "Cargo.lock").write_bytes(b"live\n")
    _git(other, env, "commit", "-qam", "test: live release head")
    _git(other, env, "push", "-q", str(bare), "HEAD:main")
    live = _git(other, env, "rev-parse", "HEAD")
    assert live != stale
    bin_dir = tmp_path / "stub bin"
    bin_dir.mkdir()
    stub = bin_dir / "gh"
    stub.write_text('#!/bin/sh\nprintf "%s\\n" "$STUB_HEAD_SHA"\n', encoding="utf-8")
    stub.chmod(0o755)
    github_env = tmp_path / "github env"
    github_env.write_bytes(b"")
    script = tmp_path / "resolve step.sh"
    script.write_text(_workflow_step(RESOLVE_STEP)["run"], encoding="utf-8")
    run_env = dict(env)
    run_env["PATH"] = str(bin_dir) + os.pathsep + env["PATH"]
    run_env.update(
        {
            "GITHUB_REPOSITORY": "dcc-mcp/dcc-mcp-core",
            "PR_NUMBER": "2486",
            "PR_HEAD_REF": "main",
            "EVENT_HEAD_SHA": stale,
            "GITHUB_ENV": str(github_env),
            "STUB_HEAD_SHA": live,
        }
    )
    return SimpleNamespace(
        env=run_env,
        work=work,
        script=script,
        github_env=github_env,
        stale=stale,
        live=live,
    )


_RESOLVE_SHELL = pytest.mark.skipif(
    os.name == "nt" or shutil.which("bash") is None,
    reason="resolve step executes through the workflow's POSIX shell",
)


@_RESOLVE_SHELL
def test_workflow_resolve_step_checks_out_live_head(tmp_path: Path) -> None:
    fixture = _resolve_fixture(tmp_path)
    result = subprocess.run(
        ["bash", str(fixture.script)],
        cwd=fixture.work,
        env=fixture.env,
        capture_output=True,
        text=True,
        timeout=60,
    )

    assert result.returncode == 0, result.stdout + result.stderr
    assert _git(fixture.work, fixture.env, "rev-parse", "HEAD") == fixture.live
    assert (fixture.work / "Cargo.lock").read_bytes() == b"live\n"
    exported = fixture.github_env.read_text(encoding="utf-8")
    assert "PR_HEAD_SHA=" + fixture.live in exported
    assert "EXPECTED_HEAD_SHA=" + fixture.live in exported


@_RESOLVE_SHELL
def test_workflow_resolve_step_keeps_current_head_without_fetching(tmp_path: Path) -> None:
    fixture = _resolve_fixture(tmp_path)
    fixture.env["STUB_HEAD_SHA"] = fixture.stale
    # Any fetch would fail against this remote, so success proves the step
    # short-circuits while the event head is still current.
    _git(fixture.work, fixture.env, "remote", "set-url", "origin", str(tmp_path / "absent remote.git"))
    result = subprocess.run(
        ["bash", str(fixture.script)],
        cwd=fixture.work,
        env=fixture.env,
        capture_output=True,
        text=True,
        timeout=60,
    )

    assert result.returncode == 0, result.stdout + result.stderr
    assert _git(fixture.work, fixture.env, "rev-parse", "HEAD") == fixture.stale
    assert "PR_HEAD_SHA=" + fixture.stale in fixture.github_env.read_text(encoding="utf-8")


@_RESOLVE_SHELL
def test_workflow_resolve_step_rejects_unresolvable_head(tmp_path: Path) -> None:
    fixture = _resolve_fixture(tmp_path)
    fixture.env["STUB_HEAD_SHA"] = "f" * 40
    result = subprocess.run(
        ["bash", str(fixture.script)],
        cwd=fixture.work,
        env=fixture.env,
        capture_output=True,
        text=True,
        timeout=60,
    )

    assert result.returncode != 0
    assert _git(fixture.work, fixture.env, "rev-parse", "HEAD") == fixture.stale
    assert fixture.github_env.read_text(encoding="utf-8") == ""
