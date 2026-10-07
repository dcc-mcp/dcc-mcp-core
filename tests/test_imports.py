"""Import smoke tests for wheel and embedded Python compatibility."""

# Import future modules
from __future__ import annotations

# Import built-in modules
import importlib
import pkgutil
import subprocess
import sys

# Import local modules
import dcc_mcp_core
from dcc_mcp_core._core import SkillScanner
from dcc_mcp_core._core import ToolRegistry
from dcc_mcp_core._core import ToolResult

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


def collect_import_failures() -> list[tuple[str, str]]:
    """Import every package module and return failures as ``(name, error)``."""
    failures: list[tuple[str, str]] = []
    for module_info in pkgutil.walk_packages(dcc_mcp_core.__path__, prefix=f"{dcc_mcp_core.__name__}."):
        try:
            importlib.import_module(module_info.name)
        except Exception as exc:
            failures.append((module_info.name, f"{type(exc).__name__}: {exc}"))
    return failures


def assert_import_smoke() -> None:
    """Run the same import smoke test used by wheel CI and pytest."""
    print(f"Version: {dcc_mcp_core.__version__}")
    result = ToolResult(success=True, message="Wheel test passed")
    print(f"Result: {result}")
    reg = ToolRegistry()
    print(f"Registry: {reg}")
    print(f"Scanner: {SkillScanner}")

    failures = collect_import_failures()
    if failures:
        for name, error in failures:
            print(f"Import failed: {name}: {error}")
    assert failures == []
    print("All imports OK!")


def test_dcc_mcp_core_import_smoke() -> None:
    """Every importable package module should load without side effects."""
    assert_import_smoke()


try:
    # Import third-party modules
    import pytest

    _parametrize = pytest.mark.parametrize("module_name", FRESH_IMPORT_ENTRYPOINTS)
except ImportError:  # pragma: no cover - exercised only outside pytest
    pytest = None

    def _parametrize(func):
        return func


@_parametrize
def test_module_imports_cleanly_as_first_import(module_name: str) -> None:
    """A fresh interpreter must import each entry point without a cycle."""
    result = subprocess.run(
        [sys.executable, "-c", f"import {module_name}"],
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 0, f"`import {module_name}` failed in a fresh interpreter:\n{result.stderr}"


if __name__ == "__main__":
    assert_import_smoke()
