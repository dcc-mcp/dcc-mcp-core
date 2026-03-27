"""Tests for ScriptAction factory."""
# Import built-in modules
import os
import sys

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core.actions.base import Action
from dcc_mcp_core.actions.registry import ActionRegistry
from dcc_mcp_core.models import SkillMetadata
from dcc_mcp_core.skills.script_action import _get_script_type
from dcc_mcp_core.skills.script_action import _make_action_name
from dcc_mcp_core.skills.script_action import create_script_action

EXAMPLE_SCRIPT = os.path.join(
    os.path.dirname(__file__), "example_skills", "test_skill", "scripts", "hello.py"
)


@pytest.fixture
def sample_metadata():
    """Create a sample SkillMetadata for testing."""
    return SkillMetadata(
        name="test-skill",
        description="A test skill",
        tools=["Bash"],
        tags=["test"],
        scripts=[EXAMPLE_SCRIPT],
        skill_path=os.path.dirname(EXAMPLE_SCRIPT),
    )


class TestMakeActionName:
    """Test action name generation."""

    def test_basic_name(self):
        assert _make_action_name("my-skill", "/path/to/run.py") == "my_skill__run"

    def test_hyphen_normalization(self):
        assert _make_action_name("maya-geometry", "/path/create-sphere.py") == "maya_geometry__create_sphere"

    def test_space_normalization(self):
        assert _make_action_name("my skill", "/path/my script.py") == "my_skill__my_script"


class TestGetScriptType:
    """Test script type detection."""

    def test_python(self):
        assert _get_script_type("run.py") == "python"

    def test_mel(self):
        assert _get_script_type("create.mel") == "mel"

    def test_maxscript(self):
        assert _get_script_type("tool.ms") == "maxscript"

    def test_shell(self):
        assert _get_script_type("build.sh") == "shell"

    def test_batch(self):
        assert _get_script_type("run.bat") == "batch"

    def test_unknown(self):
        assert _get_script_type("file.xyz") == "unknown"


class TestCreateScriptAction:
    """Test ScriptAction factory."""

    def test_creates_action_subclass(self, sample_metadata):
        """Factory creates a valid Action subclass."""
        cls = create_script_action("test-skill", EXAMPLE_SCRIPT, sample_metadata)
        assert issubclass(cls, Action)

    def test_action_has_correct_name(self, sample_metadata):
        """Generated action has expected naming convention."""
        cls = create_script_action("test-skill", EXAMPLE_SCRIPT, sample_metadata)
        assert cls.name == "test_skill__hello"

    def test_action_has_description(self, sample_metadata):
        """Generated action has a description."""
        cls = create_script_action("test-skill", EXAMPLE_SCRIPT, sample_metadata)
        assert cls.description
        assert "hello.py" in cls.description

    def test_action_has_tags(self, sample_metadata):
        """Generated action includes skill tags."""
        cls = create_script_action("test-skill", EXAMPLE_SCRIPT, sample_metadata)
        assert "test" in cls.tags
        assert "skill" in cls.tags
        assert "python" in cls.tags

    def test_action_dcc_override(self, sample_metadata):
        """DCC name can be overridden."""
        cls = create_script_action("test-skill", EXAMPLE_SCRIPT, sample_metadata, dcc_name="maya")
        assert cls.dcc == "maya"

    def test_action_not_abstract(self, sample_metadata):
        """Generated action is not abstract."""
        cls = create_script_action("test-skill", EXAMPLE_SCRIPT, sample_metadata)
        assert not cls.abstract

    def test_action_implements_execute(self, sample_metadata):
        """Generated action implements _execute (required for registration)."""
        cls = create_script_action("test-skill", EXAMPLE_SCRIPT, sample_metadata)
        assert hasattr(cls, "_execute")
        assert cls._execute is not Action._execute

    def test_action_registers_successfully(self, sample_metadata):
        """Generated action can be registered in ActionRegistry."""
        ActionRegistry.reset(full_reset=True)
        registry = ActionRegistry()
        cls = create_script_action("test-skill", EXAMPLE_SCRIPT, sample_metadata)
        assert registry.register(cls) is True

    def test_action_input_model(self, sample_metadata):
        """Generated action has InputModel with args field."""
        cls = create_script_action("test-skill", EXAMPLE_SCRIPT, sample_metadata)
        assert hasattr(cls, "InputModel")
        input_instance = cls.InputModel()
        assert hasattr(input_instance, "args")
        assert hasattr(input_instance, "timeout")

    def test_action_execution_python(self, sample_metadata):
        """Generated action can execute a Python script."""
        cls = create_script_action("test-skill", EXAMPLE_SCRIPT, sample_metadata)
        action = cls()
        action.setup(args=["TestUser"])
        result = action.process()
        assert result.success
        assert "Hello, TestUser!" in result.context.get("stdout", "")

    def test_action_execution_default_args(self, sample_metadata):
        """Generated action works with default arguments."""
        cls = create_script_action("test-skill", EXAMPLE_SCRIPT, sample_metadata)
        action = cls()
        action.setup()
        result = action.process()
        assert result.success
        assert "Hello, World!" in result.context.get("stdout", "")
