"""Release lock projection and consistency contract tests."""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path

import pytest

from conftest import REPO_ROOT
from dcc_mcp_core import yaml_loads

SCRIPT_PATH = REPO_ROOT / "scripts" / "ci" / "check_uv_lock.py"
LOCK_SYNC_WORKFLOW = REPO_ROOT / ".github" / "workflows" / "release-please-lock-sync.yml"
VERSION_WORKFLOW = REPO_ROOT / ".github" / "workflows" / "version-consistency.yml"
EXPECTED_ROOT_PACKAGE = "dcc-mcp-core"


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
    pull_request = trigger.get("pull_request")
    assert isinstance(pull_request, dict)
    paths = pull_request.get("paths")
    assert isinstance(paths, list)
    assert all(isinstance(path, str) for path in paths)
    return set(paths)


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
    assert sync_commands == ["cargo update -w", "cargo hakari generate", "vx uv lock"]
    commit_commands = _workflow_step_commands(
        sync_workflow,
        job="sync-cargo-metadata",
        step_name="Commit and push changes",
    )
    assert "git add Cargo.lock uv.lock crates/workspace-hack/Cargo.toml" in commit_commands

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
