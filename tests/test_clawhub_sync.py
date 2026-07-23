"""Tests for ClawHub publish manifest and dry-run script."""

from __future__ import annotations

import hashlib
import importlib.util
import io
import json
from pathlib import Path
import re
import shutil
import subprocess

import pytest

from conftest import REPO_ROOT
from dcc_mcp_core import parse_skill_md
from dcc_mcp_core import yaml_loads

MANIFEST = REPO_ROOT / ".github" / "clawhub-skills.json"
RELEASE_PLEASE_CONFIG = REPO_ROOT / "release-please-config.json"
CLAWHUB_WORKFLOW = REPO_ROOT / ".github" / "workflows" / "clawhub.yml"
RELEASE_WORKFLOW = REPO_ROOT / ".github" / "workflows" / "release.yml"
SYNC_SCRIPT = REPO_ROOT / "scripts" / "clawhub_sync.py"
README = REPO_ROOT / "README.md"


def manifest_entries() -> list[dict[str, str]]:
    """Return the official ClawHub Skill entries."""
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    assert set(manifest) == {"skills"}
    return manifest["skills"]


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
        entries = manifest_entries()
        slugs = {e["slug"] for e in entries}
        assert "dcc-mcp" in slugs
        assert "dcc-mcp-skills-creator" in slugs
        assert "dcc-mcp-creator" in slugs
        assert {entry["owner"] for entry in entries} == {"loonghao"}
        assert all(re.fullmatch(r"\d+\.\d+\.\d+", entry["version"]) for entry in entries)

    def test_published_skills_include_codex_interface_metadata(self) -> None:
        entries = manifest_entries()
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
            assert f"clawhub@0.23.1 install {qualified}" in readme
        assert ".github/clawhub-skills.json" in readme
        assert "versioned independently" in readme

    def test_dry_run_uses_current_cli_and_real_publish_preview(self, monkeypatch) -> None:
        sync = load_sync_module()
        calls: list[list[str]] = []

        def fake_run(cmd, *, check, capture_output, text):
            assert check is False
            assert capture_output is True
            assert text is True
            calls.append(cmd)
            payload = {
                "ok": True,
                "status": "would-publish",
                "slug": cmd[cmd.index("--slug") + 1],
                "version": cmd[cmd.index("--version") + 1],
                "fileCount": 2,
                "fingerprint": "a" * 64,
            }
            return subprocess.CompletedProcess(cmd, 0, stdout=json.dumps(payload), stderr="")

        monkeypatch.setattr(sync.subprocess, "run", fake_run)
        entries = manifest_entries()
        for entry in entries:
            assert sync.publish_one(entry, dry_run=True, cli=sync.DEFAULT_CLI) == 0

        assert sync.DEFAULT_CLI == "clawhub@0.23.1"
        assert len(calls) == 3
        for cmd in calls:
            assert Path(cmd[0]).name.lower() in {"npx", "npx.cmd", "npx.exe"}
            assert cmd[1:5] == ["clawhub@0.23.1", "--no-input", "skill", "publish"]
            assert cmd[-2:] == ["--json", "--dry-run"]
            assert "--owner" in cmd
            assert cmd[cmd.index("--owner") + 1] == "loonghao"

    def test_dry_run_does_not_verify_uploaded_state(self, tmp_path, monkeypatch) -> None:
        sync = load_sync_module()
        skill_dir = tmp_path / "skills" / "example"
        skill_dir.mkdir(parents=True)

        class CleanReport:
            is_clean = True
            issues: tuple[str, ...] = ()

        calls = []

        def fake_run(cmd, *, check, capture_output, text):
            calls.append(cmd)
            payload = {
                "ok": True,
                "status": "would-publish",
                "slug": "example",
                "version": "1.2.3",
                "fileCount": 2,
                "fingerprint": "a" * 64,
            }
            return subprocess.CompletedProcess(cmd, 0, stdout=json.dumps(payload), stderr="")

        def fail(*_args, **_kwargs):
            raise AssertionError("dry-run must not inspect uploaded or public state")

        monkeypatch.setattr(sync, "REPO_ROOT", tmp_path)
        monkeypatch.setattr(sync, "skill_version", lambda _skill_dir: "1.2.3")
        monkeypatch.setattr(sync, "skill_license", lambda _skill_dir: sync.CLAWHUB_LICENSE)
        monkeypatch.setattr(sync.dcc_mcp_core, "validate_skill", lambda _skill_dir: CleanReport())
        monkeypatch.setattr(sync.subprocess, "run", fake_run)
        monkeypatch.setattr(sync, "verify_uploaded_version", fail)

        rc = sync.publish_one(
            {"path": "skills/example", "slug": "example", "owner": "loonghao", "version": "1.2.3"},
            dry_run=True,
            cli="clawhub@test",
        )

        assert rc == 0
        assert calls[0][-1] == "--dry-run"

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
            "https://clawhub.ai/api/v1/skills/example-skill/versions/1.2.3?ownerHandle=loonghao"
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

    def test_public_version_gate_reports_pending_review_for_hidden_release(self, monkeypatch, capsys) -> None:
        sync = load_sync_module()

        def hidden_release(*_args):
            raise sync.HTTPError("https://clawhub.ai", 404, "not found", {}, None)

        monkeypatch.setattr(sync, "published_skill_release", hidden_release)
        monkeypatch.setattr(sync, "MAX_RETRIES", 1)

        assert sync.verify_published_version("example", "loonghao", "1.2.3") is None
        assert "review or moderation may still be pending" in capsys.readouterr().out

    @pytest.mark.parametrize("failure", ["rate-limit", "service", "network", "schema"])
    def test_public_version_gate_does_not_mislabel_service_errors_as_review(self, failure, monkeypatch, capsys) -> None:
        sync = load_sync_module()

        def unavailable_release(*_args):
            if failure == "rate-limit":
                raise sync.HTTPError("https://clawhub.ai", 429, "rate limited", {}, None)
            if failure == "service":
                raise sync.HTTPError("https://clawhub.ai", 503, "unavailable", {}, None)
            if failure == "network":
                raise OSError("network unavailable")
            raise ValueError("unexpected public schema")

        monkeypatch.setattr(sync, "published_skill_release", unavailable_release)
        monkeypatch.setattr(sync, "MAX_RETRIES", 1)

        assert sync.verify_published_version("example", "loonghao", "1.2.3") is False
        captured = capsys.readouterr()
        assert "public verification failed" in captured.err
        assert "pending ClawHub review" not in captured.out

    def test_owner_inspect_gate_checks_identity_and_all_file_hashes(self, tmp_path, monkeypatch) -> None:
        sync = load_sync_module()
        skill_dir = tmp_path / "example"
        (skill_dir / "agents").mkdir(parents=True)
        (skill_dir / "SKILL.md").write_text("---\nname: example\n---\n", encoding="utf-8")
        (skill_dir / "agents" / "openai.yaml").write_text(
            "interface:\n  short_description: Example\n",
            encoding="utf-8",
        )
        (skill_dir / "notes.md").write_text("release notes\n", encoding="utf-8")
        published = {
            "version": "1.2.3",
            "files": [
                {
                    "path": path.relative_to(skill_dir).as_posix(),
                    "sha256": sync.file_sha256(path),
                }
                for path in sorted(skill_dir.rglob("*"))
                if path.is_file()
            ],
        }
        payload = {
            "skill": {"slug": "example"},
            "owner": {"handle": "loonghao"},
            "version": published,
        }
        calls = []

        def fake_run(cmd, *, check, capture_output, text):
            calls.append(cmd)
            return subprocess.CompletedProcess(cmd, 0, stdout=json.dumps(payload), stderr="")

        monkeypatch.setattr(sync.subprocess, "run", fake_run)
        expected_fingerprint = "a" * 64
        monkeypatch.setattr(sync, "clawhub_file_fingerprint", lambda _hashes: expected_fingerprint)
        monkeypatch.setattr(sync, "MAX_RETRIES", 1)

        assert (
            sync.verify_owner_version(
                "clawhub@test",
                "example",
                "loonghao",
                "1.2.3",
                skill_dir,
                3,
                expected_fingerprint,
            )
            is True
        )
        assert len(calls) == 1
        assert Path(calls[0][0]).name.lower() in {"npx", "npx.cmd", "npx.exe"}
        assert calls[0][1:] == [
            "clawhub@test",
            "--no-input",
            "inspect",
            "@loonghao/example",
            "--version",
            "1.2.3",
            "--files",
            "--json",
        ]

        published["files"][0]["sha256"] = "0" * 64
        assert (
            sync.verify_owner_version(
                "clawhub@test",
                "example",
                "loonghao",
                "1.2.3",
                skill_dir,
                3,
                expected_fingerprint,
            )
            is False
        )

    def test_owner_inspect_rejects_malware_blocked_release(self) -> None:
        sync = load_sync_module()
        payload = {
            "skill": {"slug": "example"},
            "owner": {"handle": "loonghao"},
            "version": {"version": "1.2.3", "files": []},
            "moderation": {
                "isMalwareBlocked": True,
                "verdict": "malicious",
                "reasonCodes": ["malware-detected"],
            },
        }

        with pytest.raises(ValueError, match="malware-blocked"):
            sync.parse_owner_release(
                json.dumps(payload),
                slug="example",
                owner="loonghao",
                version="1.2.3",
            )

    def test_owner_inspect_rejects_malicious_version_security(self) -> None:
        sync = load_sync_module()
        payload = {
            "skill": {"slug": "example"},
            "owner": {"handle": "loonghao"},
            "version": {
                "version": "1.2.3",
                "files": [],
                "security": {"status": "malicious"},
            },
            "moderation": None,
        }

        with pytest.raises(ValueError, match="security status 'malicious'"):
            sync.parse_owner_release(
                json.dumps(payload),
                slug="example",
                owner="loonghao",
                version="1.2.3",
            )

    def test_owner_inspect_reports_pending_review_for_hidden_version(self, monkeypatch, capsys) -> None:
        sync = load_sync_module()

        monkeypatch.setattr(
            sync.subprocess,
            "run",
            lambda cmd, **_kwargs: subprocess.CompletedProcess(
                cmd,
                1,
                stdout="",
                stderr="Error: Version not found (reset in 52s)\n",
            ),
        )
        monkeypatch.setattr(sync, "MAX_RETRIES", 1)

        assert (
            sync.verify_owner_version(
                "clawhub@test",
                "example",
                "loonghao",
                "1.2.3",
                Path("example"),
                3,
                "a" * 64,
            )
            is None
        )
        assert "review or moderation may still be pending" in capsys.readouterr().out

    @pytest.mark.parametrize(
        "message",
        [
            "This skill is pending a ClawScan security review. Please try again in a few minutes.",
            "Skill is hidden while security scan is pending. Try again in a few minutes.",
        ],
    )
    def test_owner_inspect_reports_explicit_security_review_lock_as_pending(self, message, monkeypatch, capsys) -> None:
        sync = load_sync_module()

        monkeypatch.setattr(
            sync.subprocess,
            "run",
            lambda cmd, **_kwargs: subprocess.CompletedProcess(
                cmd,
                1,
                stdout="",
                stderr=f"Error: HTTP 423: {message}\n",
            ),
        )
        monkeypatch.setattr(sync, "MAX_RETRIES", 1)

        assert (
            sync.verify_owner_version(
                "clawhub@test",
                "example",
                "loonghao",
                "1.2.3",
                Path("example"),
                3,
                "a" * 64,
            )
            is None
        )
        assert "review or moderation may still be pending" in capsys.readouterr().out

    def test_owner_inspect_reports_moderation_only_payload_as_pending(self, monkeypatch, capsys) -> None:
        sync = load_sync_module()
        payload = {
            "skill": None,
            "latestVersion": None,
            "owner": None,
            "moderation": {
                "isMalwareBlocked": False,
                "isSuspicious": False,
                "verdict": "clean",
            },
            "version": None,
            "versions": None,
            "file": None,
        }
        monkeypatch.setattr(
            sync.subprocess,
            "run",
            lambda cmd, **_kwargs: subprocess.CompletedProcess(
                cmd,
                0,
                stdout=json.dumps(payload),
                stderr="",
            ),
        )
        monkeypatch.setattr(sync, "MAX_RETRIES", 1)

        assert (
            sync.verify_owner_version(
                "clawhub@test",
                "example",
                "loonghao",
                "1.2.3",
                Path("example"),
                3,
                "a" * 64,
            )
            is None
        )
        assert "moderation-only" in capsys.readouterr().out

    def test_publish_receipt_requires_exact_server_acknowledgement(self) -> None:
        sync = load_sync_module()
        payload = {
            "ok": True,
            "status": "published",
            "slug": "example",
            "version": "1.2.3",
            "fileCount": 3,
            "fingerprint": "a" * 64,
            "versionId": "k97example",
            "folder": str(Path("example").resolve()),
        }

        assert (
            sync.validate_publish_receipt(
                json.dumps(payload),
                slug="example",
                version="1.2.3",
                expected_file_count=3,
                expected_fingerprint="a" * 64,
                expected_folder=Path("example"),
            )
            == "k97example"
        )

        payload["fingerprint"] = "b" * 64
        with pytest.raises(ValueError, match="fingerprint"):
            sync.validate_publish_receipt(
                json.dumps(payload),
                slug="example",
                version="1.2.3",
                expected_file_count=3,
                expected_fingerprint="a" * 64,
                expected_folder=Path("example"),
            )

    @pytest.mark.parametrize(
        ("field", "value"),
        [("fileCount", True), ("versionId", ""), ("folder", "other-skill")],
    )
    def test_publish_receipt_rejects_invalid_acknowledgement_fields(self, field, value) -> None:
        sync = load_sync_module()
        payload = {
            "ok": True,
            "status": "published",
            "slug": "example",
            "version": "1.2.3",
            "fileCount": 3,
            "fingerprint": "a" * 64,
            "versionId": "k97example",
            "folder": str(Path("example").resolve()),
        }
        payload[field] = value

        with pytest.raises(ValueError, match=field):
            sync.validate_publish_receipt(
                json.dumps(payload),
                slug="example",
                version="1.2.3",
                expected_file_count=3,
                expected_fingerprint="a" * 64,
                expected_folder=Path("example"),
            )

    def test_hidden_immutable_version_is_resolved_by_exact_fingerprint(self, monkeypatch) -> None:
        sync = load_sync_module()
        calls: list[list[str]] = []

        def fake_run(cmd, **_kwargs):
            calls.append(cmd)
            payload = {
                "ok": True,
                "status": "unchanged",
                "slug": "example",
                "version": "1.2.3",
                "fileCount": 3,
                "fingerprint": "a" * 64,
            }
            return subprocess.CompletedProcess(cmd, 0, stdout=json.dumps(payload), stderr="")

        monkeypatch.setattr(sync.subprocess, "run", fake_run)

        assert sync.verify_resolved_fingerprint(
            "clawhub@test",
            "example",
            "loonghao",
            "1.2.3",
            Path("example"),
            3,
            "a" * 64,
        )
        assert calls[0][-2:] == ["--json", "--dry-run"]
        assert "--version" not in calls[0]

    @pytest.mark.parametrize(
        ("field", "value"),
        [("status", "would-publish"), ("version", "1.2.4"), ("fingerprint", "b" * 64)],
    )
    def test_fingerprint_resolver_rejects_unmatched_remote_state(self, field, value, monkeypatch) -> None:
        sync = load_sync_module()
        payload = {
            "ok": True,
            "status": "unchanged",
            "slug": "example",
            "version": "1.2.3",
            "fileCount": 3,
            "fingerprint": "a" * 64,
        }
        payload[field] = value
        monkeypatch.setattr(
            sync.subprocess,
            "run",
            lambda cmd, **_kwargs: subprocess.CompletedProcess(
                cmd,
                0,
                stdout=json.dumps(payload),
                stderr="",
            ),
        )

        assert not sync.verify_resolved_fingerprint(
            "clawhub@test",
            "example",
            "loonghao",
            "1.2.3",
            Path("example"),
            3,
            "a" * 64,
        )

    @pytest.mark.parametrize(
        "payload",
        [
            "not-json",
            json.dumps(
                {
                    "ok": True,
                    "status": "would-publish",
                    "slug": "wrong",
                    "version": "1.2.3",
                    "fileCount": 2,
                    "fingerprint": "a" * 64,
                }
            ),
        ],
    )
    def test_publish_preview_rejects_untrusted_output(self, payload, monkeypatch) -> None:
        sync = load_sync_module()
        monkeypatch.setattr(
            sync.subprocess,
            "run",
            lambda *_args, **_kwargs: subprocess.CompletedProcess([], 0, stdout=payload, stderr=""),
        )

        assert sync.preview_publish_metadata([], slug="example", version="1.2.3") is None

    @pytest.mark.skipif(shutil.which("node") is None, reason="Node.js is required by ClawHub")
    def test_fingerprint_matches_clawhub_reference_order(self) -> None:
        sync = load_sync_module()
        file_hashes = {
            "SKILL.md": "1" * 64,
            "agents/openai.yaml": "2" * 64,
        }
        payload = f"agents/openai.yaml:{'2' * 64}\nSKILL.md:{'1' * 64}"
        expected = hashlib.sha256(payload.encode()).hexdigest()

        assert sync.clawhub_file_fingerprint(file_hashes) == expected

    def test_same_count_selection_drift_fails_fingerprint_gate(self, tmp_path, monkeypatch) -> None:
        sync = load_sync_module()
        skill_dir = tmp_path / "example"
        (skill_dir / "agents").mkdir(parents=True)
        for relative, content in {
            "SKILL.md": "skill\n",
            "agents/openai.yaml": "interface: {}\n",
            "old-selection.md": "old\n",
            "new-selection.md": "new\n",
        }.items():
            (skill_dir / relative).write_text(content, encoding="utf-8")
        published = {
            "files": [
                {"path": relative, "sha256": sync.file_sha256(skill_dir / relative)}
                for relative in ("SKILL.md", "agents/openai.yaml", "old-selection.md")
            ]
        }
        monkeypatch.setattr(sync, "clawhub_file_fingerprint", lambda _hashes: "b" * 64)

        mismatches = sync.published_file_mismatches(
            published,
            skill_dir,
            location="owner-visible",
            expected_file_count=3,
            expected_fingerprint="a" * 64,
        )

        assert mismatches == [f"owner-visible fingerprint {'b' * 64}, expected {'a' * 64}"]

    def test_generated_skill_card_is_excluded_from_source_fingerprint(self, tmp_path, monkeypatch) -> None:
        sync = load_sync_module()
        skill_dir = tmp_path / "example"
        (skill_dir / "agents").mkdir(parents=True)
        for relative, content in {
            "SKILL.md": "skill\n",
            "agents/openai.yaml": "interface: {}\n",
        }.items():
            (skill_dir / relative).write_text(content, encoding="utf-8")
        published = {
            "files": [
                {"path": relative, "sha256": sync.file_sha256(skill_dir / relative)}
                for relative in ("SKILL.md", "agents/openai.yaml")
            ]
            + [{"path": "skill-card.md", "sha256": "c" * 64}]
        }
        monkeypatch.setattr(sync, "clawhub_file_fingerprint", lambda _hashes: "a" * 64)

        assert (
            sync.published_file_mismatches(
                published,
                skill_dir,
                location="owner-visible",
                expected_file_count=2,
                expected_fingerprint="a" * 64,
            )
            == []
        )

    def test_uploaded_version_accepts_owner_match_while_public_review_is_pending(self, monkeypatch) -> None:
        sync = load_sync_module()
        monkeypatch.setattr(sync, "preview_publish_metadata", lambda *_args, **_kwargs: (3, "a" * 64))
        monkeypatch.setattr(sync, "verify_resolved_fingerprint", lambda *_args: True)
        monkeypatch.setattr(sync, "verify_owner_version", lambda *_args: True)
        monkeypatch.setattr(sync, "verify_published_version", lambda *_args: None)

        assert (
            sync.verify_uploaded_version(
                "clawhub@test",
                "example",
                "loonghao",
                "1.2.3",
                Path("example"),
            )
            is True
        )

    def test_uploaded_version_accepts_exact_publish_receipt_while_review_is_pending(self, monkeypatch, capsys) -> None:
        sync = load_sync_module()
        receipt = json.dumps(
            {
                "ok": True,
                "status": "published",
                "slug": "example",
                "version": "1.2.3",
                "fileCount": 3,
                "fingerprint": "a" * 64,
                "versionId": "k97example",
                "folder": str(Path("example").resolve()),
            }
        )
        monkeypatch.setattr(sync, "preview_publish_metadata", lambda *_args, **_kwargs: (3, "a" * 64))
        monkeypatch.setattr(sync, "verify_owner_version", lambda *_args: None)
        monkeypatch.setattr(sync, "verify_published_version", lambda *_args: None)

        assert sync.verify_uploaded_version(
            "clawhub@test",
            "example",
            "loonghao",
            "1.2.3",
            Path("example"),
            receipt,
        )
        assert "authenticated publish receipt" in capsys.readouterr().out

    def test_fresh_receipt_uses_frozen_preflight_without_post_publish_preview(self, monkeypatch) -> None:
        sync = load_sync_module()
        receipt = json.dumps(
            {
                "ok": True,
                "status": "published",
                "slug": "example",
                "version": "1.2.3",
                "fileCount": 3,
                "fingerprint": "a" * 64,
                "versionId": "k97example",
                "folder": str(Path("example").resolve()),
            }
        )

        def fail_preview(*_args, **_kwargs):
            raise AssertionError("fresh receipt verification must not perform a post-publish preview")

        monkeypatch.setattr(sync, "preview_publish_metadata", fail_preview)
        monkeypatch.setattr(sync, "verify_owner_version", lambda *_args: None)

        assert sync.verify_uploaded_version(
            "clawhub@test",
            "example",
            "loonghao",
            "1.2.3",
            Path("example"),
            receipt,
            publish_metadata=(3, "a" * 64),
        )

    def test_existing_hidden_version_requires_fingerprint_resolution(self, monkeypatch) -> None:
        sync = load_sync_module()
        calls: list[tuple] = []
        monkeypatch.setattr(sync, "preview_publish_metadata", lambda *_args, **_kwargs: (3, "a" * 64))

        def resolve(*args):
            calls.append(args)
            return True

        monkeypatch.setattr(sync, "verify_resolved_fingerprint", resolve)
        monkeypatch.setattr(sync, "verify_owner_version", lambda *_args: None)
        monkeypatch.setattr(sync, "verify_published_version", lambda *_args: None)

        assert sync.verify_uploaded_version(
            "clawhub@test",
            "example",
            "loonghao",
            "1.2.3",
            Path("example"),
        )
        assert calls

    def test_unchanged_publish_response_uses_fingerprint_resolution(self, monkeypatch) -> None:
        sync = load_sync_module()
        calls: list[tuple] = []
        unchanged = json.dumps(
            {
                "ok": True,
                "status": "unchanged",
                "slug": "example",
                "version": "1.2.3",
                "fileCount": 3,
                "fingerprint": "a" * 64,
            }
        )
        monkeypatch.setattr(sync, "preview_publish_metadata", lambda *_args, **_kwargs: (3, "a" * 64))

        def resolve(*args):
            calls.append(args)
            return True

        monkeypatch.setattr(sync, "verify_resolved_fingerprint", resolve)
        monkeypatch.setattr(sync, "verify_owner_version", lambda *_args: None)

        assert sync.verify_uploaded_version(
            "clawhub@test",
            "example",
            "loonghao",
            "1.2.3",
            Path("example"),
            unchanged,
        )
        assert calls

    def test_uploaded_version_rejects_public_hash_mismatch(self, monkeypatch) -> None:
        sync = load_sync_module()
        monkeypatch.setattr(sync, "preview_publish_metadata", lambda *_args, **_kwargs: (3, "a" * 64))
        monkeypatch.setattr(sync, "verify_resolved_fingerprint", lambda *_args: True)
        monkeypatch.setattr(sync, "verify_owner_version", lambda *_args: True)
        monkeypatch.setattr(sync, "verify_published_version", lambda *_args: False)

        assert (
            sync.verify_uploaded_version(
                "clawhub@test",
                "example",
                "loonghao",
                "1.2.3",
                Path("example"),
            )
            is False
        )

    def test_manifest_version_must_match_skill_metadata(self, tmp_path, monkeypatch) -> None:
        sync = load_sync_module()
        skill_dir = tmp_path / "skills" / "example"
        skill_dir.mkdir(parents=True)

        def fail(*_args, **_kwargs):
            raise AssertionError("version mismatch must fail before validation or CLI execution")

        monkeypatch.setattr(sync, "REPO_ROOT", tmp_path)
        monkeypatch.setattr(sync, "skill_version", lambda _skill_dir: "1.2.2")
        monkeypatch.setattr(sync, "skill_license", fail)
        monkeypatch.setattr(sync.subprocess, "run", fail)

        assert (
            sync.publish_one(
                {"path": "skills/example", "slug": "example", "owner": "loonghao", "version": "1.2.3"},
                dry_run=True,
                cli="clawhub@test",
            )
            == 1
        )

    def test_manifest_rejects_prerelease_version_before_cli_execution(self, tmp_path, monkeypatch) -> None:
        sync = load_sync_module()

        def fail(*_args, **_kwargs):
            raise AssertionError("invalid stable version must fail before filesystem or CLI access")

        monkeypatch.setattr(sync, "REPO_ROOT", tmp_path)
        monkeypatch.setattr(sync.subprocess, "run", fail)

        assert (
            sync.publish_one(
                {
                    "path": "skills/example",
                    "slug": "example",
                    "owner": "loonghao",
                    "version": "1.2.3-01",
                },
                dry_run=True,
                cli="clawhub@test",
            )
            == 1
        )

    def test_manifest_skill_path_cannot_escape_repository(self, tmp_path, monkeypatch) -> None:
        sync = load_sync_module()
        repo_root = tmp_path / "repo"
        outside = tmp_path / "outside"
        repo_root.mkdir()
        outside.mkdir()

        def fail(*_args, **_kwargs):
            raise AssertionError("escaping paths must fail before Skill parsing")

        monkeypatch.setattr(sync, "REPO_ROOT", repo_root)
        monkeypatch.setattr(sync, "skill_version", fail)

        assert (
            sync.publish_one(
                {"path": "../outside", "slug": "example", "owner": "loonghao", "version": "1.2.3"},
                dry_run=True,
                cli="clawhub@test",
            )
            == 1
        )

    def test_existing_version_requires_owner_visible_hash_match(self, tmp_path, monkeypatch, capsys) -> None:
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
        monkeypatch.setattr(sync, "preview_publish_metadata", lambda *_args, **_kwargs: (3, "a" * 64))
        monkeypatch.setattr(sync.subprocess, "run", fake_run)
        monkeypatch.setattr(sync, "verify_uploaded_version", lambda *_args, **_kwargs: False)

        rc = sync.publish_one(
            {"path": "skills/example", "slug": "example", "owner": "loonghao", "version": "1.2.3"},
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
        verify_calls: list[tuple[tuple[object, ...], dict[str, object]]] = []
        receipt = json.dumps(
            {
                "ok": True,
                "status": "published",
                "slug": "example",
                "version": "1.2.3",
                "fileCount": 3,
                "fingerprint": "a" * 64,
                "versionId": "k97example",
                "folder": str(skill_dir.resolve()),
            }
        )

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
                stdout=receipt,
                stderr="",
            )

        def fake_verify(*args, **kwargs):
            verify_calls.append((args, kwargs))
            return True

        monkeypatch.setattr(sync, "REPO_ROOT", tmp_path)
        monkeypatch.setattr(sync, "skill_version", lambda _skill_dir: "1.2.3")
        monkeypatch.setattr(sync, "skill_license", lambda _skill_dir: sync.CLAWHUB_LICENSE)
        monkeypatch.setattr(sync.dcc_mcp_core, "validate_skill", lambda _skill_dir: CleanReport())
        monkeypatch.setattr(sync, "preview_publish_metadata", lambda *_args, **_kwargs: (3, "a" * 64))
        monkeypatch.setattr(sync.subprocess, "run", fake_run)
        monkeypatch.setattr(sync.time, "sleep", lambda _s: None)
        monkeypatch.setattr(sync, "verify_uploaded_version", fake_verify)

        rc = sync.publish_one(
            {"path": "skills/example", "slug": "example", "owner": "loonghao", "version": "1.2.3"},
            dry_run=False,
            cli="clawhub@test",
        )

        captured = capsys.readouterr()
        assert rc == 0
        assert len(calls) == 2
        assert verify_calls == [
            (
                ("clawhub@test", "example", "loonghao", "1.2.3", skill_dir.resolve(), receipt),
                {"publish_metadata": (3, "a" * 64)},
            )
        ]
        assert "Embedding failed" in captured.err
        assert "retrying in 23s" in captured.out
        assert '"status": "published"' in captured.out

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
        monkeypatch.setattr(sync, "preview_publish_metadata", lambda *_args, **_kwargs: (3, "a" * 64))
        monkeypatch.setattr(sync.subprocess, "run", fake_run)
        monkeypatch.setattr(sync.time, "sleep", lambda _s: None)
        monkeypatch.setattr(sync, "verify_uploaded_version", lambda *_args, **_kwargs: True)

        rc = sync.publish_one(
            {"path": "skills/example", "slug": "example", "owner": "loonghao", "version": "1.2.3"},
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
        monkeypatch.setattr(sync, "preview_publish_metadata", lambda *_args, **_kwargs: (3, "a" * 64))
        monkeypatch.setattr(sync.subprocess, "run", fake_run)
        monkeypatch.setattr(sync.time, "sleep", lambda _s: None)

        rc = sync.publish_one(
            {"path": "skills/example", "slug": "example", "owner": "loonghao", "version": "1.2.3"},
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
        monkeypatch.setattr(sync, "preview_publish_metadata", lambda *_args, **_kwargs: (3, "a" * 64))
        monkeypatch.setattr(sync.subprocess, "run", fake_run)
        monkeypatch.setattr(sync.time, "sleep", lambda _s: None)

        rc = sync.publish_one(
            {"path": "skills/example", "slug": "example", "owner": "loonghao", "version": "1.2.3"},
            dry_run=False,
            cli="clawhub@test",
        )

        assert rc == 1
        assert len(calls) == 1

    def test_clawhub_skill_versions_follow_independent_manifest(self) -> None:
        entries = manifest_entries()
        for entry in entries:
            meta = parse_skill_md(str(REPO_ROOT / entry["path"]))
            assert meta is not None
            assert meta.version == entry["version"]

    def test_release_please_updates_all_clawhub_skill_versions(self) -> None:
        entries = manifest_entries()
        config = json.loads(RELEASE_PLEASE_CONFIG.read_text(encoding="utf-8"))
        extra_files = config["packages"]["."]["extra-files"]
        generic_paths = {item["path"] for item in extra_files if item.get("type") == "generic"}
        manifest_jsonpaths = {
            item["jsonpath"]
            for item in extra_files
            if item.get("type") == "json" and item.get("path") == ".github/clawhub-skills.json"
        }
        release_version = json.loads((REPO_ROOT / ".release-please-manifest.json").read_text(encoding="utf-8"))["."]

        assert manifest_jsonpaths == {"$.skills[*].version"}
        for entry in entries:
            skill_path = REPO_ROOT / entry["path"] / "SKILL.md"
            assert f"{entry['path']}/SKILL.md" in generic_paths
            assert "x-release-please-version" in skill_path.read_text(encoding="utf-8")
            assert entry["version"] == release_version

    def test_clawhub_workflow_is_the_independent_publish_stream(self) -> None:
        workflow = yaml_loads(CLAWHUB_WORKFLOW.read_text(encoding="utf-8"))
        release_workflow = RELEASE_WORKFLOW.read_text(encoding="utf-8")
        assert workflow["env"]["CLAWHUB_CLI_PACKAGE"] == "clawhub@0.23.1"
        assert workflow["on"]["workflow_call"]["secrets"]["CLAWHUB_TOKEN"]["required"] is False
        assert workflow["on"]["pull_request"]["branches"] == ["main"]
        assert ".github/clawhub-skills.json" in workflow["on"]["pull_request"]["paths"]
        assert "skills/**" in workflow["on"]["pull_request"]["paths"]
        assert "push" not in workflow["on"]
        steps = {step["name"]: step for step in workflow["jobs"]["sync-skills"]["steps"] if "name" in step}
        dry_run = steps["Dry-run ClawHub publish"]
        publish = steps["Publish skills to ClawHub"]
        assert steps["Set up Rust"]["uses"] == "dtolnay/rust-toolchain@stable"
        assert "cargo-clippy" in steps["Remove pre-installed Rust component shims"]["run"]
        assert dry_run["run"] == "python scripts/clawhub_sync.py --dry-run"
        assert "github.event_name == 'pull_request'" in dry_run["if"]
        assert publish["run"] == "python scripts/clawhub_sync.py"
        assert "github.event_name == 'workflow_call' && inputs.publish" in publish["if"]

        release = yaml_loads(release_workflow)
        release_job = release["jobs"]["publish-clawhub-skills"]
        assert release_job["needs"] == ["release-please"]
        assert release_job["if"] == "needs.release-please.outputs.release_created == 'true'"
        assert release_job["uses"] == "./.github/workflows/clawhub.yml"
        assert release_job["with"] == {
            "checkout-ref": "${{ needs.release-please.outputs.tag_name }}",
            "publish": True,
        }
        assert release_job["secrets"] == "inherit"
