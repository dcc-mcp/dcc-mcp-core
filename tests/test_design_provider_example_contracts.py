"""Keep the host-free Framer example contracts in the existing test gate."""

from __future__ import annotations

from pathlib import Path
import shutil
import subprocess
import sys

import pytest

_EXAMPLE = Path(__file__).resolve().parents[1] / "examples" / "design-provider-bridge"
_CONTRACTS = _EXAMPLE / "tests" / "framer"


def _run_contract(command):
    result = subprocess.run(command, cwd=str(_EXAMPLE), capture_output=True, text=True, timeout=45, check=False)
    assert result.returncode == 0, result.stdout + result.stderr


def test_framer_python_adapter_contracts():
    if not shutil.which("node"):
        pytest.skip("The existing Node runtime is required; tests never install it.")
    _run_contract([sys.executable, str(_CONTRACTS / "test_framer_adapter.py")])


def test_framer_node_session_contracts():
    node = shutil.which("node")
    if not node:
        pytest.skip("The existing Node runtime is required; tests never install it.")
    _run_contract([node, "--test", str(_CONTRACTS / "framer_session.test.mjs")])
