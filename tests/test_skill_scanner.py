"""Tests for SkillScanner."""
# Import built-in modules
import os
import tempfile

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core.skills.scanner import SkillScanner
from dcc_mcp_core.skills.scanner import scan_skill_paths


@pytest.fixture
def skill_dirs(tmp_path):
    """Create temporary skill directories for testing."""
    # Create two valid skills
    skill1 = tmp_path / "skill-one"
    skill1.mkdir()
    (skill1 / "SKILL.md").write_text(
        "---\nname: skill-one\ndescription: First skill\n---\n# Skill One\n"
    )
    (skill1 / "scripts").mkdir()
    (skill1 / "scripts" / "run.py").write_text("print('hello')")

    skill2 = tmp_path / "skill-two"
    skill2.mkdir()
    (skill2 / "SKILL.md").write_text(
        "---\nname: skill-two\ndescription: Second skill\n---\n# Skill Two\n"
    )

    # Create a directory without SKILL.md (should be ignored)
    not_skill = tmp_path / "not-a-skill"
    not_skill.mkdir()
    (not_skill / "README.md").write_text("Not a skill")

    return tmp_path


class TestSkillScanner:
    """Test cases for SkillScanner."""

    def test_scan_finds_skills(self, skill_dirs):
        """Scanner finds directories with SKILL.md."""
        scanner = SkillScanner()
        results = scanner.scan(extra_paths=[str(skill_dirs)])
        assert len(results) == 2
        basenames = {os.path.basename(p) for p in results}
        assert basenames == {"skill-one", "skill-two"}

    def test_scan_ignores_non_skills(self, skill_dirs):
        """Scanner ignores directories without SKILL.md."""
        scanner = SkillScanner()
        results = scanner.scan(extra_paths=[str(skill_dirs)])
        basenames = {os.path.basename(p) for p in results}
        assert "not-a-skill" not in basenames

    def test_scan_empty_directory(self, tmp_path):
        """Scanner returns empty list for empty directory."""
        scanner = SkillScanner()
        results = scanner.scan(extra_paths=[str(tmp_path)])
        assert results == []

    def test_scan_nonexistent_path(self):
        """Scanner handles nonexistent paths gracefully."""
        scanner = SkillScanner()
        results = scanner.scan(extra_paths=["/nonexistent/path/xyz"])
        assert results == []

    def test_scan_caching(self, skill_dirs):
        """Scanner caches results based on mtime."""
        scanner = SkillScanner()
        results1 = scanner.scan(extra_paths=[str(skill_dirs)])
        results2 = scanner.scan(extra_paths=[str(skill_dirs)])
        assert results1 == results2

    def test_scan_force_refresh(self, skill_dirs):
        """Scanner can force refresh cache."""
        scanner = SkillScanner()
        results1 = scanner.scan(extra_paths=[str(skill_dirs)])
        results2 = scanner.scan(extra_paths=[str(skill_dirs)], force_refresh=True)
        assert len(results1) == len(results2)

    def test_discovered_skills_property(self, skill_dirs):
        """discovered_skills property returns last scan results."""
        scanner = SkillScanner()
        scanner.scan(extra_paths=[str(skill_dirs)])
        assert len(scanner.discovered_skills) == 2

    def test_clear_cache(self, skill_dirs):
        """clear_cache empties all cached data."""
        scanner = SkillScanner()
        scanner.scan(extra_paths=[str(skill_dirs)])
        scanner.clear_cache()
        assert scanner.discovered_skills == []

    def test_scan_deduplicates_paths(self, skill_dirs):
        """Scanner deduplicates search paths."""
        scanner = SkillScanner()
        results = scanner.scan(extra_paths=[str(skill_dirs), str(skill_dirs)])
        assert len(results) == 2  # Not doubled


class TestScanSkillPaths:
    """Test the convenience function."""

    def test_scan_skill_paths_convenience(self, skill_dirs):
        """scan_skill_paths finds skills."""
        results = scan_skill_paths(extra_paths=[str(skill_dirs)])
        assert len(results) == 2


class TestSkillScannerWithEnv:
    """Test scanner with environment variables."""

    def test_scan_from_env(self, skill_dirs, monkeypatch):
        """Scanner reads DCC_MCP_SKILL_PATHS environment variable."""
        monkeypatch.setenv("DCC_MCP_SKILL_PATHS", str(skill_dirs))
        scanner = SkillScanner()
        results = scanner.scan()
        assert len(results) == 2

    def test_scan_env_multiple_paths(self, skill_dirs, tmp_path, monkeypatch):
        """Scanner handles multiple paths in environment variable."""
        extra = tmp_path / "extra"
        extra.mkdir()
        (extra / "skill-three").mkdir()
        (extra / "skill-three" / "SKILL.md").write_text("---\nname: skill-three\n---\n")
        env_val = f"{skill_dirs}{os.pathsep}{extra}"
        monkeypatch.setenv("DCC_MCP_SKILL_PATHS", env_val)
        scanner = SkillScanner()
        results = scanner.scan()
        assert len(results) == 3
