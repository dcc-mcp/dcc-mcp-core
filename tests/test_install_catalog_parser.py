"""Catalog publication remains importable without optional YAML dependencies."""

from __future__ import annotations

import json
from pathlib import Path
import subprocess
import sys


def test_json_publication_uses_standard_library_without_site_packages(tmp_path):
    catalog = tmp_path / "catalog.json"
    output = tmp_path / "payload.json"
    entry = {"name": "dcc-mcp-maya", "description": "Withdrawn", "policy": {"installation": "not_available"}}
    catalog.write_text(json.dumps({"entries": [entry]}), encoding="utf-8")
    script = Path(__file__).resolve().parents[1] / "scripts/ci/prepare_install_catalog.py"
    result = subprocess.run(
        [
            sys.executable,
            "-S",
            str(script),
            "--catalog",
            str(catalog),
            "--output",
            str(output),
            "--source-revision",
            "a" * 40,
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 0, result.stderr
    assert json.loads(output.read_text(encoding="utf-8"))["entries"] == [entry]
