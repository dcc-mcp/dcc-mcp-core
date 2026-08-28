"""Package metadata contract tests."""

from __future__ import annotations

import re

from conftest import REPO_ROOT

PYPROJECT = REPO_ROOT / "pyproject.toml"
UV_LOCK = REPO_ROOT / "uv.lock"
SERVER_BINARY_DEP = "dcc-mcp-server>=0.18.17,<1.0.0"
TYPING_DEP = "typing-extensions==4.7.1; python_version<'3.8'"


def _project_dependencies() -> list[str]:
    text = PYPROJECT.read_text(encoding="utf-8")
    match = re.search(r"(?ms)^dependencies\s*=\s*\[(.*?)\]\s*(?=^\[)", text)
    assert match is not None, "pyproject.toml must declare [project] dependencies"
    return re.findall(r'"([^"]+)"', match.group(1))


def test_core_requires_packaged_server_binary_runtime() -> None:
    deps = _project_dependencies()
    assert any(dep.startswith(SERVER_BINARY_DEP) for dep in deps)


def test_python37_default_install_has_no_typing_backport() -> None:
    assert TYPING_DEP not in _project_dependencies()


def test_python37_lockfile_keeps_typing_backport_test_only() -> None:
    text = UV_LOCK.read_text(encoding="utf-8")
    start = text.index('[[package]]\nname = "dcc-mcp-core"\n')
    block = text[start + len("[[package]]") :].split("[[package]]", 1)[0]
    default_dependencies = block.split("[package.optional-dependencies]", 1)[0]
    assert 'name = "typing-extensions"' not in default_dependencies
    assert "name = \"typing-extensions\", marker = \"python_full_version < '3.8' and extra == 'test'\"" in block


def test_bridge_extra_declares_a_compatible_websockets_range() -> None:
    text = PYPROJECT.read_text(encoding="utf-8")
    section = text.split("[project.optional-dependencies]", 1)[1].split("[project.urls]", 1)[0]
    match = re.search(r"(?m)^bridge\s*=\s*\[(.*?)\]", section)
    assert match is not None
    assert '"websockets>=11,<18"' in match.group(1)
