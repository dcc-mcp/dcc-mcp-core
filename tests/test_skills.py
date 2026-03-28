"""Tests for SkillScanner, SkillMetadata, and parse_skill_md."""

import os
import tempfile

import dcc_mcp_core


class TestSkillMetadata:
    def test_create_default(self):
        sm = dcc_mcp_core.SkillMetadata(name="test-skill")
        assert sm.name == "test-skill"
        assert sm.description == ""
        assert sm.dcc == "python"
        assert sm.version == "1.0.0"
        assert sm.tags == []
        assert sm.scripts == []
        assert sm.tools == []
        assert sm.skill_path == ""

    def test_create_full(self):
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

    def test_setters(self):
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

    def test_repr(self):
        sm = dcc_mcp_core.SkillMetadata(name="test", dcc="maya")
        r = repr(sm)
        assert "test" in r
        assert "maya" in r


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

    def test_scan_updates_discovered_skills(self):
        scanner = dcc_mcp_core.SkillScanner()
        with tempfile.TemporaryDirectory() as tmpdir:
            skill_dir = os.path.join(tmpdir, "s1")
            os.makedirs(skill_dir)
            with open(os.path.join(skill_dir, "SKILL.md"), "w") as f:
                f.write("---\nname: s1\n---\n")
            scanner.scan(extra_paths=[tmpdir])
            assert len(scanner.discovered_skills) == 1

    def test_scan_multiple_skills(self):
        scanner = dcc_mcp_core.SkillScanner()
        with tempfile.TemporaryDirectory() as tmpdir:
            for name in ["skill-a", "skill-b", "skill-c"]:
                d = os.path.join(tmpdir, name)
                os.makedirs(d)
                with open(os.path.join(d, "SKILL.md"), "w") as f:
                    f.write(f"---\nname: {name}\n---\n")
            result = scanner.scan(extra_paths=[tmpdir])
            assert len(result) == 3

    def test_scan_ignores_non_skill_dirs(self):
        scanner = dcc_mcp_core.SkillScanner()
        with tempfile.TemporaryDirectory() as tmpdir:
            # dir without SKILL.md
            os.makedirs(os.path.join(tmpdir, "not-a-skill"))
            # file (not dir)
            with open(os.path.join(tmpdir, "file.txt"), "w") as f:
                f.write("not a skill")
            result = scanner.scan(extra_paths=[tmpdir])
            assert result == []

    def test_scan_with_dcc_name(self):
        scanner = dcc_mcp_core.SkillScanner()
        with tempfile.TemporaryDirectory() as tmpdir:
            d = os.path.join(tmpdir, "s1")
            os.makedirs(d)
            with open(os.path.join(d, "SKILL.md"), "w") as f:
                f.write("---\nname: s1\n---\n")
            result = scanner.scan(extra_paths=[tmpdir], dcc_name="maya")
            assert len(result) == 1

    def test_scan_force_refresh(self):
        scanner = dcc_mcp_core.SkillScanner()
        with tempfile.TemporaryDirectory() as tmpdir:
            d = os.path.join(tmpdir, "s1")
            os.makedirs(d)
            with open(os.path.join(d, "SKILL.md"), "w") as f:
                f.write("---\nname: s1\n---\n")
            r1 = scanner.scan(extra_paths=[tmpdir])
            r2 = scanner.scan(extra_paths=[tmpdir], force_refresh=True)
            assert r1 == r2

    def test_scan_uses_cache(self):
        scanner = dcc_mcp_core.SkillScanner()
        with tempfile.TemporaryDirectory() as tmpdir:
            d = os.path.join(tmpdir, "s1")
            os.makedirs(d)
            with open(os.path.join(d, "SKILL.md"), "w") as f:
                f.write("---\nname: s1\n---\n")
            r1 = scanner.scan(extra_paths=[tmpdir])
            r2 = scanner.scan(extra_paths=[tmpdir])  # should use cache
            assert r1 == r2

    def test_clear_cache(self):
        scanner = dcc_mcp_core.SkillScanner()
        with tempfile.TemporaryDirectory() as tmpdir:
            d = os.path.join(tmpdir, "s1")
            os.makedirs(d)
            with open(os.path.join(d, "SKILL.md"), "w") as f:
                f.write("---\nname: s1\n---\n")
            scanner.scan(extra_paths=[tmpdir])
            scanner.clear_cache()
            assert scanner.discovered_skills == []


class TestScanSkillPaths:
    def test_scan_nonexistent(self):
        result = dcc_mcp_core.scan_skill_paths(extra_paths=["/nonexistent"])
        assert result == []

    def test_scan_with_skills(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            d = os.path.join(tmpdir, "my-skill")
            os.makedirs(d)
            with open(os.path.join(d, "SKILL.md"), "w") as f:
                f.write("---\nname: my-skill\n---\n")
            result = dcc_mcp_core.scan_skill_paths(extra_paths=[tmpdir])
            assert len(result) == 1

    def test_scan_with_dcc_name(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            d = os.path.join(tmpdir, "s1")
            os.makedirs(d)
            with open(os.path.join(d, "SKILL.md"), "w") as f:
                f.write("---\nname: s1\n---\n")
            result = dcc_mcp_core.scan_skill_paths(
                extra_paths=[tmpdir], dcc_name="blender"
            )
            assert len(result) == 1


class TestParseSkillMd:
    def test_parse_valid(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            with open(os.path.join(tmpdir, "SKILL.md"), "w") as f:
                f.write("---\nname: test-skill\ndcc: maya\ntags:\n  - geo\n---\n# Body\n")
            meta = dcc_mcp_core._core.parse_skill_md(tmpdir)
            assert meta is not None
            assert meta.name == "test-skill"
            assert meta.dcc == "maya"
            assert meta.tags == ["geo"]
            assert meta.skill_path == tmpdir

    def test_parse_with_scripts(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            with open(os.path.join(tmpdir, "SKILL.md"), "w") as f:
                f.write("---\nname: scripted\n---\n")
            scripts_dir = os.path.join(tmpdir, "scripts")
            os.makedirs(scripts_dir)
            with open(os.path.join(scripts_dir, "hello.py"), "w") as f:
                f.write("print('hello')")
            with open(os.path.join(scripts_dir, "run.sh"), "w") as f:
                f.write("echo hello")
            meta = dcc_mcp_core._core.parse_skill_md(tmpdir)
            assert meta is not None
            assert len(meta.scripts) == 2

    def test_parse_no_skill_md(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            result = dcc_mcp_core._core.parse_skill_md(tmpdir)
            assert result is None

    def test_parse_invalid_yaml(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            with open(os.path.join(tmpdir, "SKILL.md"), "w") as f:
                f.write("---\n: invalid yaml [[\n---\n")
            result = dcc_mcp_core._core.parse_skill_md(tmpdir)
            assert result is None

    def test_parse_no_frontmatter(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            with open(os.path.join(tmpdir, "SKILL.md"), "w") as f:
                f.write("# No frontmatter\nJust body text.")
            result = dcc_mcp_core._core.parse_skill_md(tmpdir)
            assert result is None
