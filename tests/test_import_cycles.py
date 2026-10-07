"""Fresh-interpreter import cycle regression tests.

``tests/test_imports.py`` is a release smoke script that must stay free of
third-party dependencies: the ``build-wheel`` action runs it with plain
``python`` inside a venv that only holds the wheel and its runtime
dependencies. Pytest-only tests therefore live here instead.
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
import subprocess
import sys

# Import third-party modules
import pytest

#: Modules that must survive being the *first* ``dcc_mcp_core`` submodule a
#: fresh interpreter imports. ``collect_import_failures`` cannot catch a cycle
#: here because it imports ``dcc_mcp_core`` first, which fully initialises the
#: package before any submodule is touched; a cycle only fires when one of
#: these is the entry point, so each one needs its own interpreter.
FRESH_IMPORT_ENTRYPOINTS = (
    "dcc_mcp_core",
    "dcc_mcp_core.dcc_server",
    "dcc_mcp_core.server_base",
    "dcc_mcp_core._server",
    "dcc_mcp_core._server.skill_discovery",
    "dcc_mcp_core._server.diagnostic_state",
    "dcc_mcp_core.skills",
    "dcc_mcp_core.skills.builtin",
    "dcc_mcp_core.factory",
    "dcc_mcp_core.skills_helper",
)


@pytest.mark.parametrize("module_name", FRESH_IMPORT_ENTRYPOINTS)
def test_module_imports_cleanly_as_first_import(module_name: str) -> None:
    """A fresh interpreter must import each entry point without a cycle."""
    result = subprocess.run(
        [sys.executable, "-c", f"import {module_name}"],
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 0, f"`import {module_name}` failed in a fresh interpreter:\n{result.stderr}"
