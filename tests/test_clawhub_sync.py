"""Tests for ClawHub publish manifest and dry-run script."""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import subprocess
import sys

from conftest import REPO_ROOT
from dcc_mcp_core import parse_skill_md
from dcc_mcp_core import yaml_loads

MANIFEST = REPO_ROOT / ".github" / "clawhub-skills.json"
RELEASE_PLEASE_CONFIG = REPO_ROOT / "release-please-config.json"
RELEASE_MANIFEST = REPO_ROOT / ".release-please-manifest.json"
RELEASE_WORKFLOW = REPO_ROOT / ".github" / "workflows" / "release.yml"
SYNC_SCRIPT = REPO_ROOT / "scripts" / "clawhub_sync.py"


def load_sync_module():
    """Load clawhub_sync.py as an importable module for focused unit tests."""
    spec = importlib.util.spec_from_file_location("clawhub_sync_under_test", SYNC_SCRIPT)
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class TestClawhubSync:
    def test_manifest_lists_clawhub_skills(self) -> None:
        entries = json.loads(MANIFEST.read_text(encoding="utf-8"))
        slugs = {e["slug"] for e in entries}
        assert "dcc-cli-gateway" in slugs
        assert "dcc-mcp-skills-creator" in slugs
        assert "dcc-mcp-creator" in slugs

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
        assert "clawhub@0.17.0" in proc.stdout
        assert "dcc-cli-gateway" in proc.stdout
        assert "dcc-mcp-skills-creator" in proc.stdout
        assert "dcc-mcp-creator" in proc.stdout

    def test_publish_skips_existing_clawhub_version(self, tmp_path, monkeypatch, capsys) -> None:
        sync = load_sync_module()
        skill_dir = tmp_path / "skills" / "example"
        skill_dir.mkdir(parents=True)

        class CleanReport:
            is_clean = True
            issues: tuple[str, ...] = ()

        def fake_run(cmd, *, check, capture_output, text):
            assert check is False
            assert capture_output is True
            assert text is True
            return subprocess.CompletedProcess(
                cmd,
                1,
                stdout="- Preparing example@1.2.3\n",
                stderr="Error: Uncaught ConvexError: Version already exists\n",
            )

        monkeypatch.setattr(sync, "REPO_ROOT", tmp_path)
        monkeypatch.setattr(sync, "skill_version", lambda _skill_dir: "1.2.3")
        monkeypatch.setattr(sync, "skill_license", lambda _skill_dir: sync.CLAWHUB_LICENSE)
        monkeypatch.setattr(sync.dcc_mcp_core, "validate_skill", lambda _skill_dir: CleanReport())
        monkeypatch.setattr(sync.subprocess, "run", fake_run)

        rc = sync.publish_one(
            {"path": "skills/example", "slug": "example"},
            dry_run=False,
            cli="clawhub@test",
        )

        captured = capsys.readouterr()
        assert rc == 0
        assert "Version already exists" in captured.err
        assert "example@1.2.3 already exists on ClawHub; skipping." in captured.out

    def test_clawhub_skill_versions_follow_release_please(self) -> None:
        entries = json.loads(MANIFEST.read_text(encoding="utf-8"))
        release_version = json.loads(RELEASE_MANIFEST.read_text(encoding="utf-8"))["."]
        for entry in entries:
            meta = parse_skill_md(str(REPO_ROOT / entry["path"]))
            assert meta is not None
            assert meta.version == release_version

    def test_release_please_updates_published_skill_versions(self) -> None:
        entries = json.loads(MANIFEST.read_text(encoding="utf-8"))
        config = json.loads(RELEASE_PLEASE_CONFIG.read_text(encoding="utf-8"))
        extra_files = {item["path"] for item in config["packages"]["."]["extra-files"] if item.get("type") == "generic"}
        for entry in entries:
            assert f"{entry['path']}/SKILL.md" in extra_files

    def test_release_workflow_publishes_clawhub_skills_on_release(self) -> None:
        workflow = yaml_loads(RELEASE_WORKFLOW.read_text(encoding="utf-8"))
        job = workflow["jobs"]["publish-clawhub-skills"]
        assert job["needs"] == ["release-please"]
        assert job["if"] == "needs.release-please.outputs.release_created == 'true'"
        assert job["uses"] == "./.github/workflows/clawhub.yml"
        assert job["with"]["checkout-ref"] == "${{ needs.release-please.outputs.tag_name }}"
        assert job["with"]["publish"] is True
        assert job["secrets"] == "inherit"
