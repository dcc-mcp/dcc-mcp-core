"""Run a script with a Python 3.7 interpreter (for local lint / CI helpers)."""

from __future__ import annotations

from collections.abc import Sequence
import shutil
import subprocess
import sys


def _probe(cmd: Sequence[str]) -> list[str] | None:
    if not cmd or not shutil.which(cmd[0]):
        return None
    try:
        result = subprocess.run(
            [*cmd, "-c", "import sys; raise SystemExit(0 if sys.version_info[:2] == (3, 7) else 1)"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    except OSError:
        return None
    if result.returncode == 0:
        return list(cmd)
    return None


def find_python37() -> list[str] | None:
    """Return argv prefix for a Python 3.7 executable, or None."""
    if sys.version_info[:2] == (3, 7):
        return [sys.executable]
    for candidate in (["py", "-3.7"], ["python3.7"]):
        found = _probe(candidate)
        if found is not None:
            return found
    return None


def main(argv: Sequence[str] | None = None) -> int:
    """Locate Python 3.7 and exec *argv[0]* with remaining args."""
    args = list(argv if argv is not None else sys.argv[1:])
    if not args:
        sys.stderr.write("usage: run_with_py37.py <script.py> [args...]\n")
        return 2

    py37 = find_python37()
    if py37 is None:
        sys.stderr.write("run_with_py37: no Python 3.7 interpreter found (install 3.7 or use CI lint job)\n")
        return 1

    return subprocess.call([*py37, *args])


if __name__ == "__main__":
    raise SystemExit(main())
