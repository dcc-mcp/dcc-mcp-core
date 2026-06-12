"""Tests for skills/dcc-cli-gateway (ClawHub CLI-first agent skill)."""

from __future__ import annotations

import json
from pathlib import Path
import subprocess
import sys
from unittest.mock import patch

from conftest import REPO_ROOT
import dcc_mcp_core

DCC_CLI_GATEWAY_DIR = str(REPO_ROOT / "skills" / "dcc-cli-gateway")
CHECK_SCRIPT = Path(DCC_CLI_GATEWAY_DIR) / "scripts" / "check_cli.py"
RELEASE_MANIFEST = REPO_ROOT / ".release-please-manifest.json"

sys.path.insert(0, str(CHECK_SCRIPT.parent))
import check_cli as check_cli_mod  # noqa: E402
import dcc_gateway as dcc_gateway_mod  # noqa: E402

sys.path.pop(0)


class TestDccCliGatewaySkill:
    def test_skill_dir_exists(self) -> None:
        assert Path(DCC_CLI_GATEWAY_DIR).is_dir()

    def test_parse_skill_md(self) -> None:
        meta = dcc_mcp_core.parse_skill_md(DCC_CLI_GATEWAY_DIR)
        assert meta is not None
        assert meta.name == "dcc-cli-gateway"
        release_version = json.loads(RELEASE_MANIFEST.read_text(encoding="utf-8"))["."]
        assert meta.version == release_version

    def test_validate_skill_clean(self) -> None:
        report = dcc_mcp_core.validate_skill(DCC_CLI_GATEWAY_DIR)
        assert report.is_clean, report.issues

    def test_scannable_from_skills_dir(self, skills_dir: str) -> None:
        scanner = dcc_mcp_core.SkillScanner()
        dirs = scanner.scan(extra_paths=[skills_dir])
        names = {Path(d).name for d in dirs}
        assert "dcc-cli-gateway" in names

    def test_description_mentions_clawhub_and_cli(self) -> None:
        meta = dcc_mcp_core.parse_skill_md(DCC_CLI_GATEWAY_DIR)
        assert meta is not None
        desc = (meta.description or "").lower()
        assert "openclaw" in desc
        assert "dcc-mcp-cli" in desc
        assert "mcp" in desc

    def test_openclaw_metadata_does_not_require_gateway_env(self) -> None:
        meta = dcc_mcp_core.parse_skill_md(DCC_CLI_GATEWAY_DIR)
        assert meta is not None
        assert meta.primary_env() is None
        assert "DCC_MCP_BASE_URL" not in meta.required_env_vars()

    def test_reference_docs_present(self) -> None:
        root = Path(DCC_CLI_GATEWAY_DIR)
        assert (root / "references" / "CLI_CHEATSHEET.md").is_file()
        assert (root / "references" / "ZERO_INSTANCES_CLI.md").is_file()

    def test_probe_cli_missing(self) -> None:
        with patch.object(check_cli_mod.shutil, "which", return_value=None):
            with patch.object(check_cli_mod.dcc_gateway, "python_fallback", return_value={}):
                payload = check_cli_mod.probe(cli="missing-dcc-mcp-cli", base_url="http://127.0.0.1:9765")
        assert payload["cli_ok"] is False
        assert payload["gateway_ok"] is False
        assert payload["total"] == 0

    def test_probe_download_failure_falls_back_to_python_rest(self) -> None:
        fallback = {
            "total": 2,
            "instances": [
                {"dcc_type": "houdini"},
                {"dcc_type": "custom"},
            ],
        }
        with patch.object(check_cli_mod.shutil, "which", return_value=None):
            with patch.object(
                check_cli_mod.dcc_gateway,
                "install_cli",
                return_value=(False, "download failed", "https://example.invalid"),
            ):
                with patch.object(check_cli_mod.dcc_gateway, "python_fallback", return_value=fallback):
                    payload = check_cli_mod.probe(
                        cli="missing-dcc-mcp-cli",
                        base_url="http://127.0.0.1:9765",
                        ensure_cli=True,
                    )

        assert payload["cli_ok"] is False
        assert payload["install_attempted"] is True
        assert payload["install_ok"] is False
        assert payload["fallback"] == "python-stdlib-rest"
        assert payload["gateway_ok"] is True
        assert payload["by_dcc_type"] == {"houdini": 1, "custom": 1}

    def test_probe_parses_cli_instances(self) -> None:
        def fake_run(argv, capture_output=True, text=True, timeout=0, check=False):
            class Proc:
                returncode = 0
                stderr = ""

                @property
                def stdout(self) -> str:
                    if argv[-1] == "health":
                        return json.dumps({"ok": True})
                    return json.dumps(
                        {
                            "total": 3,
                            "instances": [
                                {"dcc_type": "maya"},
                                {"dcc_type": "maya"},
                                {"dcc_type": "photoshop"},
                            ],
                        }
                    )

            return Proc()

        with patch.object(check_cli_mod.shutil, "which", return_value="dcc-mcp-cli"):
            with patch.object(check_cli_mod.subprocess, "run", fake_run):
                payload = check_cli_mod.probe(cli="dcc-mcp-cli", base_url="http://127.0.0.1:9765")

        assert payload["cli_ok"] is True
        assert payload["gateway_ok"] is True
        assert payload["total"] == 3
        assert payload["by_dcc_type"] == {"maya": 2, "photoshop": 1}

    def test_check_cli_outputs_json_when_cli_missing(self) -> None:
        assert CHECK_SCRIPT.is_file()
        result = subprocess.run(
            [sys.executable, str(CHECK_SCRIPT), "--cli", "missing-dcc-mcp-cli-for-test"],
            capture_output=True,
            text=True,
            timeout=30,
            check=False,
        )
        assert result.returncode == 0, result.stderr
        payload = json.loads(result.stdout.strip())
        assert payload["cli_ok"] is False

    def test_gateway_helper_python_fallback_search(self) -> None:
        args = dcc_gateway_mod.build_parser().parse_args(
            [
                "--base-url",
                "http://127.0.0.1:9765",
                "search",
                "--query",
                "sphere",
                "--dcc-type",
                "maya",
            ]
        )
        with patch.object(dcc_gateway_mod, "resolve_cli", return_value=(None, {"cli": "dcc-mcp-cli"})):
            with patch.object(
                dcc_gateway_mod, "_request_json", return_value={"hits": [{"slug": "maya.abc.tool"}]}
            ) as request:
                payload = dcc_gateway_mod.run_command("search", args)

        request.assert_called_once_with(
            "http://127.0.0.1:9765",
            "POST",
            "/v1/search",
            {"query": "sphere", "dcc_type": "maya"},
        )
        assert payload["hits"][0]["slug"] == "maya.abc.tool"
        assert payload["_transport"] == "python-stdlib-rest"
