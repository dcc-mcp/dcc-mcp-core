"""Documentation workflow resilience tests."""

from __future__ import annotations

from conftest import REPO_ROOT
from dcc_mcp_core import yaml_loads

DOCS_CI_WORKFLOW = REPO_ROOT / ".github" / "workflows" / "docs-ci.yml"
DEPLOY_DOCS_WORKFLOW = REPO_ROOT / ".github" / "workflows" / "deploy-docs.yml"
NPM_CI_ACTION = REPO_ROOT / ".github" / "actions" / "npm-ci-with-retry" / "action.yml"
NPM_CI_ACTION_REF = "./.github/actions/npm-ci-with-retry"


def _load_yaml(path):
    return yaml_loads(path.read_text(encoding="utf-8"))


def _install_step(workflow_path):
    workflow = _load_yaml(workflow_path)
    return next(step for step in workflow["jobs"]["build"]["steps"] if step["name"] == "Install dependencies")


def test_docs_workflows_share_bounded_npm_ci_retry_action() -> None:
    expected_step = {
        "name": "Install dependencies",
        "uses": NPM_CI_ACTION_REF,
        "with": {"working-directory": "docs"},
    }

    assert _install_step(DOCS_CI_WORKFLOW) == expected_step
    assert _install_step(DEPLOY_DOCS_WORKFLOW) == expected_step


def test_npm_ci_retry_action_is_bounded_and_fail_closed() -> None:
    action = _load_yaml(NPM_CI_ACTION)
    step = action["runs"]["steps"][0]
    script = step["run"]

    assert action["runs"]["using"] == "composite"
    assert step["working-directory"] == "${{ inputs.working-directory }}"
    assert step["env"] == {
        "NPM_CONFIG_FETCH_RETRIES": "3",
        "NPM_CONFIG_FETCH_RETRY_MINTIMEOUT": "20000",
        "NPM_CONFIG_FETCH_RETRY_MAXTIMEOUT": "120000",
    }
    assert "for attempt in 1 2 3" in script
    assert "npm ci --prefer-offline --no-audit" in script
    assert 'if [[ "$attempt" -eq 3 ]]' in script
    assert "exit 1" in script


def test_docs_ci_runs_when_retry_or_docs_workflow_changes() -> None:
    workflow = _load_yaml(DOCS_CI_WORKFLOW)
    expected_paths = {
        ".github/actions/npm-ci-with-retry/**",
        ".github/workflows/deploy-docs.yml",
        ".github/workflows/docs-ci.yml",
    }

    assert expected_paths <= set(workflow["on"]["pull_request"]["paths"])
    assert expected_paths <= set(workflow["on"]["push"]["paths"])
