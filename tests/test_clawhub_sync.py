"""Tests for ClawHub publish manifest and dry-run script."""

from __future__ import annotations

import importlib.util
import io
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
README = REPO_ROOT / "README.md"


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
        assert "dcc-mcp" in slugs
        assert "dcc-mcp-skills-creator" in slugs
        assert "dcc-mcp-creator" in slugs
        assert {entry["owner"] for entry in entries} == {"loonghao"}

    def test_published_skills_include_codex_interface_metadata(self) -> None:
        entries = json.loads(MANIFEST.read_text(encoding="utf-8"))
        for entry in entries:
            metadata_path = REPO_ROOT / entry["path"] / "agents" / "openai.yaml"
            assert metadata_path.is_file(), metadata_path
            interface = yaml_loads(metadata_path.read_text(encoding="utf-8"))["interface"]
            assert 25 <= len(interface["short_description"]) <= 64
            assert f"${entry['slug']}" in interface["default_prompt"]

    def test_readme_installs_the_complete_agent_skill_suite(self) -> None:
        readme = README.read_text(encoding="utf-8")
        for slug in ("dcc-mcp", "dcc-mcp-skills-creator", "dcc-mcp-creator"):
            qualified = f"@loonghao/{slug}"
            assert f"openclaw skills install {qualified}" in readme
            assert f"clawhub@0.17.0 install {slug}" in readme
        assert ".github/clawhub-skills.json" in readme

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
        assert "dcc-mcp" in proc.stdout
        assert "dcc-mcp-skills-creator" in proc.stdout
        assert "dcc-mcp-creator" in proc.stdout
        assert proc.stdout.count("--owner loonghao") == 3

    def test_dry_run_does_not_publish_or_verify(self, tmp_path, monkeypatch) -> None:
        sync = load_sync_module()
        skill_dir = tmp_path / "skills" / "example"
        skill_dir.mkdir(parents=True)

        class CleanReport:
            is_clean = True
            issues: tuple[str, ...] = ()

        def fail(*_args, **_kwargs):
            raise AssertionError("dry-run must not publish or access the public API")

        monkeypatch.setattr(sync, "REPO_ROOT", tmp_path)
        monkeypatch.setattr(sync, "skill_version", lambda _skill_dir: "1.2.3")
        monkeypatch.setattr(sync, "skill_license", lambda _skill_dir: sync.CLAWHUB_LICENSE)
        monkeypatch.setattr(sync.dcc_mcp_core, "validate_skill", lambda _skill_dir: CleanReport())
        monkeypatch.setattr(sync.subprocess, "run", fail)
        monkeypatch.setattr(sync, "verify_published_version", fail)

        rc = sync.publish_one(
            {"path": "skills/example", "slug": "example", "owner": "loonghao"},
            dry_run=True,
            cli="clawhub@test",
        )

        assert rc == 0

    def test_public_version_request_is_owner_qualified(self, monkeypatch) -> None:
        sync = load_sync_module()
        requests = []

        def fake_urlopen(request, *, timeout):
            requests.append((request, timeout))
            return io.BytesIO(b'{"version":{"version":"1.2.3"}}')

        monkeypatch.setattr(sync, "urlopen", fake_urlopen)

        version = sync.published_skill_version("example-skill", "loonghao", "1.2.3")

        assert version == "1.2.3"
        assert requests[0][0].full_url == (
            "https://clawhub.ai/api/v1/skills/example-skill/versions/1.2.3?owner=loonghao"
        )
        assert requests[0][1] == 20

    def test_public_version_gate_accepts_existing_version_when_latest_is_newer(self, monkeypatch) -> None:
        sync = load_sync_module()

        def fake_urlopen(_request, *, timeout):
            assert timeout == 20
            return io.BytesIO(b'{"version":{"version":"1.2.3"},"latestVersion":{"version":"2.0.0"}}')

        def fail_sleep(_seconds):
            raise AssertionError("version is already public")

        monkeypatch.setattr(sync, "urlopen", fake_urlopen)
        monkeypatch.setattr(sync.time, "sleep", fail_sleep)

        assert sync.verify_published_version("example-skill", "loonghao", "1.2.3") is True

    def test_public_version_gate_retries_until_version_is_visible(self, monkeypatch) -> None:
        sync = load_sync_module()
        versions = iter(["1.2.2", "1.2.2", "1.2.3"])
        calls = []
        sleeps = []

        def fake_release(slug, owner, version):
            calls.append((slug, owner, version))
            return {"version": next(versions), "files": []}

        monkeypatch.setattr(sync, "published_skill_release", fake_release)
        monkeypatch.setattr(sync.time, "sleep", sleeps.append)

        assert sync.verify_published_version("example", "loonghao", "1.2.3") is True
        assert calls == [("example", "loonghao", "1.2.3")] * sync.MAX_RETRIES
        assert sleeps == [2, 4]

    def test_public_version_gate_honors_retry_after(self, monkeypatch) -> None:
        sync = load_sync_module()
        responses = iter(
            [
                sync.HTTPError("https://clawhub.ai", 429, "rate limited", {"Retry-After": "17"}, None),
                {"version": "1.2.3", "files": []},
            ]
        )
        sleeps = []

        def fake_release(*_args):
            response = next(responses)
            if isinstance(response, Exception):
                raise response
            return response

        monkeypatch.setattr(sync, "published_skill_release", fake_release)
        monkeypatch.setattr(sync.time, "sleep", sleeps.append)

        assert sync.verify_published_version("example", "loonghao", "1.2.3") is True
        assert sleeps == [17]

    def test_public_version_gate_fails_after_bounded_retries(self, monkeypatch, capsys) -> None:
        sync = load_sync_module()
        calls = []
        sleeps = []

        def fake_release(slug, owner, version):
            calls.append((slug, owner, version))
            return {"version": "1.2.2", "files": []}

        monkeypatch.setattr(sync, "published_skill_release", fake_release)
        monkeypatch.setattr(sync.time, "sleep", sleeps.append)

        assert sync.verify_published_version("example", "loonghao", "1.2.3") is False
        assert calls == [("example", "loonghao", "1.2.3")] * sync.MAX_RETRIES
        assert sleeps == [2, 4]
        assert "public endpoint returned 1.2.2, expected 1.2.3" in capsys.readouterr().err

    def test_public_version_gate_checks_required_file_hashes(self, tmp_path, monkeypatch, capsys) -> None:
        sync = load_sync_module()
        skill_dir = tmp_path / "example"
        (skill_dir / "agents").mkdir(parents=True)
        (skill_dir / "SKILL.md").write_text("---\nname: example\n---\n", encoding="utf-8")
        (skill_dir / "agents" / "openai.yaml").write_text(
            "interface:\n  short_description: Example\n",
            encoding="utf-8",
        )
        published = {
            "version": "1.2.3",
            "files": [
                {"path": relative, "sha256": sync.file_sha256(skill_dir / relative)}
                for relative in sync.REQUIRED_PUBLIC_FILES
            ],
        }
        monkeypatch.setattr(sync, "published_skill_release", lambda *_args: published)
        monkeypatch.setattr(sync, "MAX_RETRIES", 1)

        assert sync.verify_published_version("example", "loonghao", "1.2.3", skill_dir) is True

        published["files"][1]["sha256"] = "0" * 64
        assert sync.verify_published_version("example", "loonghao", "1.2.3", skill_dir) is False
        assert "public file hash mismatch: agents/openai.yaml" in capsys.readouterr().err

    def test_existing_version_fails_when_not_publicly_visible(self, tmp_path, monkeypatch, capsys) -> None:
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
                stderr="Error: Uncaught ConvexError: Version 1.2.3 already exists\n",
            )

        monkeypatch.setattr(sync, "REPO_ROOT", tmp_path)
        monkeypatch.setattr(sync, "skill_version", lambda _skill_dir: "1.2.3")
        monkeypatch.setattr(sync, "skill_license", lambda _skill_dir: sync.CLAWHUB_LICENSE)
        monkeypatch.setattr(sync.dcc_mcp_core, "validate_skill", lambda _skill_dir: CleanReport())
        monkeypatch.setattr(sync.subprocess, "run", fake_run)
        monkeypatch.setattr(sync, "verify_published_version", lambda *_args: False)

        rc = sync.publish_one(
            {"path": "skills/example", "slug": "example", "owner": "loonghao"},
            dry_run=False,
            cli="clawhub@test",
        )

        captured = capsys.readouterr()
        assert rc == 1
        assert "Version 1.2.3 already exists" in captured.err
        assert "example@1.2.3 already exists on ClawHub; skipping." in captured.out

    def test_publish_retries_on_embedding_failure_then_succeeds(self, tmp_path, monkeypatch, capsys) -> None:
        sync = load_sync_module()
        skill_dir = tmp_path / "skills" / "example"
        skill_dir.mkdir(parents=True)

        class CleanReport:
            is_clean = True
            issues: tuple[str, ...] = ()

        calls: list[list[str]] = []

        def fake_run(cmd, *, check, capture_output, text):
            assert check is False
            assert capture_output is True
            assert text is True
            calls.append(cmd)
            if len(calls) == 1:
                return subprocess.CompletedProcess(
                    cmd,
                    1,
                    stdout="- Preparing example@1.2.3\n",
                    stderr="Error: Embedding failed. Please try again. (reset in 22s)\n",
                )
            return subprocess.CompletedProcess(
                cmd,
                0,
                stdout="- Published example@1.2.3\n",
                stderr="",
            )

        monkeypatch.setattr(sync, "REPO_ROOT", tmp_path)
        monkeypatch.setattr(sync, "skill_version", lambda _skill_dir: "1.2.3")
        monkeypatch.setattr(sync, "skill_license", lambda _skill_dir: sync.CLAWHUB_LICENSE)
        monkeypatch.setattr(sync.dcc_mcp_core, "validate_skill", lambda _skill_dir: CleanReport())
        monkeypatch.setattr(sync.subprocess, "run", fake_run)
        monkeypatch.setattr(sync.time, "sleep", lambda _s: None)
        monkeypatch.setattr(sync, "verify_published_version", lambda *_args: True)

        rc = sync.publish_one(
            {"path": "skills/example", "slug": "example", "owner": "loonghao"},
            dry_run=False,
            cli="clawhub@test",
        )

        captured = capsys.readouterr()
        assert rc == 0
        assert len(calls) == 2
        assert "Embedding failed" in captured.err
        assert "retrying in 23s" in captured.out
        assert "Published example@1.2.3" in captured.out

    def test_publish_retries_embedding_failure_then_treats_existing_as_success(
        self, tmp_path, monkeypatch, capsys
    ) -> None:
        sync = load_sync_module()
        skill_dir = tmp_path / "skills" / "example"
        skill_dir.mkdir(parents=True)

        class CleanReport:
            is_clean = True
            issues: tuple[str, ...] = ()

        calls: list[list[str]] = []

        def fake_run(cmd, *, check, capture_output, text):
            assert check is False
            assert capture_output is True
            assert text is True
            calls.append(cmd)
            if len(calls) == 1:
                return subprocess.CompletedProcess(
                    cmd,
                    1,
                    stdout="- Preparing example@1.2.3\n",
                    stderr="Error: Embedding failed. Please try again.\n",
                )
            return subprocess.CompletedProcess(
                cmd,
                1,
                stdout="- Preparing example@1.2.3\n",
                stderr="Error: Uncaught ConvexError: Version 1.2.3 already exists\n",
            )

        monkeypatch.setattr(sync, "REPO_ROOT", tmp_path)
        monkeypatch.setattr(sync, "skill_version", lambda _skill_dir: "1.2.3")
        monkeypatch.setattr(sync, "skill_license", lambda _skill_dir: sync.CLAWHUB_LICENSE)
        monkeypatch.setattr(sync.dcc_mcp_core, "validate_skill", lambda _skill_dir: CleanReport())
        monkeypatch.setattr(sync.subprocess, "run", fake_run)
        monkeypatch.setattr(sync.time, "sleep", lambda _s: None)
        monkeypatch.setattr(sync, "verify_published_version", lambda *_args: True)

        rc = sync.publish_one(
            {"path": "skills/example", "slug": "example", "owner": "loonghao"},
            dry_run=False,
            cli="clawhub@test",
        )

        captured = capsys.readouterr()
        assert rc == 0
        assert len(calls) == 2
        assert "example@1.2.3 already exists on ClawHub; skipping." in captured.out

    def test_publish_retries_exhausted_on_embedding_failure(self, tmp_path, monkeypatch, capsys) -> None:
        sync = load_sync_module()
        skill_dir = tmp_path / "skills" / "example"
        skill_dir.mkdir(parents=True)

        class CleanReport:
            is_clean = True
            issues: tuple[str, ...] = ()

        calls: list[list[str]] = []

        def fake_run(cmd, *, check, capture_output, text):
            assert check is False
            assert capture_output is True
            assert text is True
            calls.append(cmd)
            return subprocess.CompletedProcess(
                cmd,
                1,
                stdout="- Preparing example@1.2.3\n",
                stderr="Error: Embedding failed. Please try again. (reset in 22s)\n",
            )

        monkeypatch.setattr(sync, "REPO_ROOT", tmp_path)
        monkeypatch.setattr(sync, "skill_version", lambda _skill_dir: "1.2.3")
        monkeypatch.setattr(sync, "skill_license", lambda _skill_dir: sync.CLAWHUB_LICENSE)
        monkeypatch.setattr(sync.dcc_mcp_core, "validate_skill", lambda _skill_dir: CleanReport())
        monkeypatch.setattr(sync.subprocess, "run", fake_run)
        monkeypatch.setattr(sync.time, "sleep", lambda _s: None)

        rc = sync.publish_one(
            {"path": "skills/example", "slug": "example", "owner": "loonghao"},
            dry_run=False,
            cli="clawhub@test",
        )

        captured = capsys.readouterr()
        assert rc == 1
        assert len(calls) == sync.MAX_RETRIES
        assert captured.out.count("retrying in 23s") == 2

    def test_publish_no_retry_on_permanent_error(self, tmp_path, monkeypatch, capsys) -> None:
        sync = load_sync_module()
        skill_dir = tmp_path / "skills" / "example"
        skill_dir.mkdir(parents=True)

        class CleanReport:
            is_clean = True
            issues: tuple[str, ...] = ()

        calls: list[list[str]] = []

        def fake_run(cmd, *, check, capture_output, text):
            assert check is False
            assert capture_output is True
            assert text is True
            calls.append(cmd)
            return subprocess.CompletedProcess(
                cmd,
                1,
                stdout="- Preparing example@1.2.3\n",
                stderr="Error: Unknown permanent error\n",
            )

        monkeypatch.setattr(sync, "REPO_ROOT", tmp_path)
        monkeypatch.setattr(sync, "skill_version", lambda _skill_dir: "1.2.3")
        monkeypatch.setattr(sync, "skill_license", lambda _skill_dir: sync.CLAWHUB_LICENSE)
        monkeypatch.setattr(sync.dcc_mcp_core, "validate_skill", lambda _skill_dir: CleanReport())
        monkeypatch.setattr(sync.subprocess, "run", fake_run)
        monkeypatch.setattr(sync.time, "sleep", lambda _s: None)

        rc = sync.publish_one(
            {"path": "skills/example", "slug": "example", "owner": "loonghao"},
            dry_run=False,
            cli="clawhub@test",
        )

        assert rc == 1
        assert len(calls) == 1

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
