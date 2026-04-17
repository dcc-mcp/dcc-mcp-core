"""Tests for the bundled ``dcc-diagnostics/scripts/screenshot.py`` script.

Covers:
- When ``DCC_MCP_IPC_ADDRESS`` is unset, the script falls back to the local
  ``Capturer`` path and prints success JSON with ``source`` field absent.
- ``--full-screen`` flag is accepted.
- Output JSON parses and contains ``image_base64``.
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
import base64
import json
from pathlib import Path
import subprocess
import sys

SCRIPT_PATH = (
    Path(__file__).resolve().parent.parent
    / "python"
    / "dcc_mcp_core"
    / "skills"
    / "dcc-diagnostics"
    / "scripts"
    / "screenshot.py"
)


# ── Helpers ──────────────────────────────────────────────────────────────────


def _run_script(*extra_args: str, env: dict[str, str] | None = None) -> dict:
    """Execute the screenshot script and return the parsed JSON payload."""
    cmd = [sys.executable, str(SCRIPT_PATH), *extra_args]
    proc = subprocess.run(
        cmd,
        capture_output=True,
        text=True,
        timeout=30,
        env=env,
        encoding="utf-8",
    )
    assert proc.returncode == 0, f"script failed: {proc.stderr}"
    # Strip any debug lines that may have been printed; the final line holds the JSON payload.
    lines = [line for line in proc.stdout.strip().splitlines() if line.strip().startswith("{")]
    assert lines, f"no JSON output:\nstdout={proc.stdout!r}\nstderr={proc.stderr!r}"
    return json.loads(lines[-1])


# ── Fallback path (no DCC_MCP_IPC_ADDRESS) ───────────────────────────────────


class TestScreenshotFallbackPath:
    def test_script_exists(self) -> None:
        assert SCRIPT_PATH.is_file(), f"missing: {SCRIPT_PATH}"

    def test_fallback_returns_success(self, monkeypatch) -> None:
        env = {k: v for k, v in __import__("os").environ.items()}
        env.pop("DCC_MCP_IPC_ADDRESS", None)
        env.pop("DCC_MCP_OWNER_IPC", None)
        payload = _run_script("--format", "png", env=env)
        assert payload["success"] is True

    def test_fallback_returns_image_base64(self) -> None:
        env = {k: v for k, v in __import__("os").environ.items()}
        env.pop("DCC_MCP_IPC_ADDRESS", None)
        env.pop("DCC_MCP_OWNER_IPC", None)
        payload = _run_script("--format", "png", env=env)
        assert "context" in payload
        assert isinstance(payload["context"].get("image_base64"), str)
        decoded = base64.b64decode(payload["context"]["image_base64"])
        assert len(decoded) > 0

    def test_full_screen_flag_accepted(self) -> None:
        env = {k: v for k, v in __import__("os").environ.items()}
        env.pop("DCC_MCP_IPC_ADDRESS", None)
        env.pop("DCC_MCP_OWNER_IPC", None)
        payload = _run_script("--full-screen", env=env)
        assert payload["success"] is True

    def test_unreachable_ipc_falls_back(self) -> None:
        """A bogus DCC_MCP_IPC_ADDRESS must not break the script."""
        env = {k: v for k, v in __import__("os").environ.items()}
        # Use a pipe/socket path that cannot exist.
        env["DCC_MCP_IPC_ADDRESS"] = "pipe:///\\.\\pipe\\nonexistent_dcc_mcp_pipe_xyz"
        payload = _run_script("--format", "png", env=env)
        # Fallback to the direct Capturer path should succeed.
        assert payload["success"] is True
        # The "source" field is only set on the IPC success path, so its
        # absence here confirms fallback took place.
        assert payload.get("context", {}).get("source") != "dcc-ipc"
