"""Release lock projection and consistency contract tests."""

from __future__ import annotations

import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time
from types import SimpleNamespace

import pytest

from conftest import REPO_ROOT
from dcc_mcp_core import yaml_loads

SCRIPT_PATH = REPO_ROOT / "scripts" / "ci" / "check_uv_lock.py"
LOCK_SYNC_WORKFLOW = REPO_ROOT / ".github" / "workflows" / "release-please-lock-sync.yml"
CI_WORKFLOW = REPO_ROOT / ".github" / "workflows" / "ci.yml"
VERSION_WORKFLOW = REPO_ROOT / ".github" / "workflows" / "version-consistency.yml"
EXPECTED_ROOT_PACKAGE = "dcc-mcp-core"
TRUSTED_VALIDATOR_PATH = "scripts/ci/generated_lock_sync.py"
TRUSTED_VALIDATOR_COMMIT = "3ee3cbf1445a5f3a909788443c7bdbec0c2ca3da"


def _load_checker_module():
    spec = importlib.util.spec_from_file_location("check_uv_lock", SCRIPT_PATH)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _write_version_files(
    root: Path,
    *,
    project_version: str,
    lock_version: str,
    package_name: str = EXPECTED_ROOT_PACKAGE,
    project_name: str = EXPECTED_ROOT_PACKAGE,
    lock_name: str = EXPECTED_ROOT_PACKAGE,
    extra_lock_package: str = "",
) -> None:
    (root / "release-please-config.json").write_text(
        json.dumps({"packages": {".": {"package-name": package_name}}}),
        encoding="utf-8",
    )
    (root / ".release-please-manifest.json").write_text(
        json.dumps({".": project_version}),
        encoding="utf-8",
    )
    (root / "pyproject.toml").write_text(
        f'[project]\nname = "{project_name}"\nversion = "{project_version}"\n',
        encoding="utf-8",
    )
    (root / "uv.lock").write_text(
        f'version = 1\n\n[[package]]\nname = "{lock_name}"\n'
        f'version = "{lock_version}"\nsource = {{ editable = "." }}\n{extra_lock_package}',
        encoding="utf-8",
    )


def _workflow_step_commands(workflow_text: str, *, job: str, step_name: str) -> list[str]:
    workflow = yaml_loads(workflow_text)
    assert isinstance(workflow, dict)
    jobs = workflow.get("jobs")
    assert isinstance(jobs, dict)
    selected_job = jobs.get(job)
    assert isinstance(selected_job, dict)
    steps = selected_job.get("steps")
    assert isinstance(steps, list)
    matches = [step for step in steps if isinstance(step, dict) and step.get("name") == step_name]
    assert len(matches) == 1
    run = matches[0].get("run")
    assert isinstance(run, str)
    return [line.strip() for line in run.splitlines() if line.strip() and not line.lstrip().startswith("#")]


def _workflow_pull_request_paths(workflow_text: str) -> set[str]:
    workflow = yaml_loads(workflow_text)
    assert isinstance(workflow, dict)
    trigger = workflow.get("on", workflow.get(True))
    assert isinstance(trigger, dict)
    pull_request = trigger.get("pull_request", trigger.get("pull_request_target"))
    assert isinstance(pull_request, dict)
    paths = pull_request.get("paths")
    assert isinstance(paths, list)
    assert all(isinstance(path, str) for path in paths)
    return set(paths)


def _trusted_validator_ref() -> str:
    workflow = yaml_loads(LOCK_SYNC_WORKFLOW.read_text(encoding="utf-8"))
    assert isinstance(workflow, dict)
    job = workflow["jobs"]["sync-cargo-metadata"]
    ref = job["env"]["TRUSTED_VALIDATOR_REF"]
    trusted = next(step for step in job["steps"] if step.get("name") == "Checkout trusted lock validator")
    assert trusted["with"]["ref"] == ref
    return ref


def _trusted_validator_source() -> str:
    ref = _trusted_validator_ref()
    result = subprocess.run(
        ["git", "show", f"{ref}:{TRUSTED_VALIDATOR_PATH}"],
        check=False,
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        # Most matrix jobs intentionally use a shallow checkout. The native
        # Python 3.7 jobs retain full history and are authoritative here.
        pytest.skip("trusted validator ref is unavailable in shallow checkout")
    return result.stdout


def test_stale_editable_root_version_is_rejected(tmp_path: Path) -> None:
    checker = _load_checker_module()
    _write_version_files(tmp_path, project_version="0.20.15", lock_version="0.20.14")

    errors = checker.check_uv_lock_consistency(tmp_path)

    assert errors == ["uv.lock editable root dcc-mcp-core version '0.20.14' != expected '0.20.15'"]


def test_matching_editable_root_version_is_accepted(tmp_path: Path) -> None:
    checker = _load_checker_module()
    _write_version_files(tmp_path, project_version="0.20.15", lock_version="0.20.15")

    assert checker.check_uv_lock_consistency(tmp_path) == []


def test_synchronized_root_package_rename_is_rejected(tmp_path: Path) -> None:
    checker = _load_checker_module()
    _write_version_files(
        tmp_path,
        project_version="0.20.15",
        lock_version="0.20.15",
        package_name="renamed-core",
        project_name="renamed-core",
        lock_name="renamed-core",
    )

    assert checker.check_uv_lock_consistency(tmp_path) == [
        "release-please-config.json root package-name 'renamed-core' != fixed identity 'dcc-mcp-core'"
    ]


def test_second_editable_shadow_root_is_rejected(tmp_path: Path) -> None:
    checker = _load_checker_module()
    _write_version_files(
        tmp_path,
        project_version="0.20.15",
        lock_version="0.20.15",
        extra_lock_package=('\n[[package]]\nname = "shadow-root"\nversion = "0.20.15"\nsource = { editable = "." }\n'),
    )

    assert checker.check_uv_lock_consistency(tmp_path) == [
        "uv.lock must contain exactly one source.editable='.' root package; found 2"
    ]


def test_invalid_project_version_is_rejected(tmp_path: Path) -> None:
    checker = _load_checker_module()
    _write_version_files(tmp_path, project_version="latest", lock_version="latest")

    assert checker.check_uv_lock_consistency(tmp_path) == [
        ".release-please-manifest.json root version 'latest' is not a valid project version"
    ]


def test_uv_lock_symlink_is_rejected_even_when_bytes_match(tmp_path: Path) -> None:
    checker = _load_checker_module()
    _write_version_files(tmp_path, project_version="0.20.15", lock_version="0.20.15")
    lock_path = tmp_path / "uv.lock"
    target_path = tmp_path / "uv.lock.target"
    lock_path.replace(target_path)
    lock_path.symlink_to(target_path.name)

    assert checker.check_uv_lock_consistency(tmp_path) == [
        "uv.lock must be a regular file and not a symlink or reparse point"
    ]


def test_uv_lock_directory_is_rejected_as_non_regular(tmp_path: Path) -> None:
    checker = _load_checker_module()
    _write_version_files(tmp_path, project_version="0.20.15", lock_version="0.20.15")
    lock_path = tmp_path / "uv.lock"
    lock_path.unlink()
    lock_path.mkdir()

    assert checker.check_uv_lock_consistency(tmp_path) == [
        "uv.lock must be a regular file and not a symlink or reparse point"
    ]


@pytest.mark.parametrize(
    ("path", "content", "expected"),
    [
        (
            ".release-please-manifest.json",
            "[]",
            ".release-please-manifest.json must contain a JSON object",
        ),
        (
            "release-please-config.json",
            '{"packages": []}',
            "release-please-config.json packages must be a mapping",
        ),
        (
            "pyproject.toml",
            'project = "invalid"\n',
            "pyproject.toml project must be a mapping",
        ),
        (
            "uv.lock",
            'version = 1\npackage = "invalid"\n',
            "uv.lock package must be a list",
        ),
        (
            "uv.lock",
            'version = 1\npackage = ["invalid"]\n',
            "uv.lock package entries must be mappings",
        ),
    ],
)
def test_malformed_shapes_return_stable_errors(
    tmp_path: Path,
    path: str,
    content: str,
    expected: str,
) -> None:
    checker = _load_checker_module()
    _write_version_files(tmp_path, project_version="0.20.15", lock_version="0.20.15")
    (tmp_path / path).write_text(content, encoding="utf-8")

    assert checker.check_uv_lock_consistency(tmp_path) == [expected]


def test_release_workflows_regenerate_and_validate_uv_lock() -> None:
    sync_workflow = LOCK_SYNC_WORKFLOW.read_text(encoding="utf-8")
    version_workflow = VERSION_WORKFLOW.read_text(encoding="utf-8")

    sync_commands = _workflow_step_commands(
        sync_workflow,
        job="sync-cargo-metadata",
        step_name="Sync generated lock metadata",
    )
    assert any("trusted-lock-validator show" in command and "python - generate" in command for command in sync_commands)
    commit_commands = _workflow_step_commands(
        sync_workflow,
        job="sync-cargo-metadata",
        step_name="Commit and push changes",
    )
    assert "git add Cargo.lock uv.lock crates/workspace-hack/Cargo.toml" in commit_commands
    assert any("--no-verify" in command for command in commit_commands)

    check_commands = _workflow_step_commands(
        version_workflow,
        job="version-consistency",
        step_name="Check uv lock consistency",
    )
    assert check_commands == ["python scripts/ci/check_uv_lock.py", "vx uv lock --check"]
    assert {
        "release-please-config.json",
        ".release-please-manifest.json",
        "pyproject.toml",
        "uv.lock",
        "scripts/ci/check_uv_lock.py",
        ".github/workflows/version-consistency.yml",
    }.issubset(_workflow_pull_request_paths(version_workflow))


def test_generated_lock_workflow_is_read_only_until_fixed_push() -> None:
    workflow = yaml_loads(LOCK_SYNC_WORKFLOW.read_text(encoding="utf-8"))
    assert isinstance(workflow, dict)
    assert workflow["permissions"]["contents"] == "read"
    job = workflow["jobs"]["sync-cargo-metadata"]
    assert job["env"]["TRUSTED_VALIDATOR_REF"] == TRUSTED_VALIDATOR_COMMIT
    trusted = next(step for step in job["steps"] if step.get("name") == "Checkout trusted lock validator")
    assert trusted["with"]["ref"] == TRUSTED_VALIDATOR_COMMIT
    assert trusted["with"]["persist-credentials"] is False
    pin = next(step for step in job["steps"] if step.get("name") == "Verify single-file trusted lock validator object")
    assert "$RUNNER_TEMP/generated_lock_sync.py" not in pin["run"]
    assert "pinned, self-contained object" in pin["run"]
    assert "git -C trusted-lock-validator cat-file -e" in pin["run"]
    checkouts = [step for step in job["steps"] if step.get("uses", "").startswith("actions/checkout@")]
    assert len(checkouts) == 2
    checkout = checkouts[0]
    assert checkout["with"]["path"] == "trusted-lock-validator"
    assert checkout["with"]["persist-credentials"] is False
    pr_checkout = checkouts[1]
    assert pr_checkout["name"] == "Checkout pull request head"
    assert pr_checkout["with"]["path"] == "pull-request"
    assert pr_checkout["with"]["persist-credentials"] is False
    remote_check = next(step for step in job["steps"] if step.get("name") == "Validate checkout remote")
    assert (
        "git -C ../trusted-lock-validator show" in remote_check["run"]
        and "python - validate-remote" in remote_check["run"]
    )
    assert remote_check["working-directory"] == "pull-request"
    generation = next(step for step in job["steps"] if step.get("name") == "Sync generated lock metadata")
    assert "../trusted-lock-validator show" in generation["run"] and "python - generate" in generation["run"]
    assert generation["working-directory"] == "pull-request"
    assert "PUSH_TOKEN" not in generation.get("env", {})
    push = next(step for step in job["steps"] if step.get("name") == "Push fixed generated lock commit")
    assert push["if"] == "steps.commit.outputs.changed == 'true'"
    assert "trap" in push["run"] and 'rm -f "$credential_file"' in push["run"]
    assert "--force-with-lease" in push["run"]
    assert "env -u PUSH_TOKEN git -C ../trusted-lock-validator show" in push["run"]
    assert push["working-directory"] == "pull-request"
    trigger_paths = _workflow_pull_request_paths(LOCK_SYNC_WORKFLOW.read_text(encoding="utf-8"))
    assert {
        "scripts/ci/generated_lock_sync.py",
        ".github/workflows/release-please-lock-sync.yml",
    }.issubset(trigger_paths)
    assert "scripts/ci/windows_process_job.py" not in trigger_paths
    assert "PUSH_TOKEN" not in push["env"]
    assert "credential.helper" in push["run"]
    # Clear any PR-controlled local helper before installing the ephemeral
    # trusted store helper; otherwise `credential.helper=!…` in .git/config
    # can execute during the PAT-bearing push.
    assert '-c credential.helper= \\\n  -c credential.helper="store --file=${credential_file}"' in push["run"]
    assert "-c http.proxy=" in push["run"]
    assert "-c https.proxy=" in push["run"]
    assert "-c core.gitProxy=" in push["run"]
    assert '"https://github.com/${GITHUB_REPOSITORY}.git"' in push["run"]
    assert "env -u http_proxy -u https_proxy -u HTTP_PROXY -u HTTPS_PROXY" in push["run"]
    assert "validate-remote" in push["run"]
    assert "--no-verify" in push["run"]
    assert "../trusted-lock-validator show" in push["run"] and "python - verify-commit" in push["run"]


def test_trusted_validator_overwrite_is_detected_before_execution() -> None:
    workflow_text = LOCK_SYNC_WORKFLOW.read_text(encoding="utf-8")
    assert "$RUNNER_TEMP/generated_lock_sync.py" not in workflow_text
    assert workflow_text.count("trusted-lock-validator show") >= 5


def test_pr_checkout_cannot_clean_trusted_validator_checkout() -> None:
    workflow = yaml_loads(LOCK_SYNC_WORKFLOW.read_text(encoding="utf-8"))
    assert isinstance(workflow, dict)
    job = workflow["jobs"]["sync-cargo-metadata"]
    steps = job["steps"]
    checkouts = [step for step in steps if step.get("uses", "").startswith("actions/checkout@")]
    assert checkouts[0]["with"]["path"] == "trusted-lock-validator"
    assert checkouts[1]["with"]["path"] == "pull-request"
    for name in (
        "Validate checkout remote",
        "Sync generated lock metadata",
        "Revalidate pull request identity and generated diff",
        "Commit and push changes",
        "Push fixed generated lock commit",
    ):
        step = next(step for step in steps if step.get("name") == name)
        assert step["working-directory"] == "pull-request"
    assert all(
        "git -C trusted-lock-validator" not in step.get("run", "")
        for step in steps
        if step.get("name") != "Verify single-file trusted lock validator object"
    )


@pytest.mark.skipif(os.name != "nt", reason="Windows trusted-stdin execution contract")
def test_trusted_validator_generate_runs_from_stdin_on_windows(tmp_path: Path) -> None:
    """The exact validator object must remain executable through ``python -``."""
    validator_source = _trusted_validator_source()
    fake_bin = tmp_path / "bin"
    fake_bin.mkdir()
    where = Path(os.environ["SYSTEMROOT"]) / "System32" / "where.exe"
    for name in ("cargo", "vx", "update", "-w", "hakari", "generate", "uv", "lock"):
        shutil.copy2(where, fake_bin / f"{name}.exe")
    env = dict(os.environ)
    env["PATH"] = str(fake_bin) + os.pathsep + env.get("PATH", "")

    result = subprocess.run(
        [sys.executable, "-", "generate", "--root", str(tmp_path)],
        cwd=tmp_path,
        env=env,
        input=validator_source,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        timeout=30,
    )
    assert result.returncode == 0, result.stdout


def test_generated_lock_windows_contract_documents_job_object() -> None:
    guide = (REPO_ROOT / "docs" / "guide" / "adapter-release-checklist.md").read_text(encoding="utf-8")
    section = guide.split("### Generated-lock credential boundary", 1)[1].split("### CHANGELOG Convention", 1)[0]
    contract = " ".join(section.split())
    assert "taskkill /T" not in contract
    assert "CREATE_SUSPENDED" in contract
    assert "primary-thread handle" in contract
    assert "Job Object" in contract
    assert "breakaway" in contract
    assert "bounded cleanup" in contract
    assert "immutable validator commit" in contract
    assert "later commit" in contract


@pytest.mark.skipif(os.name == "nt", reason="credential helper shell contract is POSIX-only")
def test_ephemeral_push_helper_ignores_pr_local_helper(tmp_path: Path) -> None:
    """A PR-controlled local helper must not run while resolving push creds."""
    work = tmp_path / "work"
    subprocess.run(["git", "init", "-q", str(work)], check=True, timeout=30)
    marker = tmp_path / "helper-ran"
    probe = tmp_path / "malicious-helper.sh"
    probe.write_text(f"#!/bin/sh\nprintf '%s' \"$PUSH_TOKEN\" > '{marker}'\nexit 1\n", encoding="utf-8")
    probe.chmod(0o755)
    subprocess.run(
        ["git", "-C", str(work), "config", "credential.helper", f"!{probe}"],
        check=True,
        timeout=30,
    )
    credential_file = tmp_path / "credentials"
    credential_file.write_text("https://x-access-token:ephemeral-pat@github.com\n", encoding="utf-8")
    env = {**os.environ, "GIT_CONFIG_NOSYSTEM": "1", "GIT_TERMINAL_PROMPT": "0", "PUSH_TOKEN": "parent-secret"}
    result = subprocess.run(
        [
            "git",
            "-C",
            str(work),
            "-c",
            "credential.helper=",
            "-c",
            f"credential.helper=store --file={credential_file}",
            "credential",
            "fill",
        ],
        input="protocol=https\nhost=github.com\n\n",
        text=True,
        capture_output=True,
        env=env,
        check=False,
        timeout=30,
    )
    assert result.returncode == 0, result.stderr
    assert "password=ephemeral-pat" in result.stdout
    assert not marker.exists(), "PR-controlled local credential helper executed"


def test_generation_uses_ephemeral_credential_roots(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    script = REPO_ROOT / "scripts" / "ci" / "generated_lock_sync.py"
    spec = importlib.util.spec_from_file_location("generated_lock_sync_home", script)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    captured: list[dict[str, str]] = []

    def fake_run_bounded(command, *, cwd, env, timeout_seconds):
        captured.append(env)

    monkeypatch.setattr(module, "run_bounded", fake_run_bounded)
    module.run_generation(tmp_path, timeout_seconds=1)
    assert len(captured) == 3
    env = captured[0]
    assert env["HOME"] != os.environ.get("HOME")
    assert env["USERPROFILE"] != os.environ.get("USERPROFILE")
    assert env["XDG_CONFIG_HOME"] != os.environ.get("XDG_CONFIG_HOME")
    assert env["CARGO_HOME"] != os.environ.get("CARGO_HOME")
    assert env["PIP_CONFIG_FILE"] != os.environ.get("PIP_CONFIG_FILE")
    assert not Path(env["HOME"]).exists(), "temporary credential root escaped cleanup"


def test_trusted_validator_ref_attests_self_contained_job_object() -> None:
    source = _trusted_validator_source()

    assert "class WindowsJob:" in source
    assert "WINDOWS_CREATE_SUSPENDED" in source
    assert "ResumeThread" in source
    assert "taskkill" not in source
    assert "_kill_windows_tree" not in source

    result = subprocess.run(
        [sys.executable, "-", "--help"],
        check=False,
        cwd=REPO_ROOT,
        input=source,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        timeout=30,
    )
    assert result.returncode == 0, result.stdout


def test_native_python37_windows_executes_pinned_validator_contract() -> None:
    workflow = yaml_loads(CI_WORKFLOW.read_text(encoding="utf-8"))
    assert isinstance(workflow, dict)
    steps = workflow["jobs"]["python37-native"]["steps"]
    install = next(step for step in steps if step.get("name") == "Install Python 3.7 test toolchain")
    assert install["if"] == "matrix.full_suite || runner.os == 'Windows'"
    contract = next(step for step in steps if step.get("name") == "Run trusted validator Python 3.7 contract")
    assert contract["if"] == "runner.os == 'Windows'"
    assert "tests/test_uv_lock_consistency.py" in contract["run"]
    assert "trusted_validator_ref_attests_self_contained_job_object" in contract["run"]
    assert "trusted_validator_generate_runs_from_stdin_on_windows" in contract["run"]


def test_generated_lock_contract_rejects_fork_and_identity_drift() -> None:
    script = REPO_ROOT / "scripts" / "ci" / "generated_lock_sync.py"
    spec = importlib.util.spec_from_file_location("generated_lock_sync", script)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)

    expected = module.PullRequestIdentity(
        repository="dcc-mcp/dcc-mcp-core",
        number=2381,
        head_repository="dcc-mcp/dcc-mcp-core",
        head_branch="renovate/uv-lock",
        head_sha="a" * 40,
        title="chore(deps): update locks",
    )
    assert module.validate_identity(expected, expected) == []
    fork = expected._replace(head_repository="attacker/dcc-mcp-core")
    assert any("fork" in error for error in module.validate_identity(expected, fork))
    drift = expected._replace(head_sha="b" * 40)
    assert any("head SHA" in error for error in module.validate_identity(expected, drift))


def test_generation_environment_cannot_expose_write_credentials(tmp_path: Path) -> None:
    script = REPO_ROOT / "scripts" / "ci" / "generated_lock_sync.py"
    spec = importlib.util.spec_from_file_location("generated_lock_sync_env", script)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    env = module.sanitized_environment(
        {
            **os.environ,
            "GITHUB_TOKEN": "write-secret",
            "GH_TOKEN": "write-secret",
            "PERSONAL_ACCESS_TOKEN": "write-secret",
            "GIT_CONFIG_GLOBAL": "C:/credential-store",
        },
        isolated_home=tmp_path / "isolated-home",
    )
    assert all("secret" not in value for value in env.values())
    assert "GITHUB_TOKEN" not in env and "GH_TOKEN" not in env and "PERSONAL_ACCESS_TOKEN" not in env
    assert env["GIT_CONFIG_NOSYSTEM"] == "1"
    assert env["GIT_TERMINAL_PROMPT"] == "0"

    probe = tmp_path / "hostile_lock_backend_probe.py"
    probe.write_text(
        "import os, subprocess, sys\n"
        "if any(os.environ.get(k) for k in ('GITHUB_TOKEN', 'GH_TOKEN', 'PERSONAL_ACCESS_TOKEN')): sys.exit(7)\n"
        "result = subprocess.run(['git', 'config', '--global', '--get-regexp', 'credential'], capture_output=True)\n"
        "sys.exit(8 if result.stdout else 0)\n",
        encoding="utf-8",
    )
    result = subprocess.run([sys.executable, str(probe)], env=env, check=False)
    assert result.returncode == 0


def test_generated_diff_is_exactly_bounded() -> None:
    script = REPO_ROOT / "scripts" / "ci" / "generated_lock_sync.py"
    spec = importlib.util.spec_from_file_location("generated_lock_sync_diff", script)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    assert module.validate_changed_files(["Cargo.lock", "uv.lock"]) == []
    assert module.validate_changed_files(["Cargo.lock", "evil.py"]) == ["unexpected generated-lock diff paths: evil.py"]


def test_remote_urls_are_bound_to_expected_owner_and_repo() -> None:
    script = REPO_ROOT / "scripts" / "ci" / "generated_lock_sync.py"
    spec = importlib.util.spec_from_file_location("generated_lock_sync_remote", script)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    expected = "dcc-mcp/dcc-mcp-core"
    assert module.validate_remote_url("https://github.com/dcc-mcp/dcc-mcp-core.git", expected) == []
    assert module.validate_remote_url("git@github.com:dcc-mcp/dcc-mcp-core.git", expected) == []
    assert module.validate_remote_url("https://attacker.example/dcc-mcp/dcc-mcp-core.git", expected)
    assert module.validate_remote_url("https://github.com/attacker/repo.git", expected)
    assert module.validate_remote_urls(
        ["https://github.com/dcc-mcp/dcc-mcp-core.git", "https://attacker.example/pwn.git"], expected, "push"
    )


def test_validate_remote_enumerates_and_rejects_multiple_urls(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    script = REPO_ROOT / "scripts" / "ci" / "generated_lock_sync.py"
    spec = importlib.util.spec_from_file_location("generated_lock_sync_remote_enumeration", script)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    calls: list[tuple[str, ...]] = []

    def fake_run(command, **kwargs):
        calls.append(tuple(command))
        return SimpleNamespace(returncode=0, stdout="https://github.com/dcc-mcp/dcc-mcp-core.git\n" * 2)

    monkeypatch.setattr(module.subprocess, "run", fake_run)
    with pytest.raises(SystemExit):
        module.validate_remote(tmp_path, "dcc-mcp/dcc-mcp-core")
    assert calls and calls[0][-3:] == ("get-url", "--all", "origin")


@pytest.mark.parametrize(
    "proxy_key", ["http.https://github.com/.proxy", "http.*.proxy", "remote.origin.proxy", "core.gitProxy"]
)
def test_validate_remote_rejects_local_proxy_configuration(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, proxy_key: str
) -> None:
    script = REPO_ROOT / "scripts" / "ci" / "generated_lock_sync.py"
    spec = importlib.util.spec_from_file_location("generated_lock_sync_proxy_guard", script)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)

    def fake_run(command, **kwargs):
        if command[:3] == ("git", "remote", "get-url"):
            return SimpleNamespace(returncode=0, stdout="https://github.com/dcc-mcp/dcc-mcp-core.git\n")
        return SimpleNamespace(returncode=0, stdout=f"{proxy_key}\n")

    monkeypatch.setattr(module.subprocess, "run", fake_run)
    with pytest.raises(SystemExit):
        module.validate_remote(tmp_path, "dcc-mcp/dcc-mcp-core")


def test_git_helper_uses_hard_timeout(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    script = REPO_ROOT / "scripts" / "ci" / "generated_lock_sync.py"
    spec = importlib.util.spec_from_file_location("generated_lock_sync_git_timeout", script)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    observed: dict[str, object] = {}

    def fake_run(command, **kwargs):
        observed.update(kwargs)
        raise subprocess.TimeoutExpired(command, kwargs["timeout"])

    monkeypatch.setattr(module.subprocess, "run", fake_run)
    with pytest.raises(RuntimeError, match="git command timed out"):
        module._git(tmp_path, "diff", "--name-only")
    assert isinstance(observed.get("timeout"), (int, float))
    assert observed["timeout"] > 0


@pytest.mark.skipif(sys.platform == "darwin", reason="macOS process execution fails closed")
def test_bounded_runner_kills_process_descendants(tmp_path: Path) -> None:
    script = REPO_ROOT / "scripts" / "ci" / "generated_lock_sync.py"
    spec = importlib.util.spec_from_file_location("generated_lock_sync_timeout", script)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    child_pid = tmp_path / "child.pid"
    grandchild_pid = tmp_path / "grandchild.pid"
    probe = tmp_path / "descendants.py"
    probe.write_text(
        "import os, subprocess, sys, time\n"
        "from pathlib import Path\n"
        "Path(sys.argv[1]).write_text(str(os.getpid()))\n"
        "subprocess.Popen([sys.executable, '-c', \"from pathlib import Path; import os,sys,time; Path(sys.argv[1]).write_text(str(os.getpid())); time.sleep(60)\", sys.argv[2]], start_new_session=True)\n"
        "time.sleep(60)\n",
        encoding="utf-8",
    )
    with pytest.raises(subprocess.TimeoutExpired):
        module.run_bounded([sys.executable, str(probe), str(child_pid), str(grandchild_pid)], timeout_seconds=1.0)
    assert child_pid.exists() and grandchild_pid.exists()
    for pid_path in (child_pid, grandchild_pid):
        pid = int(pid_path.read_text())
        for _ in range(20):
            if not module.process_exists(pid):
                break
            time.sleep(0.05)
        assert not module.process_exists(pid)


@pytest.mark.skipif(
    os.name == "nt" or sys.platform == "darwin",
    reason="requires a provable POSIX process reaper; macOS fails closed",
)
def test_bounded_runner_fails_closed_on_escaped_daemon(tmp_path: Path) -> None:
    script = REPO_ROOT / "scripts" / "ci" / "generated_lock_sync.py"
    spec = importlib.util.spec_from_file_location("generated_lock_sync_escaped_daemon", script)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    daemon_pid = tmp_path / "daemon.pid"
    probe = tmp_path / "escaped_daemon.py"
    probe.write_text(
        "import os, subprocess, sys, time\n"
        "from pathlib import Path\n"
        "subprocess.Popen([sys.executable, '-c', \"from pathlib import Path; import os,sys,time; Path(sys.argv[1]).write_text(str(os.getpid())); time.sleep(60)\", sys.argv[1]], start_new_session=True)\n"
        "time.sleep(0.2)\n"
        "os._exit(0)\n",
        encoding="utf-8",
    )
    try:
        with pytest.raises(RuntimeError):
            module.run_bounded([sys.executable, str(probe), str(daemon_pid)], timeout_seconds=5)
    finally:
        if daemon_pid.exists():
            pid = int(daemon_pid.read_text())
            if module.process_exists(pid):
                if os.name == "nt":
                    subprocess.run(("taskkill", "/PID", str(pid), "/T", "/F"), check=False, timeout=5)
                else:
                    os.kill(pid, 9)
    assert not daemon_pid.exists() or not module.process_exists(int(daemon_pid.read_text()))


@pytest.mark.skipif(os.name != "nt", reason="Windows process-tree contract")
def test_bounded_runner_fails_closed_on_windows_normal_daemon(tmp_path: Path) -> None:
    script = REPO_ROOT / "scripts" / "ci" / "generated_lock_sync.py"
    spec = importlib.util.spec_from_file_location("generated_lock_sync_windows_daemon", script)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    daemon_pid = tmp_path / "daemon.pid"
    probe = tmp_path / "windows_daemon.py"
    probe.write_text(
        "import subprocess, sys, time\n"
        "from pathlib import Path\n"
        "subprocess.Popen([sys.executable, '-c', \"from pathlib import Path; import os,sys,time; Path(sys.argv[1]).write_text(str(os.getpid())); time.sleep(60)\", sys.argv[1]])\n"
        "time.sleep(0.2)\n",
        encoding="utf-8",
    )
    try:
        with pytest.raises(RuntimeError):
            module.run_bounded([sys.executable, str(probe), str(daemon_pid)], timeout_seconds=5)
    finally:
        if daemon_pid.exists() and module.process_exists(int(daemon_pid.read_text())):
            subprocess.run(("taskkill", "/PID", daemon_pid.read_text(), "/T", "/F"), check=False, timeout=5)


@pytest.mark.skipif(os.name != "nt", reason="Windows process-tree contract")
def test_bounded_runner_contains_windows_last_fork_after_leader_exit(tmp_path: Path) -> None:
    script = REPO_ROOT / "scripts" / "ci" / "generated_lock_sync.py"
    spec = importlib.util.spec_from_file_location("generated_lock_sync_windows_last_fork", script)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    child_pid = tmp_path / "child.pid"
    probe = tmp_path / "windows_last_fork.py"
    probe.write_text(
        "import os, subprocess, sys, time\n"
        "from pathlib import Path\n"
        # Let the first observer pass see an empty tree, then fork and exit
        # before its next pass. PID-only walks cannot rediscover the orphan.
        "time.sleep(0.1)\n"
        "subprocess.Popen([sys.executable, '-c', \"from pathlib import Path; import os,sys,time; Path(sys.argv[1]).write_text(str(os.getpid())); time.sleep(60)\", sys.argv[1]], start_new_session=True)\n"
        "deadline = time.monotonic() + 0.2\n"
        "while not Path(sys.argv[1]).exists() and time.monotonic() < deadline: time.sleep(0.005)\n"
        "os._exit(0)\n",
        encoding="utf-8",
    )
    try:
        with pytest.raises(RuntimeError, match="descendants survived"):
            module.run_bounded([sys.executable, str(probe), str(child_pid)], timeout_seconds=5)
    finally:
        if child_pid.exists() and module.process_exists(int(child_pid.read_text())):
            subprocess.run(("taskkill", "/PID", child_pid.read_text(), "/T", "/F"), check=False, timeout=5)


@pytest.mark.skipif(os.name != "nt", reason="Windows process-tree contract")
def test_bounded_runner_allows_windows_normal_exit() -> None:
    script = REPO_ROOT / "scripts" / "ci" / "generated_lock_sync.py"
    spec = importlib.util.spec_from_file_location("generated_lock_sync_windows_normal_exit", script)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    module.run_bounded([sys.executable, "-c", "raise SystemExit(0)"], timeout_seconds=5)


@pytest.mark.skipif(os.name != "nt", reason="Windows process-tree contract")
def test_bounded_runner_does_not_kill_unrelated_windows_process() -> None:
    script = REPO_ROOT / "scripts" / "ci" / "generated_lock_sync.py"
    spec = importlib.util.spec_from_file_location("generated_lock_sync_windows_unrelated", script)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    unrelated = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)"])
    try:
        with pytest.raises(subprocess.TimeoutExpired):
            module.run_bounded([sys.executable, "-c", "import time; time.sleep(60)"], timeout_seconds=0.2)
        assert unrelated.poll() is None
    finally:
        unrelated.terminate()
        unrelated.wait(timeout=5)


@pytest.mark.skipif(os.name != "nt", reason="Windows process-tree contract")
def test_windows_create_process_retains_and_closes_returned_handles(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    script = REPO_ROOT / "scripts" / "ci" / "generated_lock_sync.py"
    spec = importlib.util.spec_from_file_location("generated_lock_sync_create_handles", script)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    observed: dict[str, object] = {}
    closed: list[int] = []

    class FakeWinApi:
        def CreateProcess(self, *args):
            observed["command_line"] = args[1]
            observed["creation_flags"] = args[5]
            observed["cwd"] = args[7]
            return 202, 404, 303, 505

    class FakeKernel32:
        def CloseHandle(self, handle):
            closed.append(handle.value)
            return 1

    fake_kernel32 = FakeKernel32()
    monkeypatch.setattr(module, "_winapi", FakeWinApi())
    monkeypatch.setattr(module.ctypes, "WinDLL", lambda *args, **kwargs: fake_kernel32)
    monkeypatch.setattr(module, "_configure_process_api", lambda _kernel32: None)

    process = module._create_suspended_windows_process(
        ["fake.exe", "argument with spaces"],
        cwd=tmp_path,
        env={"SAFE": "1"},
    )
    assert process.process_handle == 202
    assert process._thread_handle == 404
    assert process.pid == 303
    assert observed == {
        "command_line": 'fake.exe "argument with spaces"',
        "creation_flags": getattr(subprocess, "CREATE_NEW_PROCESS_GROUP", 0) | module.WINDOWS_CREATE_SUSPENDED,
        "cwd": str(tmp_path),
    }
    process.close()
    assert closed == [404, 202]


@pytest.mark.skipif(os.name != "nt", reason="Windows process-tree contract")
def test_windows_primary_thread_resume_is_handle_bound_and_exactly_once() -> None:
    script = REPO_ROOT / "scripts" / "ci" / "generated_lock_sync.py"
    spec = importlib.util.spec_from_file_location("generated_lock_sync_thread_handle", script)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    calls: list[tuple[str, int]] = []

    class FakeKernel32:
        def ResumeThread(self, thread_handle):
            calls.append(("resume", thread_handle.value))
            # Another owner contributed two suspension levels. This wrapper
            # may release only its own CREATE_SUSPENDED level.
            return 3

        def CloseHandle(self, handle):
            calls.append(("close", handle.value if hasattr(handle, "value") else handle))
            return 1

    process = object.__new__(module._SuspendedWindowsProcess)
    process.pid = 303  # May already have been reused; resume must not consult it.
    process._thread_handle = 404
    process._process_handle = 202
    process._kernel32 = FakeKernel32()
    previous_count = process.resume_initial_thread()
    assert previous_count == 3
    assert calls == [("resume", 404), ("close", 404)]
    assert process._thread_handle is None


@pytest.mark.skipif(os.name != "nt", reason="Windows process-tree contract")
def test_windows_primary_thread_resume_failure_closes_owned_handle() -> None:
    script = REPO_ROOT / "scripts" / "ci" / "generated_lock_sync.py"
    spec = importlib.util.spec_from_file_location("generated_lock_sync_thread_resume_failure", script)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    calls: list[tuple[str, int]] = []

    class FakeKernel32:
        def ResumeThread(self, thread_handle):
            calls.append(("resume", thread_handle.value))
            return 0xFFFFFFFF

        def CloseHandle(self, handle):
            calls.append(("close", handle.value if hasattr(handle, "value") else handle))
            return 1

    process = object.__new__(module._SuspendedWindowsProcess)
    process._thread_handle = 404
    process._process_handle = 202
    process._kernel32 = FakeKernel32()
    with pytest.raises(RuntimeError, match="resume suspended process primary thread"):
        process.resume_initial_thread()
    assert calls == [("resume", 404), ("close", 404)]
    assert process._thread_handle is None


@pytest.mark.skipif(os.name != "nt", reason="Windows process-tree contract")
def test_windows_job_assignment_uses_owned_process_handle() -> None:
    script = REPO_ROOT / "scripts" / "ci" / "generated_lock_sync.py"
    spec = importlib.util.spec_from_file_location("generated_lock_sync_job_assignment", script)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    calls: list[tuple[int, int]] = []

    class FakeKernel32:
        def AssignProcessToJobObject(self, job_handle, process_handle):
            calls.append((job_handle, process_handle.value))
            return 1

    job = object.__new__(module.WindowsJob)
    job._handle = 101
    job._kernel32 = FakeKernel32()
    job.assign(SimpleNamespace(process_handle=202))
    assert calls == [(101, 202)]


@pytest.mark.skipif(os.name != "nt", reason="Windows process-tree contract")
def test_windows_resume_failure_terminates_job_before_closing_handles(monkeypatch: pytest.MonkeyPatch) -> None:
    script = REPO_ROOT / "scripts" / "ci" / "generated_lock_sync.py"
    spec = importlib.util.spec_from_file_location("generated_lock_sync_resume_cleanup", script)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    events: list[object] = []

    class FakeJob:
        def assign(self, process) -> None:
            events.append(("assign", process.process_handle))

        def terminate(self) -> None:
            events.append("terminate-job")

        def wait_empty(self, timeout_seconds: float) -> None:
            events.append(("wait-job-empty", timeout_seconds))

        def close(self) -> None:
            events.append("close-job")

    class FakeProcess:
        process_handle = 202
        returncode = None

        @property
        def pid(self) -> int:
            pytest.fail("resume consulted a reused PID instead of the primary-thread handle")

        def resume_initial_thread(self) -> int:
            events.append("resume-primary-thread")
            events.append("close-primary-thread")
            raise RuntimeError("synthetic handle-bound resume failure")

        def wait(self, timeout: float) -> int:
            events.append(("wait-process", timeout))
            self.returncode = 1
            return self.returncode

        def close(self) -> None:
            events.append("close-process")

    fake_job = FakeJob()
    monkeypatch.setattr(module, "WindowsJob", lambda: fake_job)
    monkeypatch.setattr(module, "_create_suspended_windows_process", lambda *args, **kwargs: FakeProcess())

    with pytest.raises(RuntimeError, match="could not establish Windows process containment"):
        module.run_bounded_windows(["fake"], cwd=None, env=None, timeout_seconds=0.2)
    assert events == [
        ("assign", 202),
        "resume-primary-thread",
        "close-primary-thread",
        "terminate-job",
        ("wait-job-empty", module.WINDOWS_JOB_WAIT_SECONDS),
        ("wait-process", module.WINDOWS_JOB_WAIT_SECONDS),
        "close-job",
        "close-process",
    ]


@pytest.mark.skipif(os.name == "nt" or sys.platform == "darwin", reason="POSIX observer contract")
def test_observer_failure_still_kills_timeout_process(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    script = REPO_ROOT / "scripts" / "ci" / "generated_lock_sync.py"
    spec = importlib.util.spec_from_file_location("generated_lock_sync_observer_failure", script)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    child_pid = tmp_path / "child.pid"
    probe = tmp_path / "sleeping.py"
    probe.write_text(
        "import os, sys, time\n"
        "from pathlib import Path\n"
        "Path(sys.argv[1]).write_text(str(os.getpid()))\n"
        "time.sleep(60)\n",
        encoding="utf-8",
    )
    calls = 0
    killed = False

    def fail_after_baseline(_pid: int) -> list[int]:
        nonlocal calls
        calls += 1
        if calls > 1:
            raise RuntimeError("synthetic observer failure")
        return []

    monkeypatch.setattr(module, "_descendant_pids", fail_after_baseline)

    def fake_killpg(_pid: int, _signal: int) -> None:
        nonlocal killed
        killed = True

    monkeypatch.setattr(module.os, "killpg", fake_killpg)
    with pytest.raises(RuntimeError):
        module.run_bounded([sys.executable, str(probe), str(child_pid)], timeout_seconds=0.2)
    assert killed
    if child_pid.exists():
        os.kill(int(child_pid.read_text()), 9)


@pytest.mark.skipif(os.name == "nt", reason="POSIX process probe contract")
def test_posix_process_probe_timeout_fails_closed(monkeypatch: pytest.MonkeyPatch) -> None:
    script = REPO_ROOT / "scripts" / "ci" / "generated_lock_sync.py"
    spec = importlib.util.spec_from_file_location("generated_lock_sync_posix_probe_timeout", script)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)

    def timeout_run(*args, **kwargs):
        raise subprocess.TimeoutExpired(args[0], kwargs.get("timeout"))

    monkeypatch.setattr(module.subprocess, "run", timeout_run)
    with pytest.raises(RuntimeError, match="process enumeration timed out"):
        module._descendant_pids(1234)


@pytest.mark.skipif(os.name == "nt", reason="POSIX process probe contract")
def test_posix_process_probe_malformed_output_fails_closed(monkeypatch: pytest.MonkeyPatch) -> None:
    script = REPO_ROOT / "scripts" / "ci" / "generated_lock_sync.py"
    spec = importlib.util.spec_from_file_location("generated_lock_sync_posix_probe_malformed", script)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)

    monkeypatch.setattr(
        module.subprocess,
        "run",
        lambda *args, **kwargs: SimpleNamespace(returncode=0, stdout="not-a-pid still-not-a-pid\n"),
    )
    with pytest.raises(RuntimeError, match="malformed output"):
        module._descendant_pids(1234)


def test_commit_no_verify_blocks_malicious_pre_commit_hook(tmp_path: Path) -> None:
    script = REPO_ROOT / "scripts" / "ci" / "generated_lock_sync.py"
    spec = importlib.util.spec_from_file_location("generated_lock_sync_pre_commit", script)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    work = tmp_path / "work"
    subprocess.run(["git", "init", "-q", str(work)], check=True, timeout=30)
    subprocess.run(["git", "-C", str(work), "config", "user.email", "test@example.invalid"], check=True, timeout=30)
    subprocess.run(["git", "-C", str(work), "config", "user.name", "test"], check=True, timeout=30)
    marker = tmp_path / "hook-ran"
    hook = work / ".git" / "hooks" / "pre-commit"
    hook.write_text(f"#!/bin/sh\nprintf x > '{marker}'\nexit 1\n", encoding="utf-8")
    hook.chmod(0o755)
    (work / "Cargo.lock").write_text("base\n", encoding="utf-8")
    subprocess.run(["git", "-C", str(work), "add", "Cargo.lock"], check=True, timeout=30)
    result = subprocess.run(["git", "-C", str(work), "commit", "--no-verify", "-qm", "base"], check=False, timeout=30)
    assert result.returncode == 0
    assert not marker.exists()


@pytest.mark.skipif(
    os.name == "nt" or sys.platform == "darwin",
    reason="requires a provable POSIX process reaper; macOS fails closed",
)
def test_bounded_runner_catches_last_fork_after_leader_exit(tmp_path: Path) -> None:
    script = REPO_ROOT / "scripts" / "ci" / "generated_lock_sync.py"
    spec = importlib.util.spec_from_file_location("generated_lock_sync_last_fork", script)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    grandchild_pid = tmp_path / "grandchild.pid"
    probe = tmp_path / "last_fork.py"
    probe.write_text(
        "import os, subprocess, sys, time\n"
        "from pathlib import Path\n"
        "leader = os.getpid()\n"
        "subprocess.Popen([sys.executable, '-c', \"import os,subprocess,sys,time; from pathlib import Path; leader=int(sys.argv[2]); pid_file=sys.argv[1];\\nwhile os.getppid() == leader: time.sleep(0.01)\\nchild=subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(60)'], start_new_session=True); Path(pid_file).write_text(str(child.pid)); time.sleep(60)\", sys.argv[1], str(leader)], start_new_session=True)\n"
        # Give the observer a bounded opportunity to record the intermediate
        # setsid child.  The child then forks only after its leader exits,
        # reproducing the macOS reparent/final-snapshot race.
        "time.sleep(0.1)\n"
        "os._exit(0)\n",
        encoding="utf-8",
    )
    try:
        with pytest.raises(RuntimeError, match="descendants survived"):
            module.run_bounded([sys.executable, str(probe), str(grandchild_pid)], timeout_seconds=5)
    finally:
        if grandchild_pid.exists() and module.process_exists(int(grandchild_pid.read_text())):
            os.kill(int(grandchild_pid.read_text()), 9)


@pytest.mark.skipif(sys.platform != "darwin", reason="macOS fail-closed contract")
def test_bounded_runner_fails_closed_before_macos_generation(monkeypatch: pytest.MonkeyPatch) -> None:
    script = REPO_ROOT / "scripts" / "ci" / "generated_lock_sync.py"
    spec = importlib.util.spec_from_file_location("generated_lock_sync_macos_guard", script)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    with pytest.raises(RuntimeError, match="unavailable on macOS"):
        module.run_bounded([sys.executable, "-c", "raise SystemExit(99)"], timeout_seconds=5)


def test_stale_head_and_unexpected_diff_contracts_block_push() -> None:
    script = REPO_ROOT / "scripts" / "ci" / "generated_lock_sync.py"
    spec = importlib.util.spec_from_file_location("generated_lock_sync_push", script)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    assert module.validate_force_with_lease("a" * 40, "a" * 40) == []
    assert module.validate_force_with_lease("a" * 40, "b" * 40)
    assert module.validate_changed_files(["Cargo.lock", "unexpected.txt"])


def test_commit_parent_is_bound_to_original_pr_head(tmp_path: Path) -> None:
    script = REPO_ROOT / "scripts" / "ci" / "generated_lock_sync.py"
    spec = importlib.util.spec_from_file_location("generated_lock_sync_parent", script)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True)
    subprocess.run(["git", "-C", str(tmp_path), "config", "user.email", "test@example.invalid"], check=True)
    subprocess.run(["git", "-C", str(tmp_path), "config", "user.name", "test"], check=True)
    (tmp_path / "Cargo.lock").write_text("base\n", encoding="utf-8")
    subprocess.run(["git", "-C", str(tmp_path), "add", "Cargo.lock"], check=True)
    subprocess.run(["git", "-C", str(tmp_path), "commit", "-qm", "base"], check=True)
    parent = subprocess.check_output(["git", "-C", str(tmp_path), "rev-parse", "HEAD"], text=True).strip()
    (tmp_path / "Cargo.lock").write_text("generated\n", encoding="utf-8")
    subprocess.run(["git", "-C", str(tmp_path), "commit", "-qam", "generated"], check=True)
    module.verify_commit(tmp_path, parent)
    with pytest.raises(SystemExit):
        module.verify_commit(tmp_path, "f" * 40)


def test_no_verify_prevents_malicious_pre_push_hook_from_running(tmp_path: Path) -> None:
    bare = tmp_path / "remote.git"
    work = tmp_path / "work"
    subprocess.run(["git", "init", "--bare", "-q", str(bare)], check=True)
    subprocess.run(["git", "init", "-q", str(work)], check=True)
    subprocess.run(["git", "-C", str(work), "config", "user.email", "test@example.invalid"], check=True)
    subprocess.run(["git", "-C", str(work), "config", "user.name", "test"], check=True)
    (work / "Cargo.lock").write_text("base\n", encoding="utf-8")
    subprocess.run(["git", "-C", str(work), "add", "Cargo.lock"], check=True, timeout=30)
    subprocess.run(["git", "-C", str(work), "commit", "-qm", "base"], check=True, timeout=30)
    subprocess.run(["git", "-C", str(work), "branch", "-M", "main"], check=True, timeout=30)
    subprocess.run(["git", "-C", str(work), "remote", "add", "origin", str(bare)], check=True, timeout=30)
    subprocess.run(["git", "-C", str(work), "push", "-q", "origin", "main"], check=True, timeout=30)
    marker = tmp_path / "hook-ran"
    hook = work / ".git" / "hooks" / "pre-push"
    hook.write_text(f"#!/bin/sh\nprintf '%s' \"$PUSH_TOKEN\" > '{marker}'\nexit 1\n", encoding="utf-8")
    hook.chmod(0o755)
    (work / "Cargo.lock").write_text("generated\n", encoding="utf-8")
    subprocess.run(["git", "-C", str(work), "commit", "-qam", "generated"], check=True, timeout=30)
    result = subprocess.run(
        ["git", "-C", str(work), "push", "--no-verify", "origin", "HEAD:main"], check=False, timeout=30
    )
    assert result.returncode == 0
    assert not marker.exists()


def test_stale_force_lease_does_not_mutate_remote_ref(tmp_path: Path) -> None:
    bare = tmp_path / "remote.git"
    first = tmp_path / "first"
    second = tmp_path / "second"
    subprocess.run(["git", "init", "--bare", "-q", str(bare)], check=True)
    for work in (first, second):
        subprocess.run(["git", "clone", "-q", str(bare), str(work)], check=True)
        subprocess.run(["git", "-C", str(work), "config", "user.email", "test@example.invalid"], check=True)
        subprocess.run(["git", "-C", str(work), "config", "user.name", "test"], check=True)
    (first / "Cargo.lock").write_text("base\n", encoding="utf-8")
    subprocess.run(["git", "-C", str(first), "add", "Cargo.lock"], check=True)
    subprocess.run(["git", "-C", str(first), "commit", "-qm", "base"], check=True)
    subprocess.run(["git", "-C", str(first), "branch", "-M", "main"], check=True)
    subprocess.run(["git", "-C", str(first), "push", "-q", "origin", "main"], check=True)
    old_head = subprocess.check_output(["git", "-C", str(first), "rev-parse", "HEAD"], text=True).strip()
    subprocess.run(["git", "-C", str(second), "fetch", "-q", "origin", "main"], check=True)
    subprocess.run(["git", "-C", str(second), "checkout", "-q", "-B", "main", "origin/main"], check=True)
    (second / "Cargo.lock").write_text("remote-advance\n", encoding="utf-8")
    subprocess.run(["git", "-C", str(second), "commit", "-qam", "advance"], check=True)
    subprocess.run(["git", "-C", str(second), "push", "-q", "origin", "HEAD:main"], check=True)
    advanced = subprocess.check_output(["git", "-C", str(second), "rev-parse", "HEAD"], text=True).strip()
    (first / "Cargo.lock").write_text("generated\n", encoding="utf-8")
    subprocess.run(["git", "-C", str(first), "commit", "-qam", "generated"], check=True)
    result = subprocess.run(
        [
            "git",
            "-C",
            str(first),
            "push",
            f"--force-with-lease=refs/heads/main:{old_head}",
            "origin",
            "HEAD:main",
        ],
        check=False,
    )
    assert result.returncode != 0
    remote_head = subprocess.check_output(
        ["git", "--git-dir", str(bare), "rev-parse", "refs/heads/main"], text=True
    ).strip()
    assert remote_head == advanced


def test_unexpected_diff_blocks_before_remote_write(tmp_path: Path) -> None:
    script = REPO_ROOT / "scripts" / "ci" / "generated_lock_sync.py"
    spec = importlib.util.spec_from_file_location("generated_lock_sync_unexpected_diff", script)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    bare = tmp_path / "remote.git"
    work = tmp_path / "work"
    subprocess.run(["git", "init", "--bare", "-q", str(bare)], check=True)
    subprocess.run(["git", "init", "-q", str(work)], check=True)
    subprocess.run(["git", "-C", str(work), "config", "user.email", "test@example.invalid"], check=True)
    subprocess.run(["git", "-C", str(work), "config", "user.name", "test"], check=True)
    (work / "Cargo.lock").write_text("base\n", encoding="utf-8")
    subprocess.run(["git", "-C", str(work), "add", "Cargo.lock"], check=True)
    subprocess.run(["git", "-C", str(work), "commit", "-qm", "base"], check=True)
    subprocess.run(["git", "-C", str(work), "branch", "-M", "main"], check=True)
    subprocess.run(["git", "-C", str(work), "remote", "add", "origin", str(bare)], check=True)
    subprocess.run(["git", "-C", str(work), "push", "-q", "origin", "main"], check=True)
    base_head = subprocess.check_output(["git", "-C", str(work), "rev-parse", "HEAD"], text=True).strip()
    (work / "evil.py").write_text("print('unexpected')\n", encoding="utf-8")
    with pytest.raises(SystemExit):
        module.verify_diff(work)
    remote_head = subprocess.check_output(
        ["git", "--git-dir", str(bare), "rev-parse", "refs/heads/main"], text=True
    ).strip()
    assert remote_head == base_head


def test_comment_only_workflow_text_is_not_an_executable_command() -> None:
    workflow = VERSION_WORKFLOW.read_text(encoding="utf-8").replace(
        "          vx uv lock --check",
        "          # vx uv lock --check",
        1,
    )

    commands = _workflow_step_commands(
        workflow,
        job="version-consistency",
        step_name="Check uv lock consistency",
    )

    assert commands == ["python scripts/ci/check_uv_lock.py"]


def test_workflow_contract_parser_uses_wheel_owned_yaml_codec() -> None:
    workflow = yaml_loads("jobs:\n  validate:\n    steps:\n      - name: Run check\n        run: vx uv lock --check\n")

    assert workflow["jobs"]["validate"]["steps"][0]["run"] == "vx uv lock --check"


def test_checked_in_uv_lock_matches_release_version() -> None:
    checker = _load_checker_module()

    assert checker.check_uv_lock_consistency(REPO_ROOT) == []
