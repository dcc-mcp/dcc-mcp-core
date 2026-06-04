"""E2E test for real Sentry ingest through dcc-mcp-server.

Exercises the Rust `init_sentry()` path compiled into `dcc-mcp-server`:
initialise from ``DCC_MCP_SENTRY_DSN``, capture a probe message, and flush
the transport to the configured Sentry project.

Requirements:
    ``DCC_MCP_SENTRY_DSN`` — Sentry project DSN (GitHub Actions secret in CI)

The test is skipped automatically when the DSN is absent so local ``pytest
tests/`` runs stay zero-config.

CI status: dedicated ``sentry-e2e`` job in ``.github/workflows/ci.yml`` sets
the secret and runs ``pytest tests/test_sentry_e2e.py``.
"""

from __future__ import annotations

import os
from pathlib import Path
import shlex
import subprocess
import sys

import pytest

REPO_ROOT = Path(__file__).resolve().parent.parent
SENTRY_DSN = os.environ.get("DCC_MCP_SENTRY_DSN", "").strip()


def _vx_cmd() -> tuple[str, ...]:
    cmd = os.environ.get("VX_CMD", "vx")
    return tuple(shlex.split(cmd, posix=sys.platform != "win32"))


@pytest.mark.skipif(not SENTRY_DSN, reason="DCC_MCP_SENTRY_DSN not set")
def test_sentry_real_ingest_via_rust_probe() -> None:
    """Run the Rust sentry_real_ingest_e2e probe (single-threaded)."""
    env = os.environ.copy()
    env.setdefault("DCC_MCP_SENTRY_ENVIRONMENT", "ci-e2e")
    env.setdefault("DCC_MCP_SENTRY_SAMPLE_RATE", "1.0")

    cmd = (
        *_vx_cmd(),
        "cargo",
        "test",
        "-p",
        "dcc-mcp-server",
        "--features",
        "sentry",
        "sentry_real_ingest_e2e",
        "--",
        "--exact",
        "--test-threads=1",
        "--nocapture",
    )
    result = subprocess.run(
        cmd,
        cwd=REPO_ROOT,
        env=env,
        check=False,
        capture_output=True,
        text=True,
        timeout=int(os.environ.get("SENTRY_E2E_TIMEOUT", "300")),
    )
    if result.returncode != 0:
        combined = f"{result.stdout}\n{result.stderr}".strip()
        pytest.fail(f"sentry_real_ingest_e2e failed (exit {result.returncode}):\n{combined}")
