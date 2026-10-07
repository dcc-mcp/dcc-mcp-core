"""Guard: ``tests/test_imports.py`` must not require third-party imports.

The ``build-wheel`` action runs ``python tests/test_imports.py`` in a clean venv
that holds only the built wheel and its runtime dependencies (see
``.github/actions/build-wheel/action.yml``). An unconditional third-party import
in that file makes the release wheel verification fail deterministically -- which
is exactly how 0.20.42 lost its ``dcc-mcp-core`` PyPI upload. This guard blocks
the regression at PR time instead of release time.

An import already wrapped in ``try``/``except ImportError`` is allowed: it
degrades gracefully when the package is absent, so it cannot break the release
venv. Only unconditional imports are the regression this guard catches.
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
import ast
from pathlib import Path
import subprocess
import sys

IMPORTS_PATH = Path(__file__).with_name("test_imports.py")


def _allowed_module_names() -> set[str]:
    """Return module names the smoke script may import.

    Standard library plus the package under test and the test helper package:
    the wheel venv contains both, so neither counts as a third-party import.
    """
    names: set[str] = set(getattr(sys, "stdlib_module_names", ()))
    # `sys.stdlib_module_names` is missing before Python 3.10; fall back to a
    # conservative allowlist covering the modules the file actually needs.
    if not names:
        names = {
            "__future__",
            "importlib",
            "json",
            "os",
            "pathlib",
            "pkgutil",
            "subprocess",
            "sys",
            "typing",
        }
    names |= {"dcc_mcp_core", "tests"}
    return names


def _collect_optional_names(tree: ast.AST) -> set[str]:
    """Return roots imported inside a ``try``/``except ImportError`` guard.

    An import already tolerant of ``ImportError`` cannot break the release
    verification venv, so it is not the regression this guard exists to catch.
    Only unconditional imports are.
    """
    optional: set[str] = set()
    for node in ast.walk(tree):
        if not isinstance(node, ast.Try):
            continue
        handles_import_error = any(
            isinstance(handler.type, ast.Name) and handler.type.id == "ImportError" for handler in node.handlers
        )
        if not handles_import_error:
            continue
        for inner in ast.walk(node):
            if isinstance(inner, ast.Import):
                for alias in inner.names:
                    optional.add(alias.name.split(".")[0])
            elif isinstance(inner, ast.ImportFrom) and inner.level == 0 and inner.module:
                optional.add(inner.module.split(".")[0])
    return optional


def _imported_roots() -> set[str]:
    """Return the top-level package names imported by ``tests/test_imports.py``."""
    tree = ast.parse(IMPORTS_PATH.read_text(encoding="utf-8"), filename=str(IMPORTS_PATH))
    roots: set[str] = set()
    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            for alias in node.names:
                roots.add(alias.name.split(".")[0])
        elif isinstance(node, ast.ImportFrom) and node.level == 0 and node.module:
            roots.add(node.module.split(".")[0])
    return roots


def test_imports_script_only_imports_stdlib() -> None:
    """The release smoke script must run without third-party packages installed."""
    tree = ast.parse(IMPORTS_PATH.read_text(encoding="utf-8"), filename=str(IMPORTS_PATH))
    allowed = _allowed_module_names() | _collect_optional_names(tree)
    third_party = sorted(root for root in _imported_roots() if root not in allowed)
    assert third_party == [], (
        f"{IMPORTS_PATH.name} is executed by the release wheel verification step in a "
        f"venv that contains only the wheel and its runtime dependencies. "
        f"Unconditional third-party imports ({', '.join(third_party)}) would break the "
        f"release. Guard them with try/except ImportError, or move the tests to "
        f"tests/test_import_cycles.py."
    )


def test_imports_script_avoids_dynamic_third_party_imports() -> None:
    """The smoke script must not resolve third-party modules dynamically.

    ``importlib.import_module`` with a literal string cannot be checked by the
    import-name guard above, so a dynamic third-party import would slip past
    it. The script legitimately walks its own package with ``import_module``,
    so only literal names outside the package under test are rejected.
    """
    tree = ast.parse(IMPORTS_PATH.read_text(encoding="utf-8"), filename=str(IMPORTS_PATH))
    allowed = _allowed_module_names()
    offenders: list[str] = []
    for node in ast.walk(tree):
        is_dynamic_call = (
            isinstance(node, ast.Call)
            and isinstance(node.func, ast.Attribute)
            and node.func.attr == "import_module"
            and bool(node.args)
            and isinstance(node.args[0], ast.Constant)
            and isinstance(node.args[0].value, str)
        )
        if not is_dynamic_call:
            continue
        root = node.args[0].value.split(".")[0]
        if root not in allowed:
            offenders.append(f"{root!r} (line {node.lineno})")
    assert offenders == [], (
        f"{IMPORTS_PATH.name} dynamically imports third-party module(s) {', '.join(offenders)}. "
        f"Dynamic imports bypass the static import guard and would break the release "
        f"wheel verification venv."
    )


def test_imports_script_runs_as_plain_script() -> None:
    """`python tests/test_imports.py` must exit 0 without pytest installed."""
    result = subprocess.run(
        [sys.executable, str(IMPORTS_PATH)],
        capture_output=True,
        text=True,
        check=False,
        cwd=str(IMPORTS_PATH.parent.parent),
    )
    assert result.returncode == 0, f"`python {IMPORTS_PATH}` failed:\n{result.stderr}"
