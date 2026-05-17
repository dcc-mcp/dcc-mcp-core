"""Regression for cp37 wheel parity — syntax must parse on Python 3.7."""

from __future__ import annotations

from pathlib import Path
import subprocess
import sys

import pytest

_REPO_ROOT = Path(__file__).resolve().parents[1]
_CHECKER = _REPO_ROOT / "scripts" / "check_py37_syntax.py"


@pytest.mark.skipif(sys.version_info[:2] != (3, 7), reason="requires Python 3.7 interpreter")
def test_check_py37_syntax_passes_on_repo_tree() -> None:
    result = subprocess.run(
        [sys.executable, str(_CHECKER)],
        cwd=str(_REPO_ROOT),
        capture_output=True,
        text=True,
    )
    assert result.returncode == 0, result.stderr or result.stdout


@pytest.mark.skipif(sys.version_info[:2] != (3, 7), reason="requires Python 3.7 interpreter")
def test_check_py37_syntax_rejects_pep604_union(tmp_path: Path) -> None:
    bad = tmp_path / "bad.py"
    bad.write_text("def f(x: int | None) -> None:\n    pass\n", encoding="utf-8")
    source = bad.read_text(encoding="utf-8")
    with pytest.raises(SyntaxError):
        compile(source, str(bad), "exec")
