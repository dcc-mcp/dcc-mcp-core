"""Tests for SkillScanner, SkillMetadata, and parse_skill_md."""

# Import future modules
from __future__ import annotations

# Import built-in modules
from pathlib import Path

# Import local modules
from conftest import create_skill_dir
import dcc_mcp_core


class TestSkillMetadata:
    def test_create_default(self) -> None:
        sm = dcc_mcp_core.SkillMetadata(name="test-skill")
        assert sm.name == "test-skill"
        assert sm.description == ""
        assert sm.dcc == "python"
        assert sm.version == "1.0.0"
        assert sm.tags == []
        assert sm.scripts == []
        assert sm.tools == []
        assert sm.skill_path == ""

    def test_create_full(self) -> None:
        sm = dcc_mcp_core.SkillMetadata(
            name="maya-tool",
            description="A Maya tool",
            tools=["read", "write"],
            dcc="maya",
            tags=["geometry"],
            scripts=["hello.py"],
            skill_path="/path/to/skill",
            version="2.0.0",
        )
        assert sm.dcc == "maya"
        assert sm.description == "A Maya tool"
        assert sm.tools == ["read", "write"]
        assert sm.tags == ["geometry"]
        assert sm.scripts == ["hello.py"]
        assert sm.skill_path == "/path/to/skill"
        assert sm.version == "2.0.0"

    def test_setters(self) -> None:
        sm = dcc_mcp_core.SkillMetadata(name="old")
        sm.name = "new"
        sm.description = "new desc"
        sm.tools = ["tool1"]
        sm.dcc = "houdini"
        sm.tags = ["tag1", "tag2"]
        sm.scripts = ["s.py"]
        sm.skill_path = "/new/path"
        sm.version = "3.0.0"
        assert sm.name == "new"
        assert sm.description == "new desc"
        assert sm.tools == ["tool1"]
        assert sm.dcc == "houdini"
        assert sm.tags == ["tag1", "tag2"]
        assert sm.scripts == ["s.py"]
        assert sm.skill_path == "/new/path"
        assert sm.version == "3.0.0"

    def test_repr(self) -> None:
        sm = dcc_mcp_core.SkillMetadata(name="test", dcc="maya")
        r = repr(sm)
        assert "test" in r
        assert "maya" in r


class TestSkillScanner:
    def test_create(self) -> None:
        scanner = dcc_mcp_core.SkillScanner()
        assert scanner.discovered_skills == []

    def test_scan_empty_dir(self, tmp_path: Path) -> None:
        scanner = dcc_mcp_core.SkillScanner()
        result = scanner.scan(extra_paths=[str(tmp_path)])
        assert result == []

    def test_scan_with_skill(self, tmp_path: Path) -> None:
        scanner = dcc_mcp_core.SkillScanner()
        create_skill_dir(str(tmp_path), "my-skill", dcc="maya", body="# My Skill\n")
        result = scanner.scan(extra_paths=[str(tmp_path)])
        assert len(result) == 1
        assert "my-skill" in result[0]

    def test_scan_updates_discovered_skills(self, tmp_path: Path) -> None:
        scanner = dcc_mcp_core.SkillScanner()
        create_skill_dir(str(tmp_path), "s1")
        scanner.scan(extra_paths=[str(tmp_path)])
        assert len(scanner.discovered_skills) == 1

    def test_scan_multiple_skills(self, tmp_path: Path) -> None:
        scanner = dcc_mcp_core.SkillScanner()
        for name in ["skill-a", "skill-b", "skill-c"]:
            create_skill_dir(str(tmp_path), name)
        result = scanner.scan(extra_paths=[str(tmp_path)])
        assert len(result) == 3

    def test_scan_ignores_non_skill_dirs(self, tmp_path: Path) -> None:
        scanner = dcc_mcp_core.SkillScanner()
        # dir without SKILL.md
        (tmp_path / "not-a-skill").mkdir()
        # file (not dir)
        (tmp_path / "file.txt").write_text("not a skill", encoding="utf-8")
        result = scanner.scan(extra_paths=[str(tmp_path)])
        assert result == []

    def test_scan_with_dcc_name(self, tmp_path: Path) -> None:
        scanner = dcc_mcp_core.SkillScanner()
        create_skill_dir(str(tmp_path), "s1")
        result = scanner.scan(extra_paths=[str(tmp_path)], dcc_name="maya")
        assert len(result) == 1

    def test_scan_force_refresh(self, tmp_path: Path) -> None:
        scanner = dcc_mcp_core.SkillScanner()
        create_skill_dir(str(tmp_path), "s1")
        r1 = scanner.scan(extra_paths=[str(tmp_path)])
        r2 = scanner.scan(extra_paths=[str(tmp_path)], force_refresh=True)
        assert r1 == r2

    def test_scan_uses_cache(self, tmp_path: Path) -> None:
        scanner = dcc_mcp_core.SkillScanner()
        create_skill_dir(str(tmp_path), "s1")
        r1 = scanner.scan(extra_paths=[str(tmp_path)])
        r2 = scanner.scan(extra_paths=[str(tmp_path)])  # should use cache
        assert r1 == r2

    def test_clear_cache(self, tmp_path: Path) -> None:
        scanner = dcc_mcp_core.SkillScanner()
        create_skill_dir(str(tmp_path), "s1")
        scanner.scan(extra_paths=[str(tmp_path)])
        scanner.clear_cache()
        assert scanner.discovered_skills == []


class TestScanSkillPaths:
    def test_scan_nonexistent(self) -> None:
        result = dcc_mcp_core.scan_skill_paths(extra_paths=["/nonexistent"])
        assert result == []

    def test_scan_with_skills(self, tmp_path: Path) -> None:
        create_skill_dir(str(tmp_path), "my-skill")
        result = dcc_mcp_core.scan_skill_paths(extra_paths=[str(tmp_path)])
        assert len(result) == 1

    def test_scan_with_dcc_name(self, tmp_path: Path) -> None:
        create_skill_dir(str(tmp_path), "s1")
        result = dcc_mcp_core.scan_skill_paths(extra_paths=[str(tmp_path)], dcc_name="blender")
        assert len(result) == 1


class TestParseSkillMd:
    def test_parse_valid(self, tmp_path: Path) -> None:
        (tmp_path / "SKILL.md").write_text(
            "---\nname: test-skill\ndcc: maya\ntags:\n  - geo\n---\n# Body\n", encoding="utf-8"
        )
        tmpdir = str(tmp_path)
        meta = dcc_mcp_core.parse_skill_md(tmpdir)
        assert meta is not None
        assert meta.name == "test-skill"
        assert meta.dcc == "maya"
        assert meta.tags == ["geo"]
        assert meta.skill_path == tmpdir

    def test_parse_with_scripts(self, tmp_path: Path) -> None:
        (tmp_path / "SKILL.md").write_text("---\nname: scripted\n---\n", encoding="utf-8")
        scripts_dir = tmp_path / "scripts"
        scripts_dir.mkdir()
        (scripts_dir / "hello.py").write_text("print('hello')", encoding="utf-8")
        (scripts_dir / "run.sh").write_text("echo hello", encoding="utf-8")
        meta = dcc_mcp_core.parse_skill_md(str(tmp_path))
        assert meta is not None
        assert len(meta.scripts) == 2

    def test_parse_no_skill_md(self, tmp_path: Path) -> None:
        result = dcc_mcp_core.parse_skill_md(str(tmp_path))
        assert result is None

    def test_parse_invalid_yaml(self, tmp_path: Path) -> None:
        (tmp_path / "SKILL.md").write_text("---\n: invalid yaml [[\n---\n", encoding="utf-8")
        result = dcc_mcp_core.parse_skill_md(str(tmp_path))
        assert result is None

    def test_parse_no_frontmatter(self, tmp_path: Path) -> None:
        (tmp_path / "SKILL.md").write_text("# No frontmatter\nJust body text.", encoding="utf-8")
        result = dcc_mcp_core.parse_skill_md(str(tmp_path))
        assert result is None
