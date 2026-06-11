"""Package metadata contract tests."""

from __future__ import annotations

import re

from conftest import REPO_ROOT

PYPROJECT = REPO_ROOT / "pyproject.toml"
SERVER_BINARY_DEP = "dcc-mcp-server>=0.18.17,<1.0.0"


def _project_dependencies() -> list[str]:
    text = PYPROJECT.read_text(encoding="utf-8")
    match = re.search(r"(?ms)^dependencies\s*=\s*\[(.*?)\]\s*(?=^\[)", text)
    assert match is not None, "pyproject.toml must declare [project] dependencies"
    return re.findall(r'"([^"]+)"', match.group(1))


def test_core_requires_packaged_server_binary_runtime() -> None:
    assert SERVER_BINARY_DEP in _project_dependencies()
