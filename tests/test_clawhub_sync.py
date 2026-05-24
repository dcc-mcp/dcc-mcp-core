"""Tests for ClawHub publish manifest and dry-run script."""

from __future__ import annotations

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


class TestClawhubSync:
    def test_manifest_lists_clawhub_skills(self) -> None:
        entries = json.loads(MANIFEST.read_text(encoding="utf-8"))
        slugs = {e["slug"] for e in entries}
        assert "dcc-rest-gateway" in slugs
        assert "dcc-cli-gateway" in slugs

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
        assert "dcc-rest-gateway" in proc.stdout
        assert "dcc-cli-gateway" in proc.stdout

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
