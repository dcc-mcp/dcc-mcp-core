"""Tests for the dcc-mcp default agent-control skill."""

from __future__ import annotations

import ast
import hashlib
import io
import json
from pathlib import Path
import re
import subprocess
import sys
from unittest.mock import patch

from conftest import REPO_ROOT
import dcc_mcp_core

DCC_MCP_SKILL_DIR = str(REPO_ROOT / "skills" / "dcc-mcp")
CHECK_SCRIPT = Path(DCC_MCP_SKILL_DIR) / "scripts" / "check_cli.py"
CLAWHUB_MANIFEST = REPO_ROOT / ".github" / "clawhub-skills.json"

sys.path.insert(0, str(CHECK_SCRIPT.parent))
import check_cli as check_cli_mod  # noqa: E402
import dcc_gateway as dcc_gateway_mod  # noqa: E402

sys.path.pop(0)


class _BytesResponse(io.BytesIO):
    def __enter__(self):
        return self

    def __exit__(self, *_args):
        self.close()


class TestDccMcpSkill:
    def test_skill_dir_exists(self) -> None:
        assert Path(DCC_MCP_SKILL_DIR).is_dir()

    def test_parse_skill_md(self) -> None:
        meta = dcc_mcp_core.parse_skill_md(DCC_MCP_SKILL_DIR)
        assert meta is not None
        assert meta.name == "dcc-mcp"
        entries = json.loads(CLAWHUB_MANIFEST.read_text(encoding="utf-8"))
        manifest_version = next(entry["version"] for entry in entries if entry["slug"] == "dcc-mcp")
        assert meta.version == manifest_version

    def test_validate_skill_clean(self) -> None:
        report = dcc_mcp_core.validate_skill(DCC_MCP_SKILL_DIR)
        assert report.is_clean, report.issues

    def test_scannable_from_skills_dir(self, skills_dir: str) -> None:
        scanner = dcc_mcp_core.SkillScanner()
        dirs = scanner.scan(extra_paths=[skills_dir])
        names = {Path(d).name for d in dirs}
        assert "dcc-mcp" in names

    def test_description_mentions_clawhub_and_cli(self) -> None:
        meta = dcc_mcp_core.parse_skill_md(DCC_MCP_SKILL_DIR)
        assert meta is not None
        desc = (meta.description or "").lower()
        assert "openclaw" in desc
        assert "dcc-mcp-cli" in desc
        assert "mcp" in desc

    def test_description_triggers_on_natural_dcc_requests(self) -> None:
        meta = dcc_mcp_core.parse_skill_md(DCC_MCP_SKILL_DIR)
        assert meta is not None
        desc = (meta.description or "").lower()
        for keyword in ("maya", "blender", "houdini", "photoshop", "godot", "renderdoc"):
            assert keyword in desc
        assert "use this skill first" in desc

    def test_marketplace_intent_is_cli_search_first(self) -> None:
        meta = dcc_mcp_core.parse_skill_md(DCC_MCP_SKILL_DIR)
        assert meta is not None
        desc = (meta.description or "").lower()
        assert "marketplace" in desc
        assert "query the marketplace" in desc

        body = " ".join((Path(DCC_MCP_SKILL_DIR) / "SKILL.md").read_text(encoding="utf-8").lower().split())
        assert "marketplace intent" in body
        assert "dcc-mcp-cli marketplace search" in body
        assert 'dcc-mcp-cli marketplace search --query "maya rigging"' in body
        assert "does not require a live dcc instance" in body
        assert "never invent a package name" in body
        assert "dcc-mcp-cli marketplace inspect" in body

        release_compatible_examples = {
            REPO_ROOT / "README.md": 'dcc-mcp-cli search --query "create sphere"',
            REPO_ROOT / "docs" / "guide" / "cli-reference.md": (
                'dcc-mcp-cli marketplace search --query "maya rigging"'
            ),
            REPO_ROOT / "docs" / "guide" / "marketplace.md": "marketplace search --query <q>",
            Path(DCC_MCP_SKILL_DIR) / "references" / "CLI_CHEATSHEET.md": (
                'dcc-mcp-cli marketplace search --query "maya rigging"'
            ),
        }
        for path, example in release_compatible_examples.items():
            assert example in path.read_text(encoding="utf-8"), path
        cli_reference = (REPO_ROOT / "docs" / "guide" / "cli-reference.md").read_text(encoding="utf-8")
        assert "marketplace search [-q\\|--query <q>] [--dcc <dcc>]" in cli_reference

    def test_body_prioritizes_structured_dcc_mcp_tools_for_user_intent(self) -> None:
        body = (Path(DCC_MCP_SKILL_DIR) / "SKILL.md").read_text(encoding="utf-8").lower()
        assert "dcc intent routing" in body
        assert "in maya" in body
        assert "in blender" in body
        assert "in photoshop" in body
        assert "prefer structured dcc-mcp tools" in body
        assert "mcp-native host" in body
        assert "call the gateway/dcc structured tools directly" in body
        assert "dcc-mcp-cli dcc-types" in body
        assert "dcc-mcp-cli stats" in body
        assert "review_skill_improvement" in body
        assert "do not use this skill" not in body

    def test_openclaw_metadata_does_not_require_gateway_env(self) -> None:
        meta = dcc_mcp_core.parse_skill_md(DCC_MCP_SKILL_DIR)
        assert meta is not None
        assert meta.primary_env() is None
        assert "DCC_MCP_BASE_URL" not in meta.required_env_vars()

    def test_reference_docs_present(self) -> None:
        root = Path(DCC_MCP_SKILL_DIR)
        assert (root / "references" / "CLI_CHEATSHEET.md").is_file()
        assert (root / "references" / "ZERO_INSTANCES_CLI.md").is_file()
        assert len((root / "SKILL.md").read_text(encoding="utf-8").splitlines()) <= 550

    def test_public_skill_has_no_remote_pipe_to_shell_bootstrap(self) -> None:
        pattern = re.compile(
            r"(?:curl|irm|invoke-restmethod)[^\r\n|]*\|[^\r\n]*(?:sh|bash|iex|invoke-expression)",
            re.IGNORECASE,
        )
        root = Path(DCC_MCP_SKILL_DIR)
        for path in sorted(root.rglob("*")):
            if path.is_file() and path.suffix.lower() in {".md", ".py", ".yaml", ".yml", ".json"}:
                assert pattern.search(path.read_text(encoding="utf-8")) is None, path

    def test_cli_bootstrap_uses_fixed_official_source(self) -> None:
        source = (Path(DCC_MCP_SKILL_DIR) / "scripts" / "dcc_gateway.py").read_text(encoding="utf-8")
        assert "DCC_MCP_REPO" not in source
        assert "--repo" not in source
        assert "sha256" in source.lower()
        assert "update-manifest" in source
        assert "verified" in (check_cli_mod.__doc__ or "").lower()

    def test_packaged_helpers_keep_python_37_syntax_compatibility(self) -> None:
        parse_kwargs = {"feature_version": 7} if sys.version_info >= (3, 8) else {}
        scripts = Path(DCC_MCP_SKILL_DIR) / "scripts"
        for path in (scripts / "dcc_gateway.py", scripts / "check_cli.py"):
            source = path.read_text(encoding="utf-8")
            assert "from __future__ import annotations" in source
            ast.parse(source, filename=str(path), **parse_kwargs)

    def test_verified_cli_bootstrap_accepts_matching_manifest(self, tmp_path) -> None:
        binary = b"verified dcc-mcp-cli"
        digest = hashlib.sha256(binary).hexdigest()
        asset_url = "https://github.com/dcc-mcp/dcc-mcp-core/releases/download/v0.19.63/dcc-mcp-cli-linux-x86_64"
        manifest = json.dumps(
            {
                "dcc-mcp-cli": {
                    "version": "0.19.63",
                    "url": asset_url,
                    "sha256": digest,
                }
            }
        ).encode()

        with patch.object(dcc_gateway_mod.platform, "system", return_value="Linux"):
            with patch.object(dcc_gateway_mod.platform, "machine", return_value="x86_64"):
                with patch.object(
                    dcc_gateway_mod.urllib.request,
                    "urlopen",
                    side_effect=[_BytesResponse(manifest), _BytesResponse(binary)],
                ):
                    with patch.object(
                        dcc_gateway_mod.urllib.request,
                        "urlretrieve",
                        side_effect=AssertionError("unverified downloader must not be used"),
                    ):
                        ok, installed, resolved_url = dcc_gateway_mod.install_cli(
                            install_dir=tmp_path,
                            version="v0.19.63",
                        )

        assert ok is True
        assert Path(installed).read_bytes() == binary
        assert resolved_url == asset_url

    def test_verified_cli_bootstrap_rejects_checksum_mismatch_without_replacing_binary(self, tmp_path) -> None:
        target = tmp_path / "dcc-mcp-cli"
        target.write_bytes(b"known-good-existing-cli")
        binary = b"tampered download"
        asset_url = "https://github.com/dcc-mcp/dcc-mcp-core/releases/download/v0.19.63/dcc-mcp-cli-linux-x86_64"
        manifest = json.dumps(
            {
                "dcc-mcp-cli": {
                    "version": "0.19.63",
                    "url": asset_url,
                    "sha256": "0" * 64,
                }
            }
        ).encode()

        with patch.object(dcc_gateway_mod.platform, "system", return_value="Linux"):
            with patch.object(dcc_gateway_mod.platform, "machine", return_value="x86_64"):
                with patch.object(
                    dcc_gateway_mod.urllib.request,
                    "urlopen",
                    side_effect=[_BytesResponse(manifest), _BytesResponse(binary)],
                ):
                    with patch.object(
                        dcc_gateway_mod.urllib.request,
                        "urlretrieve",
                        side_effect=AssertionError("unverified downloader must not be used"),
                    ):
                        ok, message, resolved_url = dcc_gateway_mod.install_cli(
                            install_dir=tmp_path,
                            version="v0.19.63",
                        )

        assert ok is False
        assert "sha-256" in message.lower()
        assert resolved_url == asset_url
        assert target.read_bytes() == b"known-good-existing-cli"

    def test_verified_cli_bootstrap_rejects_non_official_asset_url(self, tmp_path) -> None:
        manifest = json.dumps(
            {
                "dcc-mcp-cli": {
                    "version": "0.19.63",
                    "url": "https://example.invalid/dcc-mcp-cli-linux-x86_64",
                    "sha256": "a" * 64,
                }
            }
        ).encode()

        with patch.object(dcc_gateway_mod.platform, "system", return_value="Linux"):
            with patch.object(dcc_gateway_mod.platform, "machine", return_value="x86_64"):
                with patch.object(
                    dcc_gateway_mod.urllib.request,
                    "urlopen",
                    return_value=_BytesResponse(manifest),
                ) as opener:
                    ok, message, resolved_url = dcc_gateway_mod.install_cli(
                        install_dir=tmp_path,
                        version="v0.19.63",
                    )

        assert ok is False
        assert "official release" in message.lower()
        assert resolved_url is None
        assert opener.call_count == 1
        assert not (tmp_path / "dcc-mcp-cli").exists()

    def test_verified_cli_bootstrap_latest_uses_manifest_pinned_asset(self, tmp_path) -> None:
        binary = b"release-pinned-cli"
        digest = hashlib.sha256(binary).hexdigest()
        asset_url = "https://github.com/dcc-mcp/dcc-mcp-core/releases/download/v0.19.63/dcc-mcp-cli-linux-x86_64"
        manifest = json.dumps(
            {
                "dcc-mcp-cli": {
                    "version": "0.19.63",
                    "url": asset_url,
                    "sha256": digest,
                }
            }
        ).encode()
        requested_urls: list[str] = []

        def open_url(url, **_kwargs):
            requested_urls.append(str(url))
            return _BytesResponse(manifest if len(requested_urls) == 1 else binary)

        with patch.object(dcc_gateway_mod.platform, "system", return_value="Linux"):
            with patch.object(dcc_gateway_mod.platform, "machine", return_value="x86_64"):
                with patch.object(dcc_gateway_mod.urllib.request, "urlopen", side_effect=open_url):
                    ok, installed, resolved_url = dcc_gateway_mod.install_cli(install_dir=tmp_path)

        assert ok is True
        assert Path(installed).read_bytes() == binary
        assert resolved_url == asset_url
        assert requested_urls == [
            "https://github.com/dcc-mcp/dcc-mcp-core/releases/latest/download/"
            "dcc-mcp-update-manifest-linux-x86_64.json",
            asset_url,
        ]

    def test_verified_cli_bootstrap_rejects_requested_version_mismatch(self, tmp_path) -> None:
        asset_url = "https://github.com/dcc-mcp/dcc-mcp-core/releases/download/v0.19.62/dcc-mcp-cli-linux-x86_64"
        manifest = json.dumps(
            {
                "dcc-mcp-cli": {
                    "version": "0.19.62",
                    "url": asset_url,
                    "sha256": "a" * 64,
                }
            }
        ).encode()

        with patch.object(dcc_gateway_mod.platform, "system", return_value="Linux"):
            with patch.object(dcc_gateway_mod.platform, "machine", return_value="x86_64"):
                with patch.object(
                    dcc_gateway_mod.urllib.request,
                    "urlopen",
                    return_value=_BytesResponse(manifest),
                ) as opener:
                    ok, message, resolved_url = dcc_gateway_mod.install_cli(
                        install_dir=tmp_path,
                        version="v0.19.63",
                    )

        assert ok is False
        assert "does not match" in message.lower()
        assert resolved_url is None
        assert opener.call_count == 1
        assert not (tmp_path / "dcc-mcp-cli").exists()

    def test_probe_cli_missing(self) -> None:
        with patch.object(check_cli_mod.shutil, "which", return_value=None):
            with patch.object(check_cli_mod.dcc_gateway, "install_cli") as installer:
                with patch.object(check_cli_mod.dcc_gateway, "python_fallback", return_value={}):
                    payload = check_cli_mod.probe(
                        cli="missing-dcc-mcp-cli",
                        base_url="http://127.0.0.1:9765",
                    )
        installer.assert_not_called()
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
