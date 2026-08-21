"""Guard the single ownership boundary for Python runtime env names."""

from __future__ import annotations

import ast
from pathlib import Path

from dcc_mcp_core import constants

REPO_ROOT = Path(__file__).resolve().parent.parent
PACKAGE_ROOT = REPO_ROOT / "python" / "dcc_mcp_core"


def test_runtime_env_names_are_owned_by_constants_module() -> None:
    violations = []
    for path in PACKAGE_ROOT.rglob("*.py"):
        relative = path.relative_to(PACKAGE_ROOT)
        if relative.name == "constants.py" or relative.parts[0] == "skills":
            continue
        tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(relative))
        for node in ast.walk(tree):
            value = getattr(node, "value", None)
            if isinstance(node, ast.Constant) and isinstance(value, str) and value.startswith("DCC_MCP_"):
                violations.append(f"{relative}:{node.lineno}:{value}")

    assert not violations, "Runtime env names must come from dcc_mcp_core.constants:\n{}".format("\n".join(violations))


def test_all_env_constants_are_exported_and_namespaced() -> None:
    env_constants = {
        name: value for name, value in vars(constants).items() if name.startswith("ENV_") and isinstance(value, str)
    }

    assert env_constants
    assert all(value.startswith("DCC_MCP_") for value in env_constants.values())
    assert set(env_constants).issubset(constants.__all__)
