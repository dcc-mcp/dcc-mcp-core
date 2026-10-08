"""Guard: ``tests/test_imports.py`` must not require third-party imports.

The ``build-wheel`` action runs ``python tests/test_imports.py`` in a clean venv
that holds only the built wheel and its runtime dependencies (see
``.github/actions/build-wheel/action.yml``). An unconditional third-party import
in that file makes the release wheel verification fail deterministically -- which
is exactly how 0.20.42 lost its ``dcc-mcp-core`` PyPI upload. This guard blocks
the regression at PR time instead of release time.

An import wrapped in ``try``/``except ImportError`` is allowed: it degrades
gracefully when the package is absent, so it cannot break the release venv.
The exemption is scoped to that specific import statement's location, not to
the module name, so a guarded ``import pytest`` never excuses an unconditional
``import pytest`` elsewhere in the same file.

Detection boundary: this is a static AST check. It reports the module root and
line of every third-party import that is not inside an ``ImportError`` guard,
including imports nested in ``if`` branches, function bodies, and ``try``
blocks whose handlers do not catch ``ImportError``. It does not follow imports
resolved at runtime through a variable name, nor ``__import__`` calls with a
non-literal argument. Those stay covered by the real ``Test wheel`` gate, which
executes the script in a wheel-only venv; this guard is defence in depth, not a
replacement for it.
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


def _collect_guarded_node_ids(tree: ast.AST) -> set[int]:
    """Return the ``id()`` of import nodes sitting inside a guarded ``try``.

    The exemption is keyed to the import's AST location, not its module name.
    A name-keyed exemption leaks: one guarded ``import pytest`` would excuse an
    unconditional ``import pytest`` elsewhere in the same file, and that second
    import *does* break the release venv.
    """
    guarded: set[int] = set()
    for node in ast.walk(tree):
        if not isinstance(node, ast.Try):
            continue
        handles_import_error = any(
            isinstance(handler.type, ast.Name) and handler.type.id == "ImportError" for handler in node.handlers
        )
        if not handles_import_error:
            continue
        for inner in ast.walk(node):
            if isinstance(inner, (ast.Import, ast.ImportFrom)):
                guarded.add(id(inner))
    return guarded


def _unguarded_imports(tree: ast.AST) -> list[str]:
    """Return ``"root (line N)"`` for imports not inside an ``ImportError`` guard.

    Walks the tree itself rather than calling ``ast.walk`` on a detached set of
    names, so each import is judged at its own location.
    """
    guarded = _collect_guarded_node_ids(tree)
    offenders: list[str] = []
    for node in ast.walk(tree):
        if id(node) in guarded:
            continue
        if isinstance(node, ast.Import):
            for alias in node.names:
                offenders.append(f"{alias.name.split('.')[0]} (line {node.lineno})")
        elif isinstance(node, ast.ImportFrom) and node.level == 0 and node.module:
            offenders.append(f"{node.module.split('.')[0]} (line {node.lineno})")
    return offenders


def test_imports_script_only_imports_stdlib() -> None:
    """The release smoke script must run without third-party packages installed."""
    tree = ast.parse(IMPORTS_PATH.read_text(encoding="utf-8"), filename=str(IMPORTS_PATH))
    allowed = _allowed_module_names()
    third_party = sorted(entry for entry in _unguarded_imports(tree) if entry.split(" ")[0] not in allowed)
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


def _third_party_in(source: str) -> list[str]:
    """Run the static check over arbitrary source and return offenders."""
    tree = ast.parse(source)
    allowed = _allowed_module_names()
    return [entry for entry in _unguarded_imports(tree) if entry.split(" ")[0] not in allowed]


# ``responses`` is a dev-only test dependency: absent from the wheel venv, so it
# is a faithful stand-in for a real third-party import in these fixtures.
_CAUGHT_FIXTURES = {
    "unconditional": "import responses\n",
    "inside_if_branch": "if True:\n    import responses\n",
    "inside_function": "def _f():\n    import responses\n    return responses\n",
    "from_import": "from responses import mark\n",
    "try_bare_except": "try:\n    import responses\nexcept Exception:\n    responses = None\n",
    "leaked_by_guarded_twin": (
        "try:\n    import responses\nexcept ImportError:\n    responses = None\n\nimport responses\n"
    ),
}

_ALLOWED_FIXTURES = {
    "guarded_import": "try:\n    import responses\nexcept ImportError:\n    responses = None\n",
    "guarded_from_import": "try:\n    from responses import mark\nexcept ImportError:\n    mark = None\n",
    "guarded_with_other_body": ("try:\n    import responses\n    _x = responses\nexcept ImportError:\n    _x = None\n"),
    "stdlib_import": "import json\nimport os.path\n",
}


def test_guard_reports_unguarded_third_party_imports() -> None:
    """Imports outside an ``ImportError`` guard must be reported, wherever they sit.

    These fixtures are the reverse-mutation set for the guard: each one is a
    shape a future edit could reintroduce into ``test_imports.py``, including the
    name-leak case where a guarded import of a module used to excuse an
    unconditional import of the same module.
    """
    missed = {name: _third_party_in(src) for name, src in _CAUGHT_FIXTURES.items()}
    missed = {name: offenders for name, offenders in missed.items() if not offenders}
    assert missed == {}, (
        f"The static guard missed unguarded third-party imports in: {', '.join(sorted(missed))}. "
        f"An import must only be exempt at its own AST location, never by module name."
    )


def test_guard_allows_imports_guarded_against_import_error() -> None:
    """A real ``try``/``except ImportError`` guard must stay exempt.

    Tightening the check must not produce false positives, or the guard would
    block the very pattern it is designed to tolerate.
    """
    false_positives = {name: _third_party_in(src) for name, src in _ALLOWED_FIXTURES.items()}
    false_positives = {name: offenders for name, offenders in false_positives.items() if offenders}
    assert false_positives == {}, (
        f"The static guard wrongly flagged ImportError-guarded or stdlib imports: {', '.join(sorted(false_positives))}."
    )
