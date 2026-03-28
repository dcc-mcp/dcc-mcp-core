"""Tests for SkillScanner and SkillMetadata."""

import os
import tempfile

import dcc_mcp_core


class TestSkillMetadata:
    def test_create_default(self):
        sm = dcc_mcp_core.SkillMetadata(name="test-skill")
        assert sm.name == "test-skill"
        assert sm.dcc == "python"
        assert sm.version == "1.0.0"
        assert sm.tags == []
        assert sm.scripts == []

    def test_create_full(self):
        sm = dcc_mcp_core.SkillMetadata(
            name="maya-tool",
            description="A Maya tool",
            dcc="maya",
            tags=["geometry"],
            version="2.0.0",
        )
        assert sm.dcc == "maya"
        assert sm.description == "A Maya tool"
        assert sm.tags == ["geometry"]

    def test_repr(self):
        sm = dcc_mcp_core.SkillMetadata(name="test")
        assert "test" in repr(sm)


class TestSkillScanner:
    def test_create(self):
        scanner = dcc_mcp_core.SkillScanner()
        assert scanner.discovered_skills == []

    def test_scan_empty_dir(self):
        scanner = dcc_mcp_core.SkillScanner()
        with tempfile.TemporaryDirectory() as tmpdir:
            result = scanner.scan(extra_paths=[tmpdir])
            assert result == []

    def test_scan_with_skill(self):
        scanner = dcc_mcp_core.SkillScanner()
        with tempfile.TemporaryDirectory() as tmpdir:
            skill_dir = os.path.join(tmpdir, "my-skill")
            os.makedirs(skill_dir)
            with open(os.path.join(skill_dir, "SKILL.md"), "w") as f:
                f.write("---\nname: my-skill\ndcc: maya\n---\n# My Skill\n")
            result = scanner.scan(extra_paths=[tmpdir])
            assert len(result) == 1
            assert "my-skill" in result[0]

    def test_clear_cache(self):
        scanner = dcc_mcp_core.SkillScanner()
        scanner.clear_cache()
        assert scanner.discovered_skills == []


class TestScanSkillPaths:
    def test_scan_nonexistent(self):
        result = dcc_mcp_core.scan_skill_paths(extra_paths=["/nonexistent"])
        assert result == []
