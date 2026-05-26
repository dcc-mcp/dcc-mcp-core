"""Regression for cp37 wheel parity — syntax must parse on Python 3.7."""

from __future__ import annotations

import ast
from pathlib import Path
import subprocess
import sys

import pytest

_REPO_ROOT = Path(__file__).resolve().parents[1]
_CHECKER = _REPO_ROOT / "scripts" / "check_py37_syntax.py"
_HOST_PUMP = _REPO_ROOT / "python" / "dcc_mcp_core" / "_server" / "host_pump.py"


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


def test_host_pump_runtime_aliases_avoid_pep604_unions() -> None:
    """Runtime aliases are evaluated during Python 3.7 wheel import smoke."""
    tree = ast.parse(_HOST_PUMP.read_text(encoding="utf-8"), filename=str(_HOST_PUMP))
    for node in tree.body:
        if not isinstance(node, ast.Assign):
            continue
        if not any(isinstance(target, ast.Name) and target.id == "HostTick" for target in node.targets):
            continue
        assert not any(
            isinstance(child, ast.BinOp) and isinstance(child.op, ast.BitOr) for child in ast.walk(node.value)
        )
        return
    pytest.fail("HostTick runtime alias not found")
