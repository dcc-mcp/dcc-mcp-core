"""Verify pure-Python modules are syntax-compatible with Python 3.7 (Maya 2022).

Must be invoked with a Python 3.7 interpreter (``python3.7 scripts/check_py37_syntax.py``).
PEP 604 unions (``str | None``) and builtin generics (``dict[str, Any]``) are syntax
errors on 3.7 even with ``from __future__ import annotations``.

This is enforced in CI via ``check-py37-syntax`` job. The Rust extension
(``_core``) requires Python 3.8+, but pure-Python modules must remain
importable on Maya 2022's embedded Python 3.7.
"""

from __future__ import annotations

from pathlib import Path
import sys
from typing import Iterator
from typing import List
from typing import Tuple

if sys.version_info[:2] != (3, 7):
    sys.stderr.write("check_py37_syntax: requires Python 3.7, got {}.{}.{}\n".format(*sys.version_info[:3]))
    raise SystemExit(2)

_REPO_ROOT = Path(__file__).resolve().parents[1]
_SCAN_ROOTS = (
    _REPO_ROOT / "python" / "dcc_mcp_core",
    _REPO_ROOT / "tests",
    _REPO_ROOT / "examples",
    _REPO_ROOT / "scripts",
)
_SKIP_NAMES = frozenset({"_core.pyi"})
# Launchers that only run on the developer/CI Python (3.8+), not inside cp37 wheels.
_SKIP_RELATIVE = frozenset({"scripts/run_with_py37.py"})


def _iter_py_files() -> Iterator[Path]:
    for root in _SCAN_ROOTS:
        if not root.is_dir():
            continue
        for path in sorted(root.rglob("*.py")):
            if path.name in _SKIP_NAMES:
                continue
            rel = path.relative_to(_REPO_ROOT).as_posix()
            if rel in _SKIP_RELATIVE:
                continue
            yield path


def main() -> int:
    """Scan the repo tree and exit non-zero on any SyntaxError under 3.7."""
    failures: List[Tuple[Path, SyntaxError]] = []  # noqa: UP006
    count = 0
    for path in _iter_py_files():
        count += 1
        source = path.read_text(encoding="utf-8")
        try:
            compile(source, str(path), "exec")
        except SyntaxError as exc:
            failures.append((path, exc))

    if failures:
        for path, exc in failures:
            location = exc.lineno or 0
            sys.stderr.write("{}:{}: {}\n".format(path, location, exc.msg))
        sys.stderr.write("check_py37_syntax: {} file(s) failed to parse on Python 3.7\n".format(len(failures)))
        return 1

    sys.stdout.write("check_py37_syntax: OK ({} files)\n".format(count))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
