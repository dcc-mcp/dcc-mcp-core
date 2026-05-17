"""Tests for ClawHub publish manifest and dry-run script."""

from __future__ import annotations

import json
from pathlib import Path
import subprocess
import sys

from conftest import REPO_ROOT

MANIFEST = REPO_ROOT / ".github" / "clawhub-skills.json"
SYNC_SCRIPT = REPO_ROOT / "scripts" / "clawhub_sync.py"


class TestClawhubSync:
    def test_manifest_lists_dcc_rest_gateway(self) -> None:
        entries = json.loads(MANIFEST.read_text(encoding="utf-8"))
        slugs = {e["slug"] for e in entries}
        assert "dcc-rest-gateway" in slugs

    def test_dry_run_exits_zero(self) -> None:
        proc = subprocess.run(
            [sys.executable, str(SYNC_SCRIPT), "--dry-run"],
            capture_output=True,
            text=True,
            timeout=60,
            cwd=str(REPO_ROOT),
            check=False,
        )
        assert proc.returncode == 0, proc.stderr
        assert "DRY-RUN" in proc.stdout
        assert "dcc-rest-gateway" in proc.stdout
