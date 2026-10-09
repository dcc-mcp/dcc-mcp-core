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

# NOTE: this file doubles as the release wheel smoke script. The ``build-wheel``
# action runs it with plain ``python`` inside a venv holding only the wheel and
# its runtime dependencies, so it must never require third-party packages at
# import time. ``tests/test_imports_stdlib_only.py`` enforces this statically.

#: Modules that must survive being the *first* ``dcc_mcp_core`` submodule a
#: fresh interpreter imports. ``collect_import_failures`` cannot catch a cycle
#: here because it imports ``dcc_mcp_core`` first, which fully initialises the
#: package before any submodule is touched; a cycle only fires when one of
#: these is the entry point, so each one needs its own interpreter.
FRESH_IMPORT_ENTRYPOINTS = (
    "dcc_mcp_core",
    # Console-script / `python -m` target: pip and Rez both launch it as the
    # first dcc_mcp_core import in a fresh interpreter.
    "dcc_mcp_core.__main__",
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

#: Symbols that a package resolves through a lazy re-export table
#: (``_LAZY_EXPORTS``) instead of at import time. Importing the package proves
#: nothing about that table: the name is only resolved when a consumer touches
#: the attribute. Asserting reachability here puts the guard in the same place
#: as the mechanism, so corrupting or emptying ``_LAZY_EXPORTS`` fails this
#: test instead of passing silently.
LAZY_REACHABILITY_PROBES: tuple[tuple[str, str], ...] = (("dcc_mcp_core._server", "SkillDiscoveryController"),)

#: Statement resolving each probe inside a fresh interpreter. Built as a
#: sequence so the import stays the first ``dcc_mcp_core`` import the
#: interpreter performs, which is what the cycle regression needs.
_LAZY_PROBE_TEMPLATE = """
import {module_name}
from {module_name} import {symbol}
assert {symbol} is not None
print({symbol})
"""


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
    _lazy_probe_parametrize = pytest.mark.parametrize(("module_name", "symbol"), LAZY_REACHABILITY_PROBES)
except ImportError:  # pragma: no cover - exercised only outside pytest
    pytest = None

    def _parametrize(func):
        return func

    def _lazy_probe_parametrize(func):
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


@_lazy_probe_parametrize
def test_lazy_reexport_resolves_as_first_import(module_name: str, symbol: str) -> None:
    """A lazy re-export must resolve to a real object in a fresh interpreter.

    Guards the ``_LAZY_EXPORTS`` table itself. A bare ``import`` only runs the
    module body; the deferred name is resolved on first attribute access, so
    this test both imports and touches the symbol in one interpreter.
    """
    code = _LAZY_PROBE_TEMPLATE.format(module_name=module_name, symbol=symbol)
    result = subprocess.run(
        [sys.executable, "-c", code],
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 0, (
        f"`from {module_name} import {symbol}` failed in a fresh interpreter "
        f"— the lazy re-export table is broken:\n{result.stderr}"
    )
    assert symbol in result.stdout


def test_server_dir_lists_lazy_export() -> None:
    """``dir()`` must advertise the deferred name before it is resolved.

    Covers ``dcc_mcp_core._server.__dir__``, which unions the module globals
    with the lazy-export names. Importing the submodule alone does not touch
    the name, so this assertion only holds when ``__dir__`` consults the table.
    """
    server_module = importlib.import_module("dcc_mcp_core._server")
    names = dir(server_module)

    assert "SkillDiscoveryController" in names
    # Real, eagerly imported collaborators must still show up alongside it.
    assert "DccServerOptions" in names
    # __dir__ returns a sorted, de-duplicated union.
    assert names == sorted(set(names))
    assert len(names) == len(set(names))


if __name__ == "__main__":
    assert_import_smoke()
