"""Tests for Skill loader (parse_skill_md and load_skill)."""
# Import built-in modules
import os

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core.actions.registry import ActionRegistry
from dcc_mcp_core.skills.loader import load_skill
from dcc_mcp_core.skills.loader import parse_skill_md

EXAMPLE_SKILLS_DIR = os.path.join(os.path.dirname(__file__), "example_skills", "test_skill")


class TestParseSkillMd:
    """Test cases for parse_skill_md."""

    def test_parse_example_skill(self):
        """Parse the example test skill SKILL.md."""
        metadata = parse_skill_md(EXAMPLE_SKILLS_DIR)
        assert metadata is not None
        assert metadata.name == "test-skill"
        assert "test" in metadata.description.lower()
        assert "Bash" in metadata.tools
        assert "Read" in metadata.tools
        assert "test" in metadata.tags

    def test_parse_discovers_scripts(self):
        """Parser discovers scripts in scripts/ directory."""
        metadata = parse_skill_md(EXAMPLE_SKILLS_DIR)
        assert metadata is not None
        assert len(metadata.scripts) >= 1
        script_names = [os.path.basename(s) for s in metadata.scripts]
        assert "hello.py" in script_names

    def test_parse_nonexistent_dir(self, tmp_path):
        """Parser returns None for nonexistent directory."""
        result = parse_skill_md(str(tmp_path / "nonexistent"))
        assert result is None

    def test_parse_no_skill_md(self, tmp_path):
        """Parser returns None when SKILL.md is missing."""
        result = parse_skill_md(str(tmp_path))
        assert result is None

    def test_parse_no_frontmatter(self, tmp_path):
        """Parser returns None when SKILL.md has no frontmatter."""
        (tmp_path / "SKILL.md").write_text("# No frontmatter here\n")
        result = parse_skill_md(str(tmp_path))
        assert result is None

    def test_parse_minimal_frontmatter(self, tmp_path):
        """Parser handles minimal frontmatter (just name)."""
        (tmp_path / "SKILL.md").write_text("---\nname: minimal\n---\n# Minimal\n")
        metadata = parse_skill_md(str(tmp_path))
        assert metadata is not None
        assert metadata.name == "minimal"
        assert metadata.tools == []
        assert metadata.tags == []

    def test_parse_fallback_name_from_directory(self, tmp_path):
        """Parser uses directory name when name is missing from frontmatter."""
        (tmp_path / "SKILL.md").write_text("---\ndescription: no name here\n---\n")
        metadata = parse_skill_md(str(tmp_path))
        assert metadata is not None
        assert metadata.name == tmp_path.name

    def test_parse_with_scripts_directory(self, tmp_path):
        """Parser enumerates scripts from scripts/ directory."""
        (tmp_path / "SKILL.md").write_text("---\nname: with-scripts\n---\n")
        scripts = tmp_path / "scripts"
        scripts.mkdir()
        (scripts / "run.py").write_text("print(1)")
        (scripts / "build.sh").write_text("echo 1")
        (scripts / "README.md").write_text("not a script")  # Should be ignored

        metadata = parse_skill_md(str(tmp_path))
        assert metadata is not None
        assert len(metadata.scripts) == 2
        names = {os.path.basename(s) for s in metadata.scripts}
        assert names == {"run.py", "build.sh"}

    def test_parse_inline_list(self, tmp_path):
        """Parser handles inline YAML lists."""
        (tmp_path / "SKILL.md").write_text(
            '---\nname: list-test\ntools: ["Bash", "Read", "Write"]\ntags: ["a", "b"]\n---\n'
        )
        metadata = parse_skill_md(str(tmp_path))
        assert metadata is not None
        assert metadata.tools == ["Bash", "Read", "Write"]
        assert metadata.tags == ["a", "b"]


class TestLoadSkill:
    """Test cases for load_skill."""

    def setup_method(self):
        """Reset registry before each test."""
        ActionRegistry.reset(full_reset=True)

    def test_load_example_skill(self):
        """Load the example test skill and register its actions."""
        registry = ActionRegistry()
        actions = load_skill(EXAMPLE_SKILLS_DIR, registry=registry)
        assert len(actions) >= 1
        # Verify actions are registered
        for action_cls in actions:
            assert registry.get_action(action_cls.name) is not None

    def test_load_skill_action_names(self):
        """Loaded actions have correct naming convention."""
        registry = ActionRegistry()
        actions = load_skill(EXAMPLE_SKILLS_DIR, registry=registry)
        for action_cls in actions:
            assert "test_skill" in action_cls.name or "test-skill" in action_cls.name

    def test_load_nonexistent_skill(self):
        """load_skill returns empty list for nonexistent path."""
        result = load_skill("/nonexistent/path/xyz")
        assert result == []

    def test_load_skill_with_dcc_override(self):
        """load_skill applies DCC name override."""
        registry = ActionRegistry()
        actions = load_skill(EXAMPLE_SKILLS_DIR, registry=registry, dcc_name="maya")
        for action_cls in actions:
            assert action_cls.dcc == "maya"

    def test_load_skill_registers_in_dcc_registry(self):
        """Loaded actions appear in DCC-specific registry."""
        registry = ActionRegistry()
        actions = load_skill(EXAMPLE_SKILLS_DIR, registry=registry, dcc_name="test_dcc")
        dcc_actions = registry.get_actions_by_dcc("test_dcc")
        assert len(dcc_actions) >= 1
