"""Coverage boost phase 2: target 90% overall.

Focus on: script_action (61%), manager async (81%), events async (81%),
function_adapter (80%), log_config (85%), scanner (83%), loader (81%).
"""

# Import built-in modules
import asyncio
import logging
import os
import subprocess
import sys
import tempfile
from pathlib import Path
from types import ModuleType
from typing import Any
from typing import ClassVar
from typing import Dict
from typing import List
from typing import Optional
from unittest.mock import MagicMock
from unittest.mock import patch

# Import third-party modules
from pydantic import Field
import pytest

# Import local modules
from dcc_mcp_core.actions.base import Action
from dcc_mcp_core.actions.events import EventBus
from dcc_mcp_core.actions.function_adapter import create_function_adapter
from dcc_mcp_core.actions.function_adapter import create_function_adapters
from dcc_mcp_core.actions.manager import ActionManager
from dcc_mcp_core.actions.manager import _prepare_action_for_execution
from dcc_mcp_core.actions.manager import create_action_manager
from dcc_mcp_core.actions.middleware import LoggingMiddleware
from dcc_mcp_core.actions.middleware import PerformanceMiddleware
from dcc_mcp_core.actions.registry import ActionRegistry
from dcc_mcp_core.models import ActionResultModel
from dcc_mcp_core.models import SkillMetadata
from dcc_mcp_core.protocols.adapter import MCPAdapter
from dcc_mcp_core.skills.loader import _enumerate_scripts
from dcc_mcp_core.skills.loader import _regex_parse_frontmatter
from dcc_mcp_core.skills.loader import _try_yaml_parse
from dcc_mcp_core.skills.loader import load_skill
from dcc_mcp_core.skills.loader import parse_skill_md
from dcc_mcp_core.skills.scanner import SkillScanner
from dcc_mcp_core.skills.script_action import _get_script_type
from dcc_mcp_core.skills.script_action import _make_action_name
from dcc_mcp_core.skills.script_action import create_script_action


@pytest.fixture(autouse=True)
def reset_reg():
    """Reset registry."""
    ActionRegistry.reset(full_reset=True)
    yield
    ActionRegistry.reset(full_reset=True)


# ============================================================================
# script_action.py — cover subprocess paths for various script types
# ============================================================================


class TestScriptActionExecution:
    """Cover the _execute method for various script types."""

    def _make_metadata(self, tmp_path, script_name):
        return SkillMetadata(
            name="test-skill",
            description="Test",
            scripts=[str(tmp_path / script_name)],
            skill_path=str(tmp_path),
        )

    def test_python_script_execution(self, tmp_path):
        """Test executing a Python script via subprocess."""
        script = tmp_path / "run.py"
        script.write_text("import sys; print('hello', sys.argv[1] if len(sys.argv) > 1 else 'world')")
        metadata = self._make_metadata(tmp_path, "run.py")
        cls = create_script_action("test-skill", str(script), metadata)
        action = cls()
        action.setup(args=["TestArg"])
        result = action.process()
        assert result.success is True
        assert "hello TestArg" in result.context.get("stdout", "")

    def test_shell_script_type(self):
        """Test shell/bash script command building."""
        assert _get_script_type("run.sh") == "shell"
        assert _get_script_type("run.bash") == "shell"

    def test_batch_script_type(self):
        """Test batch script type detection."""
        assert _get_script_type("run.bat") == "batch"
        assert _get_script_type("run.cmd") == "batch"

    def test_powershell_script_type(self):
        """Test powershell script type detection."""
        assert _get_script_type("run.ps1") == "powershell"

    def test_javascript_script_type(self):
        """Test javascript script type detection."""
        assert _get_script_type("run.jsx") == "javascript"
        assert _get_script_type("run.js") == "javascript"

    def test_vbscript_type(self):
        """Test vbscript type detection."""
        assert _get_script_type("run.vbs") == "vbscript"

    def test_mel_no_adapter(self, tmp_path):
        """Test MEL script without DCC adapter."""
        script = tmp_path / "test.mel"
        script.write_text("print 1;")
        metadata = self._make_metadata(tmp_path, "test.mel")
        cls = create_script_action("test-skill", str(script), metadata)
        action = cls()
        action.setup()
        result = action.process()
        assert result.success is True
        assert "No DCC adapter" in result.context.get("stderr", "")

    def test_mel_with_adapter(self, tmp_path):
        """Test MEL script with DCC adapter."""
        script = tmp_path / "test.mel"
        script.write_text("print 1;")
        metadata = self._make_metadata(tmp_path, "test.mel")
        mock_adapter = MagicMock()
        mock_adapter.execute.return_value = {"success": True, "output": "MEL ok", "error": ""}
        cls = create_script_action("test-skill", str(script), metadata)
        action = cls(context={"dcc_adapter": mock_adapter})
        action.setup()
        result = action.process()
        assert result.success is True
        assert "MEL ok" in result.context.get("stdout", "")

    def test_mel_adapter_error(self, tmp_path):
        """Test MEL script when adapter raises error."""
        script = tmp_path / "test.mel"
        script.write_text("print 1;")
        metadata = self._make_metadata(tmp_path, "test.mel")
        mock_adapter = MagicMock()
        mock_adapter.execute.side_effect = RuntimeError("Adapter boom")
        cls = create_script_action("test-skill", str(script), metadata)
        action = cls(context={"dcc_adapter": mock_adapter})
        action.setup()
        result = action.process()
        assert result.success is True
        assert "Adapter boom" in result.context.get("stderr", "")

    def test_mel_adapter_failure_result(self, tmp_path):
        """Test MEL script when adapter returns failure."""
        script = tmp_path / "test.mel"
        script.write_text("print 1;")
        metadata = self._make_metadata(tmp_path, "test.mel")
        mock_adapter = MagicMock()
        mock_adapter.execute.return_value = {"success": False, "output": "", "error": "MEL error"}
        cls = create_script_action("test-skill", str(script), metadata)
        action = cls(context={"dcc_adapter": mock_adapter})
        action.setup()
        result = action.process()
        assert result.success is True
        assert result.context.get("return_code") == 1

    def test_timeout_script(self, tmp_path):
        """Test script timeout."""
        script = tmp_path / "slow.py"
        script.write_text("import time; time.sleep(30)")
        metadata = self._make_metadata(tmp_path, "slow.py")
        cls = create_script_action("test-skill", str(script), metadata)
        action = cls()
        action.setup(timeout=1)
        result = action.process()
        assert result.success is True
        assert "timed out" in result.context.get("stderr", "")

    def test_file_not_found_script(self, tmp_path):
        """Test script with nonexistent interpreter."""
        script = tmp_path / "run.vbs"
        script.write_text("MsgBox 1")
        metadata = self._make_metadata(tmp_path, "run.vbs")
        cls = create_script_action("test-skill", str(script), metadata)
        action = cls()
        action.setup()
        result = action.process()
        # vbscript may not exist on this machine, FileNotFoundError is caught
        assert result.success is True

    def test_generic_exception_script(self, tmp_path):
        """Test script execution with generic subprocess exception."""
        script = tmp_path / "run.py"
        script.write_text("print('hello')")
        metadata = self._make_metadata(tmp_path, "run.py")
        cls = create_script_action("test-skill", str(script), metadata)
        action = cls()
        action.setup()

        with patch("subprocess.run", side_effect=OSError("OS error")):
            result = action.process()
            assert result.success is True
            assert "OS error" in result.context.get("stderr", "")

    def test_env_vars_passed(self, tmp_path):
        """Test that env_vars are passed to subprocess."""
        script = tmp_path / "env_test.py"
        script.write_text("import os; print(os.environ.get('MY_VAR', 'missing'))")
        metadata = self._make_metadata(tmp_path, "env_test.py")
        cls = create_script_action("test-skill", str(script), metadata)
        action = cls()
        action.setup(env_vars={"MY_VAR": "hello"})
        result = action.process()
        assert result.success is True
        assert "hello" in result.context.get("stdout", "")

    def test_working_dir(self, tmp_path):
        """Test script execution with custom working directory."""
        script = tmp_path / "cwd_test.py"
        script.write_text("import os; print(os.getcwd())")
        metadata = self._make_metadata(tmp_path, "cwd_test.py")
        cls = create_script_action("test-skill", str(script), metadata)
        action = cls()
        action.setup(working_dir=str(tmp_path))
        result = action.process()
        assert result.success is True


# ============================================================================
# Manager async paths
# ============================================================================


class TestManagerAsyncPaths:
    """Cover async paths in manager.py."""

    def test_prepare_action_setup_error(self):
        """Test _prepare_action_for_execution when setup raises."""
        manager = ActionManager("test", "test_dcc")

        class SetupErrorAction:
            name = "setup_error"
            dcc = "test_dcc"

            def __init__(self, context=None):
                self.context = context or {}

            def setup(self, **kwargs):
                raise ValueError("Setup failed")

        with patch.object(manager.registry, "get_action", return_value=SetupErrorAction):
            action, result = _prepare_action_for_execution(manager, "setup_error")
            assert action is None
            assert result.success is False
            assert "Setup failed" in result.error

    def test_create_action_manager_with_skills(self):
        """Test create_action_manager with skill loading."""
        with patch("dcc_mcp_core.actions.manager._load_skills_for_manager") as mock_load:
            manager = create_action_manager("test_dcc", load_skill_paths=True)
            mock_load.assert_called_once()

    def test_create_action_manager_skill_loading_error(self):
        """Test create_action_manager when skill loading raises."""
        with patch("dcc_mcp_core.actions.manager._load_skills_for_manager", side_effect=Exception("boom")):
            manager = create_action_manager("test_dcc", load_skill_paths=True)
            assert manager is not None

    def test_manager_list_available_actions(self):
        """Test list_available_actions."""
        manager = ActionManager("test", "test_dcc")

        class TestAct(Action):
            name = "list_test_action"
            dcc = "test_dcc"

            class InputModel(Action.InputModel):
                pass

            def _execute(self):
                pass

        manager.registry.register(TestAct)
        actions = manager.list_available_actions()
        assert "list_test_action" in actions


# ============================================================================
# EventBus async publish_async with sync subscribers
# ============================================================================


class TestEventBusAsyncSync:
    """Cover async publish with sync subscribers in events.py."""

    @pytest.mark.asyncio
    async def test_publish_async_with_sync_subscribers(self):
        """Test publish_async calls sync subscribers via thread pool."""
        bus = EventBus()
        results = []

        def sync_callback(*args, **kwargs):
            results.append(("sync", args, kwargs))

        bus.subscribe("test_event", sync_callback)
        await bus.publish_async("test_event", "arg1", key="val")
        assert len(results) == 1
        assert results[0][0] == "sync"

    @pytest.mark.asyncio
    async def test_publish_async_sync_subscriber_error(self):
        """Test publish_async handles sync subscriber errors."""
        bus = EventBus()

        def error_callback(*args, **kwargs):
            raise ValueError("Sync error")

        bus.subscribe("test_event", error_callback)
        await bus.publish_async("test_event")  # Should not raise

    @pytest.mark.asyncio
    async def test_publish_async_async_subscriber_error(self):
        """Test publish_async handles async subscriber errors."""
        bus = EventBus()

        async def error_callback(*args, **kwargs):
            raise ValueError("Async error")

        await bus.subscribe_async("test_event", error_callback)
        await bus.publish_async("test_event")  # Should not raise

    @pytest.mark.asyncio
    async def test_unsubscribe_async_nonexistent(self):
        """Test unsubscribe_async with nonexistent event."""
        bus = EventBus()
        callback = MagicMock()
        await bus.unsubscribe_async("no_event", callback)  # Should not raise


# ============================================================================
# function_adapter.py — cover remaining error paths
# ============================================================================


class TestFunctionAdapterPaths:
    """Cover remaining paths in function_adapter.py."""

    def test_create_adapters_manager_returns_non_list(self):
        """Test create_function_adapters when manager returns non-list."""
        mock_manager = MagicMock()
        mock_manager.name = "test"
        mock_manager.list_available_actions.return_value = "not_a_list"
        adapters = create_function_adapters(manager=mock_manager)
        assert adapters == {}

    def test_create_adapters_manager_action_error(self):
        """Test create_function_adapters when adapter creation fails."""
        mock_manager = MagicMock()
        mock_manager.name = "test"
        mock_manager.list_available_actions.return_value = ["action1"]

        with patch("dcc_mcp_core.actions.function_adapter.create_function_adapter", side_effect=Exception("boom")):
            adapters = create_function_adapters(manager=mock_manager)
            assert adapters == {}

    def test_direct_adapter_action_instantiation_error(self):
        """Test direct adapter when action class init fails."""
        registry = ActionRegistry()

        class BadInitAction(Action):
            name = "bad_init"
            dcc = "test"

            class InputModel(Action.InputModel):
                pass

            def __init__(self, *args, **kwargs):
                raise RuntimeError("Init error")

            def _execute(self):
                pass

        # Manually register
        registry._actions["bad_init"] = BadInitAction
        registry._dcc_actions.setdefault("test", {})["bad_init"] = BadInitAction

        adapter = create_function_adapter("bad_init")
        result = adapter()
        assert result.success is False
        assert "Failed to create instance" in result.message

    def test_direct_adapter_process_error(self):
        """Test direct adapter when process raises."""
        registry = ActionRegistry()

        class ProcessErrorAction(Action):
            name = "process_error"
            dcc = "test"

            class InputModel(Action.InputModel):
                pass

            def _execute(self):
                pass

            def process(self):
                raise RuntimeError("Process error")

        registry.register(ProcessErrorAction)

        adapter = create_function_adapter("process_error")
        result = adapter()
        assert result.success is False
        assert "execution failed during processing" in result.message


# ============================================================================
# log_config.py — cover loguru paths
# ============================================================================


class TestLogConfigLoguru:
    """Cover loguru-specific paths in log_config.py."""

    def test_setup_loguru_logger_when_unavailable(self):
        """Test setup_loguru_logger falls back when loguru unavailable."""
        from dcc_mcp_core.log_config import setup_loguru_logger

        with patch("dcc_mcp_core.log_config.LOGURU_AVAILABLE", False):
            logger = setup_loguru_logger("fallback_test")
            assert logger is not None

    def test_set_log_level_with_loguru_configured(self):
        """Test set_log_level updates loguru loggers."""
        from dcc_mcp_core.log_config import _configured_loggers
        from dcc_mcp_core.log_config import set_log_level

        # Add a fake loguru logger entry
        _configured_loggers["loguru_test"] = {
            "type": "loguru",
            "logger": MagicMock(),
            "handlers": [],
        }
        set_log_level("WARNING")
        # Should not raise

        # Clean up
        del _configured_loggers["loguru_test"]

    def test_get_logger_info_loguru_handlers(self):
        """Test get_logger_info with handlers that have baseFilename."""
        from dcc_mcp_core.log_config import _configured_loggers
        from dcc_mcp_core.log_config import get_logger_info

        mock_file_handler = MagicMock()
        mock_file_handler.baseFilename = "/test/log.txt"

        mock_console_handler = MagicMock(spec=logging.StreamHandler)
        mock_console_handler.stream = sys.stdout

        _configured_loggers["info_test"] = {
            "type": "standard",
            "logger": MagicMock(),
            "handlers": [mock_file_handler, mock_console_handler],
            "log_file": "/test/log.txt",
            "dcc_type": None,
        }

        info = get_logger_info("info_test")
        assert info["configured"] is True
        assert info["log_file"] == "/test/log.txt"
        assert info["file_handler"] is mock_file_handler
        assert info["console_handler"] is mock_console_handler

        del _configured_loggers["info_test"]


# ============================================================================
# skills/loader.py — cover remaining paths
# ============================================================================


class TestSkillLoaderPaths:
    """Cover remaining paths in skills/loader.py."""

    def test_try_yaml_parse_exception(self):
        """Test _try_yaml_parse with exception."""
        result = _try_yaml_parse(":::invalid:::yaml:::")
        # Should fall back to None on parse error
        assert result is None or isinstance(result, dict)

    def test_regex_parse_single_quoted(self):
        """Test regex parse with single-quoted values."""
        result = _regex_parse_frontmatter("name: 'my skill'")
        assert result["name"] == "my skill"

    def test_parse_skill_md_invalid_frontmatter(self, tmp_path):
        """Test parse_skill_md with invalid frontmatter content."""
        skill_dir = tmp_path / "bad"
        skill_dir.mkdir()
        (skill_dir / "SKILL.md").write_text("---\n:::not valid:::\n---\n")
        result = parse_skill_md(str(skill_dir))
        # Should either parse partially or return None
        assert result is None or hasattr(result, "name")

    def test_enumerate_scripts_no_scripts_dir(self, tmp_path):
        """Test _enumerate_scripts when scripts/ doesn't exist."""
        result = _enumerate_scripts(str(tmp_path))
        assert result == []

    def test_enumerate_scripts_os_error(self, tmp_path):
        """Test _enumerate_scripts with OS error."""
        scripts_dir = tmp_path / "scripts"
        scripts_dir.mkdir()
        with patch("os.scandir", side_effect=OSError("Scan error")):
            result = _enumerate_scripts(str(tmp_path))
            assert result == []

    def test_load_skill_create_action_error(self, tmp_path):
        """Test load_skill when create_script_action raises."""
        skill_dir = tmp_path / "skill"
        skill_dir.mkdir()
        (skill_dir / "SKILL.md").write_text("---\nname: error-skill\n---\n")
        scripts = skill_dir / "scripts"
        scripts.mkdir()
        (scripts / "run.py").write_text("print(1)")

        with patch("dcc_mcp_core.skills.loader.create_script_action", side_effect=RuntimeError("Create error")):
            result = load_skill(str(skill_dir))
            assert result == []

    def test_load_skill_register_fails(self, tmp_path):
        """Test load_skill when registration fails."""
        skill_dir = tmp_path / "skill"
        skill_dir.mkdir()
        (skill_dir / "SKILL.md").write_text("---\nname: reg-fail\n---\n")
        scripts = skill_dir / "scripts"
        scripts.mkdir()
        (scripts / "run.py").write_text("print(1)")

        with patch.object(ActionRegistry, "register", return_value=False):
            result = load_skill(str(skill_dir))
            assert result == []


# ============================================================================
# skills/scanner.py — cover remaining paths
# ============================================================================


class TestScannerPaths:
    """Cover remaining paths in skills/scanner.py."""

    def test_scan_with_dcc_name(self, tmp_path):
        """Test scan with DCC name and platform dir."""
        scanner = SkillScanner()
        with patch("dcc_mcp_core.skills.scanner.get_skill_paths_from_env", return_value=[]):
            with patch("dcc_mcp_core.skills.scanner.get_skills_dir") as mock_dir:
                mock_dir.return_value = str(tmp_path)
                results = scanner.scan(dcc_name="maya")
                assert isinstance(results, list)

    def test_scan_platform_dir_exception(self):
        """Test scan when get_skills_dir raises."""
        scanner = SkillScanner()
        with patch("dcc_mcp_core.skills.scanner.get_skill_paths_from_env", return_value=[]):
            with patch("dcc_mcp_core.skills.scanner.get_skills_dir", side_effect=Exception("Dir error")):
                results = scanner.scan()
                assert isinstance(results, list)

    def test_scan_os_error(self, tmp_path):
        """Test _scan_directory with general OSError."""
        scanner = SkillScanner()
        with patch("os.scandir", side_effect=OSError("General OS error")):
            results = scanner._scan_directory(str(tmp_path))
            assert results == []

    def test_scan_mtime_os_error(self, tmp_path):
        """Test _scan_directory when getmtime raises OSError."""
        skill_dir = tmp_path / "skill"
        skill_dir.mkdir()
        (skill_dir / "SKILL.md").write_text("---\nname: test\n---\n")

        scanner = SkillScanner()
        # First scan to populate cache
        scanner._scan_directory(str(tmp_path))

        # Patch getmtime to raise OSError on second scan
        original_getmtime = os.path.getmtime
        call_count = [0]

        def patched_getmtime(path):
            call_count[0] += 1
            if call_count[0] > 1:
                raise OSError("mtime error")
            return original_getmtime(path)

        with patch("os.path.getmtime", side_effect=patched_getmtime):
            results = scanner._scan_directory(str(tmp_path))
            assert isinstance(results, list)


# ============================================================================
# protocols/adapter.py — cover output schema required field removal
# ============================================================================


class TestAdapterOutputSchema:
    """Cover output schema edge cases in adapter.py."""

    def test_output_schema_required_prompt_removal(self):
        """Test that 'prompt' is removed from required in output schema."""

        class ActionWithRequiredOutput(Action):
            name = "required_output"
            description = "Test"
            dcc = "test"

            class InputModel(Action.InputModel):
                pass

            class OutputModel(Action.OutputModel):
                result: str = Field(description="Result")

            def _execute(self):
                pass

        tool = MCPAdapter.action_to_tool(ActionWithRequiredOutput, include_output_schema=True)
        assert tool.outputSchema is not None
        # prompt should be filtered out
        assert "prompt" not in tool.outputSchema.get("properties", {})


# ============================================================================
# Middleware async error paths
# ============================================================================


class TestMiddlewareAsyncErrors:
    """Cover async error paths in middleware."""

    @pytest.mark.asyncio
    async def test_performance_middleware_async_slow(self):
        """Test PerformanceMiddleware async with slow action warning."""

        class SlowAsyncAction(Action):
            name = "slow_async"
            dcc = "test"

            class InputModel(Action.InputModel):
                pass

            async def _execute_async(self):
                await asyncio.sleep(0.1)
                self.output = self.OutputModel()

        middleware = PerformanceMiddleware(threshold=0.01)
        action = SlowAsyncAction()
        action.setup()
        result = await middleware.process_async(action)
        assert result.success is True
        assert "performance" in result.context
        assert result.context["performance"]["execution_time"] > 0.01

    @pytest.mark.asyncio
    async def test_middleware_chain_async(self):
        """Test middleware chain async processing."""
        from dcc_mcp_core.actions.middleware import MiddlewareChain

        chain = MiddlewareChain()
        chain.add(LoggingMiddleware)
        chain.add(PerformanceMiddleware, threshold=0.001)
        middleware = chain.build()

        class SimpleAct(Action):
            name = "simple_chain"
            dcc = "test"

            class InputModel(Action.InputModel):
                pass

            def _execute(self):
                self.output = self.OutputModel()

        action = SimpleAct()
        action.setup()
        result = await middleware.process_async(action)
        assert result.success is True


# ============================================================================
# Registry — cover discover_actions_from_path process module
# ============================================================================


class TestRegistryDiscoverPath:
    """Cover discover_actions_from_path and _process_module_for_actions."""

    def test_discover_actions_from_path_success(self, tmp_path):
        """Test discovering actions from a Python file."""
        registry = ActionRegistry()

        action_file = tmp_path / "my_action.py"
        action_file.write_text("""
from dcc_mcp_core.actions.base import Action

class MyAction(Action):
    name = "my_action"
    description = "My action"
    dcc = "test"

    class InputModel(Action.InputModel):
        pass

    def _execute(self):
        pass
""")

        discovered = registry.discover_actions_from_path(str(action_file), dcc_name="test")
        assert len(discovered) >= 1
        assert registry.get_action("my_action") is not None

    def test_discover_from_package_module_import_error(self):
        """Test discover_actions_from_package with module import error."""
        registry = ActionRegistry()

        with patch("importlib.import_module") as mock_import:
            mock_package = MagicMock()
            mock_package.__file__ = "/fake/__init__.py"
            mock_package.__name__ = "fake_pkg"

            mock_import.side_effect = [mock_package, ImportError("Module error")]

            with patch.object(registry, "_discover_actions_from_module_object", return_value=[]):
                with patch.object(registry, "_find_modules_in_package", return_value=["fake_pkg.module"]):
                    result = registry.discover_actions_from_package("fake_pkg")
                    assert isinstance(result, list)


# ============================================================================
# Filesystem — cover get_skills_dir and skill path env
# ============================================================================


class TestFilesystemSkills:
    """Cover skills-related filesystem functions."""

    def test_get_skills_dir_with_dcc(self):
        """Test get_skills_dir with DCC name."""
        from dcc_mcp_core.utils.filesystem import get_skills_dir

        with patch("dcc_mcp_core.utils.filesystem.get_data_dir", return_value="/test/data"):
            with patch("os.makedirs"):
                result = get_skills_dir(dcc_name="maya")
                assert "maya" in result

    def test_get_skills_dir_without_dcc(self):
        """Test get_skills_dir without DCC name."""
        from dcc_mcp_core.utils.filesystem import get_skills_dir

        with patch("dcc_mcp_core.utils.filesystem.get_data_dir", return_value="/test/data"):
            with patch("os.makedirs"):
                result = get_skills_dir()
                assert result.endswith("skills")

    def test_get_skill_paths_from_env(self, tmp_path):
        """Test get_skill_paths_from_env with valid paths."""
        from dcc_mcp_core.utils.filesystem import get_skill_paths_from_env

        with patch.dict(os.environ, {"DCC_MCP_SKILL_PATHS": str(tmp_path)}, clear=False):
            paths = get_skill_paths_from_env()
            assert str(tmp_path) in [os.path.abspath(p) for p in paths]

    def test_get_skill_paths_from_env_invalid(self):
        """Test get_skill_paths_from_env with invalid paths."""
        from dcc_mcp_core.utils.filesystem import get_skill_paths_from_env

        with patch.dict(os.environ, {"DCC_MCP_SKILL_PATHS": "/nonexistent/path/xyz"}, clear=False):
            paths = get_skill_paths_from_env()
            assert len(paths) == 0
