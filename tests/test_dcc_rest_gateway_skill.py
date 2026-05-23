"""Tests for skills/dcc-rest-gateway (ClawHub REST-only agent skill)."""

from __future__ import annotations

import json
from pathlib import Path
import subprocess
import sys
from unittest.mock import patch

from conftest import REPO_ROOT
import dcc_mcp_core

DCC_REST_GATEWAY_DIR = str(REPO_ROOT / "skills" / "dcc-rest-gateway")
CHECK_SCRIPT = Path(DCC_REST_GATEWAY_DIR) / "scripts" / "check_gateway.py"
RELEASE_MANIFEST = REPO_ROOT / ".release-please-manifest.json"

# Import probe from skill script (stdlib-only; safe on all platforms).
sys.path.insert(0, str(CHECK_SCRIPT.parent))
import check_gateway as check_gateway_mod  # noqa: E402

sys.path.pop(0)


class TestDccRestGatewaySkill:
    def test_skill_dir_exists(self) -> None:
        assert Path(DCC_REST_GATEWAY_DIR).is_dir()

    def test_parse_skill_md(self) -> None:
        meta = dcc_mcp_core.parse_skill_md(DCC_REST_GATEWAY_DIR)
        assert meta is not None
        assert meta.name == "dcc-rest-gateway"
        release_version = json.loads(RELEASE_MANIFEST.read_text(encoding="utf-8"))["."]
        assert meta.version == release_version

    def test_validate_skill_clean(self) -> None:
        report = dcc_mcp_core.validate_skill(DCC_REST_GATEWAY_DIR)
        assert report.is_clean, report.issues

    def test_scannable_from_skills_dir(self, skills_dir: str) -> None:
        scanner = dcc_mcp_core.SkillScanner()
        dirs = scanner.scan(extra_paths=[skills_dir])
        names = {Path(d).name for d in dirs}
        assert "dcc-rest-gateway" in names

    def test_description_mentions_clawhub_and_rest(self) -> None:
        meta = dcc_mcp_core.parse_skill_md(DCC_REST_GATEWAY_DIR)
        assert meta is not None
        desc = (meta.description or "").lower()
        assert "clawhub" in desc
        assert "rest" in desc
        assert "mcp" in desc

    def test_reference_docs_present(self) -> None:
        root = Path(DCC_REST_GATEWAY_DIR)
        assert (root / "references" / "ZERO_INSTANCES.md").is_file()
        assert (root / "references" / "REST_CHEATSHEET.md").is_file()

    def test_probe_offline_gateway(self) -> None:
        payload = check_gateway_mod.probe("http://127.0.0.1:1")
        assert payload["gateway_url"] == "http://127.0.0.1:1"
        assert payload["gateway_ok"] is False
        assert payload["total"] == 0

    def test_probe_parses_instances(self) -> None:
        body = json.dumps(
            {
                "total": 2,
                "instances": [
                    {"dcc_type": "maya"},
                    {"dcc_type": "maya"},
                    {"dcc_type": "blender"},
                ],
            }
        ).encode()

        def fake_urlopen(url, timeout=0):
            url_s = str(url)

            class Resp:
                status = 200

                def __enter__(self):
                    return self

                def __exit__(self, *args):
                    return False

                def read(self):
                    if url_s.endswith("/v1/instances"):
                        return body
                    return b"{}"

            return Resp()

        with patch.object(check_gateway_mod.urllib.request, "urlopen", fake_urlopen):
            payload = check_gateway_mod.probe("http://127.0.0.1:9765")

        assert payload["gateway_ok"] is True
        assert payload["ready"] is True
        assert payload["total"] == 2
        assert payload["by_dcc_type"] == {"maya": 2, "blender": 1}

    def test_check_gateway_cli_outputs_json(self) -> None:
        assert CHECK_SCRIPT.is_file()
        result = subprocess.run(
            [sys.executable, str(CHECK_SCRIPT), "--gateway", "http://127.0.0.1:1"],
            capture_output=True,
            text=True,
            timeout=30,
            check=False,
        )
        assert result.returncode == 0, result.stderr
        payload = json.loads(result.stdout.strip())
        assert payload["gateway_url"] == "http://127.0.0.1:1"
        assert payload["gateway_ok"] is False
