"""Tests for deterministic ClawHub skill archives."""

from __future__ import annotations

import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
from zipfile import ZipFile

import pytest

from conftest import REPO_ROOT

SCRIPT = REPO_ROOT / "scripts" / "package_openclaw_skill.py"
MANIFEST = REPO_ROOT / ".github" / "clawhub-skills.json"


def load_packager():
    """Load the packaging script without invoking its CLI."""
    spec = importlib.util.spec_from_file_location("package_openclaw_skill_under_test", SCRIPT)
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_package_excludes_python_cache_and_bytecode(tmp_path: Path) -> None:
    packager = load_packager()
    skill_dir = tmp_path / "skills" / "dcc-mcp"
    script_dir = skill_dir / "scripts"
    agents_dir = skill_dir / "agents"
    cache_dir = script_dir / "__pycache__"
    cache_dir.mkdir(parents=True)
    agents_dir.mkdir()
    (skill_dir / "SKILL.md").write_text("---\nname: dcc-mcp\n---\n", encoding="utf-8")
    (agents_dir / "openai.yaml").write_text('interface:\n  display_name: "DCC-MCP"\n', encoding="utf-8")
    (script_dir / "helper.py").write_text("VALUE = 1\n", encoding="utf-8")
    (cache_dir / "helper.cpython-312.pyc").write_bytes(b"bytecode")
    (script_dir / "orphan.pyc").write_bytes(b"bytecode")

    output_dir = tmp_path / "dist"
    output_dir.mkdir()
    archive_path = packager.package_skill(skill_dir, output_dir, "1.2.3")

    with ZipFile(archive_path) as archive:
        names = set(archive.namelist())
    assert names == {
        "dcc-mcp/SKILL.md",
        "dcc-mcp/agents/openai.yaml",
        "dcc-mcp/scripts/helper.py",
    }


def test_manifest_packages_each_skill_with_its_independent_version(tmp_path: Path) -> None:
    packager = load_packager()
    repo_root = tmp_path / "repo"
    first = repo_root / "skills" / "first"
    second = repo_root / "skills" / "second"
    first.mkdir(parents=True)
    second.mkdir(parents=True)
    (first / "SKILL.md").write_text("---\nname: first\n---\n", encoding="utf-8")
    (second / "SKILL.md").write_text("---\nname: second\n---\n", encoding="utf-8")
    manifest = repo_root / "clawhub-skills.json"
    manifest.write_text(
        json.dumps(
            [
                {"path": "skills/first", "version": "1.2.3"},
                {"path": "skills/second", "version": "2.0.0"},
            ]
        ),
        encoding="utf-8",
    )

    releases = packager.resolve_manifest_skills(manifest, repo_root)
    output_dir = tmp_path / "dist"
    output_dir.mkdir()
    archives = [packager.package_skill(skill_dir, output_dir, version) for skill_dir, version in releases]

    assert [archive.name for archive in archives] == ["first-1.2.3.zip", "second-2.0.0.zip"]


def test_manifest_cli_packages_the_repository_suite(tmp_path: Path) -> None:
    proc = subprocess.run(
        [sys.executable, str(SCRIPT), str(MANIFEST), str(tmp_path), "--manifest"],
        capture_output=True,
        text=True,
        check=False,
        timeout=30,
    )

    assert proc.returncode == 0, proc.stderr
    entries = json.loads(MANIFEST.read_text(encoding="utf-8"))["skills"]
    expected = {f"{Path(entry['path']).name}-{entry['version']}.zip" for entry in entries}
    assert {path.name for path in tmp_path.glob("*.zip")} == expected


def test_package_rejects_symbolic_links(tmp_path: Path) -> None:
    packager = load_packager()
    skill_dir = tmp_path / "skills" / "example"
    skill_dir.mkdir(parents=True)
    (skill_dir / "SKILL.md").write_text("---\nname: example\n---\n", encoding="utf-8")
    outside = tmp_path / "outside.txt"
    outside.write_text("secret\n", encoding="utf-8")
    link = skill_dir / "outside.txt"
    try:
        link.symlink_to(outside)
    except OSError as error:
        pytest.skip(f"symbolic links are unavailable: {error}")

    with pytest.raises(ValueError, match="symbolic link"):
        list(packager.iter_skill_files(skill_dir))


def test_package_all_rejects_symbolic_link_skill_directory(tmp_path: Path) -> None:
    packager = load_packager()
    skills_root = tmp_path / "skills"
    real_skill = tmp_path / "real-skill"
    skills_root.mkdir()
    real_skill.mkdir()
    (real_skill / "SKILL.md").write_text("---\nname: linked\n---\n", encoding="utf-8")
    linked_skill = skills_root / "linked"
    try:
        linked_skill.symlink_to(real_skill, target_is_directory=True)
    except OSError as error:
        pytest.skip(f"symbolic links are unavailable: {error}")

    with pytest.raises(ValueError, match="symbolic-link Skill directory"):
        packager.resolve_skill_dirs(skills_root, package_all=True)


def test_package_all_containment_catches_unreported_directory_link(tmp_path: Path, monkeypatch) -> None:
    packager = load_packager()
    skills_root = tmp_path / "skills"
    real_skill = tmp_path / "real-skill"
    skills_root.mkdir()
    real_skill.mkdir()
    (real_skill / "SKILL.md").write_text("---\nname: linked\n---\n", encoding="utf-8")
    linked_skill = skills_root / "linked"
    try:
        linked_skill.symlink_to(real_skill, target_is_directory=True)
    except OSError as error:
        pytest.skip(f"directory links are unavailable: {error}")
    original_is_symlink = Path.is_symlink

    def hide_link_from_pathlib(path: Path) -> bool:
        if path == linked_skill:
            return False
        return original_is_symlink(path)

    monkeypatch.setattr(Path, "is_symlink", hide_link_from_pathlib)
    with pytest.raises(ValueError, match="Skill directory escapes skills root"):
        packager.resolve_skill_dirs(skills_root, package_all=True)


def test_package_rejects_unsafe_direct_version(tmp_path: Path) -> None:
    packager = load_packager()
    skill_dir = tmp_path / "skills" / "example"
    output_dir = tmp_path / "dist"
    skill_dir.mkdir(parents=True)
    output_dir.mkdir()
    (skill_dir / "SKILL.md").write_text("---\nname: example\n---\n", encoding="utf-8")

    with pytest.raises(ValueError, match="invalid stable Skill archive version"):
        packager.package_skill(skill_dir, output_dir, "../../victim")


def test_package_rejects_output_inside_skill_directory(tmp_path: Path) -> None:
    packager = load_packager()
    skill_dir = tmp_path / "skills" / "example"
    output_dir = skill_dir / "dist"
    output_dir.mkdir(parents=True)
    (skill_dir / "SKILL.md").write_text("---\nname: example\n---\n", encoding="utf-8")

    with pytest.raises(ValueError, match="output directory cannot be inside Skill directory"):
        packager.package_skill(skill_dir, output_dir, "1.2.3")


def test_python37_workspace_version_fallback_parser() -> None:
    packager = load_packager()
    raw = """
[workspace]
members = []

[workspace.package]
version = "1.2.3" # release
edition = "2021"
"""

    assert packager.workspace_version_from_text(raw) == "1.2.3"


def test_package_is_reproducible_across_source_mtime_changes(tmp_path: Path) -> None:
    packager = load_packager()
    skill_dir = tmp_path / "skills" / "example"
    skill_dir.mkdir(parents=True)
    skill_md = skill_dir / "SKILL.md"
    skill_md.write_text("---\nname: example\n---\n", encoding="utf-8")
    first_output = tmp_path / "first"
    second_output = tmp_path / "second"
    first_output.mkdir()
    second_output.mkdir()

    first_bytes = packager.package_skill(skill_dir, first_output, "1.2.3").read_bytes()
    stat = skill_md.stat()
    os.utime(skill_md, (stat.st_atime + 60, stat.st_mtime + 60))
    second_bytes = packager.package_skill(skill_dir, second_output, "1.2.3").read_bytes()

    assert first_bytes == second_bytes


@pytest.mark.parametrize("version", ["", "next", "../1.2.3", "1.2.3-01", "1.2.3-rc.1"])
def test_manifest_rejects_unsafe_versions(tmp_path: Path, version: str) -> None:
    packager = load_packager()
    repo_root = tmp_path / "repo"
    skill_dir = repo_root / "skills" / "example"
    skill_dir.mkdir(parents=True)
    (skill_dir / "SKILL.md").write_text("---\nname: example\n---\n", encoding="utf-8")
    manifest = repo_root / "clawhub-skills.json"
    manifest.write_text(
        json.dumps([{"path": "skills/example", "version": version}]),
        encoding="utf-8",
    )

    with pytest.raises(ValueError):
        packager.resolve_manifest_skills(manifest, repo_root)
