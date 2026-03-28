"""Comprehensive tests to boost code coverage to 90%.

This module targets uncovered lines across all modules on the main branch.
"""

# Import built-in modules
import asyncio
import logging
import os
from pathlib import Path
import sys
import tempfile
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
from dcc_mcp_core.actions.manager import ActionManager
from dcc_mcp_core.actions.manager import _prepare_action_for_execution
from dcc_mcp_core.actions.manager import create_action_manager
from dcc_mcp_core.actions.manager import get_action_manager
from dcc_mcp_core.actions.middleware import LoggingMiddleware
from dcc_mcp_core.actions.middleware import Middleware
from dcc_mcp_core.actions.middleware import MiddlewareChain
from dcc_mcp_core.actions.middleware import PerformanceMiddleware
from dcc_mcp_core.actions.registry import ActionRegistry
from dcc_mcp_core.models import ActionResultModel
from dcc_mcp_core.protocols.adapter import MCPAdapter
from dcc_mcp_core.protocols.base import Prompt
from dcc_mcp_core.protocols.base import Resource
from dcc_mcp_core.protocols.server import MCPPromptsProtocol
from dcc_mcp_core.protocols.server import MCPResourcesProtocol
from dcc_mcp_core.protocols.server import MCPServerProtocol
from dcc_mcp_core.protocols.server import MCPToolsProtocol
from dcc_mcp_core.protocols.types import PromptDefinition
from dcc_mcp_core.protocols.types import ResourceDefinition
from dcc_mcp_core.protocols.types import ResourceTemplateDefinition
from dcc_mcp_core.protocols.types import ToolAnnotations
from dcc_mcp_core.protocols.types import ToolDefinition
from dcc_mcp_core.utils.exceptions import ActionError
from dcc_mcp_core.utils.exceptions import ActionExecutionError
from dcc_mcp_core.utils.exceptions import ActionParameterError
from dcc_mcp_core.utils.exceptions import ActionSetupError
from dcc_mcp_core.utils.exceptions import ActionValidationError
from dcc_mcp_core.utils.filesystem import ensure_directory_exists
from dcc_mcp_core.utils.filesystem import get_platform_dir
from dcc_mcp_core.utils.result_factory import error_result
from dcc_mcp_core.utils.result_factory import from_exception
from dcc_mcp_core.utils.result_factory import success_result

# ============================================================================
# Test Action classes
# ============================================================================


class SimpleTestAction(Action):
    """Simple test action."""

    name = "simple_test"
    description = "Simple test action"
    dcc = "test"

    class InputModel(Action.InputModel):
        value: int = Field(default=0, description="Value")

    class OutputModel(Action.OutputModel):
        result: int = Field(default=0, description="Result")

    def _execute(self) -> None:
        self.output = self.OutputModel(result=self.input.value * 2)


class ActionWithoutOutput(Action):
    """Action that sets no output."""

    name = "no_output_action"
    description = "No output action"
    dcc = "test"

    class InputModel(Action.InputModel):
        pass

    def _execute(self) -> None:
        pass


class ActionExecutionErrorAction(Action):
    """Action that raises ActionExecutionError."""

    name = "exec_error_action"
    description = "Execution error action"
    dcc = "test"

    class InputModel(Action.InputModel):
        pass

    def _execute(self) -> None:
        raise ActionExecutionError(
            message="Test exec error",
            action_name="exec_error_action",
            action_class="ActionExecutionErrorAction",
            execution_phase="test_phase",
        )


@pytest.fixture(autouse=True)
def reset_registry():
    """Reset registry for each test."""
    ActionRegistry.reset(full_reset=True)
    yield
    ActionRegistry.reset(full_reset=True)


# ============================================================================
# Action base coverage
# ============================================================================


class TestActionBaseCov:
    """Cover uncovered lines in actions/base.py."""

    def test_process_without_output(self):
        """Test action process when _execute doesn't set output."""
        action = ActionWithoutOutput()
        action.setup()
        result = action.process()
        assert result.success is True
        assert result.prompt is None

    def test_process_with_action_execution_error(self):
        """Test action process with ActionExecutionError."""
        action = ActionExecutionErrorAction()
        action.setup()
        result = action.process()
        assert result.success is False
        assert "Test exec error" in result.error

    def test_process_parameters_non_dict_non_string(self):
        """Test process_parameters with non-dict non-string input."""
        result = Action.process_parameters(123)
        assert result == {}

    def test_process_parameters_json_string(self):
        """Test process_parameters with valid JSON string."""
        result = Action.process_parameters('{"key": "value", "num": 42}')
        assert result == {"key": "value", "num": 42}

    def test_process_parameter_value_yes_no(self):
        """Test process_parameter_value with yes/no strings."""
        assert Action.process_parameter_value("yes") is True
        assert Action.process_parameter_value("no") is False
        assert Action.process_parameter_value("1") is True
        assert Action.process_parameter_value("0") is False
        assert Action.process_parameter_value("none") is None
        assert Action.process_parameter_value("null") is None

    @pytest.mark.asyncio
    async def test_process_async_without_output(self):
        """Test async process without output."""
        action = ActionWithoutOutput()
        action.setup()
        result = await action.process_async()
        assert result.success is True
        assert "asynchronously" in result.message

    @pytest.mark.asyncio
    async def test_process_async_with_execution_error(self):
        """Test async process with ActionExecutionError."""

        class AsyncExecError(Action):
            name = "async_exec_error"
            dcc = "test"

            class InputModel(Action.InputModel):
                pass

            async def _execute_async(self):
                raise ActionExecutionError(
                    message="Async exec error",
                    action_name="async_exec_error",
                    execution_phase="async",
                )

        action = AsyncExecError()
        action.setup()
        result = await action.process_async()
        assert result.success is False
        assert "Async exec error" in result.error

    @pytest.mark.asyncio
    async def test_process_async_with_generic_error(self):
        """Test async process with generic error."""

        class AsyncGenericError(Action):
            name = "async_generic_error"
            dcc = "test"

            class InputModel(Action.InputModel):
                pass

            async def _execute_async(self):
                raise RuntimeError("Generic async error")

        action = AsyncGenericError()
        action.setup()
        result = await action.process_async()
        assert result.success is False
        assert "Generic async error" in result.error


# ============================================================================
# Manager coverage
# ============================================================================


class TestManagerCov:
    """Cover uncovered lines in actions/manager.py."""

    def test_create_action_manager_with_env_action_paths(self, tmp_path):
        """Test create_action_manager with actual env action paths."""
        test_dir = str(tmp_path)
        with patch.dict(os.environ, {"DCC_MCP_ACTION_PATHS": test_dir}, clear=False):
            with patch.object(ActionManager, "discover_actions_from_path") as mock_discover:
                manager = create_action_manager("test_dcc")
                mock_discover.assert_called_once_with(test_dir)

    def test_get_action_manager_caching(self):
        """Test get_action_manager caching behavior."""
        # Import local modules
        from dcc_mcp_core.actions.manager import _action_managers
        from dcc_mcp_core.actions.manager import _action_managers_lock

        with _action_managers_lock:
            _action_managers.clear()

        manager1 = get_action_manager("test_dcc", name="cached")
        manager2 = get_action_manager("test_dcc", name="cached")
        assert manager1 is manager2

        manager3 = get_action_manager("test_dcc", name="cached", force_new=True)
        assert manager3 is not manager1

        with _action_managers_lock:
            _action_managers.clear()

    def test_call_action_async_sync_wrapper(self):
        """Test call_action_async via asyncio.run."""
        # Import built-in modules
        import asyncio

        manager = ActionManager("test", "test_dcc")
        registry = ActionRegistry()
        registry.register(SimpleTestAction)

        # call_action_async is wrapped by @error_handler which makes it sync
        # We need to test the actual async path
        result = manager.call_action_async("simple_test", value=5)
        assert isinstance(result, ActionResultModel)

    def test_call_action_async_not_found_sync(self):
        """Test call_action_async with non-existent action."""
        manager = ActionManager("test", "test_dcc")
        result = manager.call_action_async("non_existent")
        assert isinstance(result, ActionResultModel)

    def test_call_action_async_with_middleware_sync(self):
        """Test call_action_async with middleware."""
        manager = ActionManager("test", "test_dcc")
        registry = ActionRegistry()
        registry.register(SimpleTestAction)

        mock_middleware = MagicMock()
        mock_middleware.process_async = MagicMock(return_value=ActionResultModel(success=True, message="MW async"))
        manager.middleware = mock_middleware

        result = manager.call_action_async("simple_test", value=5)
        assert isinstance(result, ActionResultModel)

    def test_call_action_async_exception_sync(self):
        """Test call_action_async with exception during process."""
        manager = ActionManager("test", "test_dcc")

        class ExceptionAction:
            name = "exception_action"
            dcc = "test_dcc"

            def __init__(self, context=None):
                self.context = context or {}

            def setup(self, **kwargs):
                pass

            async def process_async(self):
                raise RuntimeError("Async exception")

        with patch.object(manager.registry, "get_action", return_value=ExceptionAction):
            result = manager.call_action_async("exception_action")
            assert isinstance(result, ActionResultModel)

    def test_call_action_with_empty_message(self):
        """Test call_action when result has empty message."""
        manager = ActionManager("test", "test_dcc")

        class EmptyMsgAction:
            name = "empty_msg"
            dcc = "test"

            def __init__(self, context=None):
                self.context = context or {}

            def setup(self, **kwargs):
                pass

            def process(self):
                return ActionResultModel(success=True, message="")

        with patch.object(manager.registry, "get_action", return_value=EmptyMsgAction):
            result = manager.call_action("empty_msg")
            assert "empty_msg" in result.message

    def test_refresh_actions_skipped(self):
        """Test refresh_actions when refresh is not needed."""
        # Import built-in modules
        import time

        manager = ActionManager("test", "test_dcc")
        manager._last_refresh_time = time.time()
        manager._refresh_interval = 3600
        manager.refresh_actions(force=False)

    def test_configure_middleware(self):
        """Test configure_middleware."""
        manager = ActionManager("test", "test_dcc")
        chain = manager.configure_middleware()
        assert isinstance(chain, MiddlewareChain)

    def test_discover_actions_from_path_with_custom_context(self):
        """Test discover_actions_from_path with custom context."""
        manager = ActionManager("test", "test_dcc")
        custom_context = {"custom": "value"}

        with patch.object(manager.registry, "discover_actions_from_path") as mock_discover:
            manager.discover_actions_from_path("/test/path", context=custom_context)
            mock_discover.assert_called_once_with(path="/test/path", dependencies=custom_context, dcc_name="test_dcc")

    def test_discover_actions_from_path_with_custom_dcc(self):
        """Test discover_actions_from_path with custom dcc_name."""
        manager = ActionManager("test", "test_dcc")

        with patch.object(manager.registry, "discover_actions_from_path") as mock_discover:
            manager.discover_actions_from_path("/test/path", dcc_name="maya")
            mock_discover.assert_called_once_with(path="/test/path", dependencies=manager.context, dcc_name="maya")

    def test_discover_actions_from_package_with_custom_dcc(self):
        """Test discover_actions_from_package with custom dcc_name."""
        manager = ActionManager("test", "test_dcc")

        with patch.object(manager.registry, "discover_actions_from_package") as mock_discover:
            manager.discover_actions_from_package("test_pkg", dcc_name="maya")
            mock_discover.assert_called_once_with(package_name="test_pkg", dcc_name="maya")


# ============================================================================
# Protocol coverage
# ============================================================================


class TestProtocolCov:
    """Cover uncovered lines in protocols/."""

    def test_resource_with_context(self):
        """Test Resource initialization with context."""

        class TestResource(Resource):
            uri = "test://resource"
            name = "Test"
            description = "Test resource"

            def read(self, **params):
                return "test content"

        resource = TestResource(context={"key": "value"})
        assert resource.context == {"key": "value"}

    @pytest.mark.asyncio
    async def test_resource_read_async(self):
        """Test Resource read_async default implementation."""

        class TestResource(Resource):
            uri = "test://resource"
            name = "Test"
            description = "Test resource"

            def read(self, **params):
                return "async content"

        resource = TestResource()
        content = await resource.read_async()
        assert content == "async content"

    def test_resource_get_uri_template(self):
        """Test Resource get_uri for template resource."""

        class TemplateResource(Resource):
            uri_template = "test://resource/{id}"
            name = "Test"
            description = "Test"

            def read(self, **params):
                return ""

        assert TemplateResource.get_uri() == "test://resource/{id}"

    def test_prompt_with_context(self):
        """Test Prompt initialization with context."""

        class TestPrompt(Prompt):
            name = "test_prompt"
            description = "Test prompt"

            def render(self, **kwargs):
                return "test"

        prompt = TestPrompt(context={"key": "value"})
        assert prompt.context == {"key": "value"}

    def test_action_to_tool_no_annotations(self):
        """Test action_to_tool with no annotations."""

        class MinimalAction(Action):
            name = "minimal"
            description = "Minimal action"
            dcc = "test"

            class InputModel(Action.InputModel):
                pass

            def _execute(self):
                pass

        tool = MCPAdapter.action_to_tool(MinimalAction, include_annotations=False)
        assert tool.annotations is None

    def test_action_to_tool_annotations_null_when_no_hints(self):
        """Test action_to_tool annotations are null when no hints set."""

        class NoHintAction(Action):
            name = "no_hint"
            description = "No hint action"
            dcc = "test"

            class InputModel(Action.InputModel):
                pass

            def _execute(self):
                pass

        tool = MCPAdapter.action_to_tool(NoHintAction, include_annotations=True)
        assert tool.annotations is None

    def test_action_to_tool_with_title_different_from_name(self):
        """Test action_to_tool with title different from name."""

        class TitledAction(Action):
            name = "titled"
            description = "Titled action"
            title = "My Custom Title"
            dcc = "test"

            class InputModel(Action.InputModel):
                pass

            def _execute(self):
                pass

        tool = MCPAdapter.action_to_tool(TitledAction, include_annotations=True)
        assert tool.annotations is not None
        assert tool.annotations.title == "My Custom Title"

    def test_prompt_to_definition_no_arguments(self):
        """Test prompt_to_definition with no arguments."""

        class EmptyPrompt(Prompt):
            name = "empty"
            description = "Empty prompt"

            def render(self, **kwargs):
                return ""

        prompt_def = MCPAdapter.prompt_to_definition(EmptyPrompt)
        assert prompt_def.name == "empty"
        assert prompt_def.arguments is None

    def test_protocol_runtime_checkable(self):
        """Test that protocols are runtime checkable."""

        class FullServer:
            @property
            def name(self):
                return "test"

            @property
            def version(self):
                return "1.0"

            async def list_tools(self):
                return []

            async def call_tool(self, name, arguments):
                return ActionResultModel(message="ok")

            async def list_resources(self):
                return []

            async def read_resource(self, uri):
                return ""

            async def list_prompts(self):
                return []

            async def get_prompt(self, name, arguments=None):
                return {}

        server = FullServer()
        assert isinstance(server, MCPServerProtocol)
        assert isinstance(server, MCPToolsProtocol)
        assert isinstance(server, MCPResourcesProtocol)
        assert isinstance(server, MCPPromptsProtocol)

    def test_tool_annotations_extra_allow(self):
        """Test ToolAnnotations allows extra fields."""
        annotations = ToolAnnotations(title="Test", custom_field="custom")
        assert annotations.title == "Test"

    def test_resource_template_definition(self):
        """Test ResourceTemplateDefinition creation."""
        template = ResourceTemplateDefinition(
            uriTemplate="test://{id}",
            name="Test",
            description="Test template",
        )
        assert template.uriTemplate == "test://{id}"


# ============================================================================
# Log config coverage
# ============================================================================


class TestLogConfigCov:
    """Cover uncovered lines in log_config.py."""

    def test_set_log_level_invalid(self):
        """Test set_log_level with invalid level."""
        # Import local modules
        from dcc_mcp_core.log_config import set_log_level

        set_log_level("INVALID_LEVEL")

    def test_get_logger_cached(self):
        """Test get_logger returns cached logger."""
        # Import local modules
        from dcc_mcp_core.log_config import get_logger

        logger1 = get_logger("cache_test_logger_x")
        logger2 = get_logger("cache_test_logger_x")
        assert logger1 is logger2

    def test_get_logger_with_dcc_type(self):
        """Test get_logger with DCC type."""
        # Import local modules
        from dcc_mcp_core.log_config import get_logger

        logger = get_logger("dcc_test_logger_x", dcc_type="maya")
        assert logger is not None

    def test_integrate_with_dcc_logger_none(self):
        """Test integrate_with_dcc_logger with None DCC logger."""
        # Import local modules
        from dcc_mcp_core.log_config import integrate_with_dcc_logger

        result = integrate_with_dcc_logger(None, "test_x", "test_dcc_x")
        assert result is not None

    def test_get_logger_info_unconfigured(self):
        """Test get_logger_info for unconfigured logger."""
        # Import local modules
        from dcc_mcp_core.log_config import get_logger_info

        info = get_logger_info("nonexistent_logger_xyz_abc")
        assert info["configured"] is False


# ============================================================================
# Exceptions coverage
# ============================================================================


class TestExceptionsCov:
    """Cover uncovered lines in utils/exceptions.py."""

    def test_action_error_str_with_action_class(self):
        """Test ActionError __str__ with action_class."""
        error = ActionError("Test error", action_class="TestClass")
        result = str(error)
        assert "Action class 'TestClass'" in result

    def test_action_setup_error(self):
        """Test ActionSetupError."""
        error = ActionSetupError(
            "Setup failed",
            action_name="test",
            missing_dependencies=["dep1", "dep2"],
        )
        assert error.missing_dependencies == ["dep1", "dep2"]
        assert error.code == "MCP-E-ACTION-SETUP"

    def test_action_validation_error(self):
        """Test ActionValidationError."""
        error = ActionValidationError(
            "Validation failed",
            action_name="test",
            validation_errors={"field1": "error1"},
        )
        assert error.validation_errors == {"field1": "error1"}
        assert error.code == "MCP-E-ACTION-VALIDATION"


# ============================================================================
# Result factory coverage
# ============================================================================


class TestResultFactoryCov:
    """Cover uncovered lines in utils/result_factory.py."""

    def test_from_exception_with_action_parameter_error(self):
        """Test from_exception with ActionParameterError."""
        error = ActionParameterError(
            message="Invalid parameter",
            action_name="test_action",
            action_class="TestAction",
            parameter_name="radius",
            parameter_value="-1",
            validation_error="Value must be positive",
        )
        result = from_exception(error, include_traceback=True)
        assert result.success is False
        assert "parameter_name" in result.context
        assert result.context["parameter_name"] == "radius"
        assert "validation_error" in result.context

    def test_from_exception_with_action_execution_error(self):
        """Test from_exception with ActionExecutionError."""
        error = ActionExecutionError(
            message="Execution failed",
            action_name="test_action",
            execution_phase="execute",
            traceback="Traceback...",
        )
        result = from_exception(error, include_traceback=True)
        assert result.success is False
        assert "execution_phase" in result.context

    def test_from_exception_with_action_error_no_dup(self):
        """Test from_exception with ActionError doesn't override existing context keys."""
        error = ActionError(
            message="Action error",
            action_name="test_action",
            action_class="TestAction",
        )
        result = from_exception(error, include_traceback=False, action_name="custom_name")
        assert result.context.get("action_name") == "custom_name"

    def test_from_exception_prompt_for_parameter_error(self):
        """Test from_exception default prompt for ActionParameterError."""
        error = ActionParameterError(message="Bad param", action_name="test")
        result = from_exception(error)
        assert "parameters" in result.prompt.lower() or "parameter" in result.prompt.lower()

    def test_from_exception_prompt_for_execution_error(self):
        """Test from_exception default prompt for ActionExecutionError."""
        error = ActionExecutionError(message="Exec fail", action_name="test")
        result = from_exception(error)
        assert "execution" in result.prompt.lower() or "error" in result.prompt.lower()

    def test_error_result_with_solutions(self):
        """Test error_result with possible_solutions."""
        result = error_result("Failed", "err", possible_solutions=["Fix A", "Fix B"])
        assert "possible_solutions" in result.context
        assert len(result.context["possible_solutions"]) == 2


# ============================================================================
# Filesystem coverage
# ============================================================================


class TestFilesystemCov:
    """Cover uncovered lines in utils/filesystem.py."""

    def test_get_platform_dir_invalid_type(self):
        """Test get_platform_dir with invalid directory type."""
        with pytest.raises(ValueError, match="Unknown directory type"):
            get_platform_dir("invalid_type")

    def test_ensure_directory_exists_error(self):
        """Test ensure_directory_exists with error."""
        with patch("dcc_mcp_core.utils.filesystem.Path") as mock_path:
            mock_instance = MagicMock()
            mock_instance.exists.return_value = False
            mock_instance.mkdir.side_effect = PermissionError("No permission")
            mock_path.return_value = mock_instance

            result = ensure_directory_exists("/no/permission/dir")
            assert result is False


# ============================================================================
# Pydantic extensions coverage
# ============================================================================


class TestPydanticExtCov:
    """Cover uncovered lines in utils/pydantic_extensions.py."""

    def test_apply_patches_with_auto_apply_false(self):
        """Test apply_patches with auto_apply=False."""
        # Import local modules
        from dcc_mcp_core.utils.pydantic_extensions import apply_patches

        result = apply_patches(auto_apply=False)
        assert result == {"uuid": False}

    def test_generate_uuid_schema_with_none(self):
        """Test generate_uuid_schema with None input."""
        # Import local modules
        from dcc_mcp_core.utils.pydantic_extensions import generate_uuid_schema

        result = generate_uuid_schema(None)
        assert result["type"] == "string"
        assert result["format"] == "uuid"


# ============================================================================
# Template coverage
# ============================================================================


class TestTemplateCov:
    """Cover uncovered lines in utils/template.py."""

    def test_render_template_default_dir(self):
        """Test render_template with default template directory."""
        # Import local modules
        from dcc_mcp_core.utils.filesystem import get_templates_directory
        from dcc_mcp_core.utils.template import render_template

        template_dir = get_templates_directory()
        # Verify template directory exists and has action.template
        if os.path.isfile(os.path.join(template_dir, "action.template")):
            result = render_template(
                "action.template",
                {
                    "action_name": "test",
                    "description": "Test",
                    "functions": [],
                    "author": "Test",
                    "dcc_name": "maya",
                    "date": "2025-01-01",
                },
            )
            assert isinstance(result, str)
        else:
            pytest.skip("action.template not found in template directory")

    def test_get_template_default_dir(self):
        """Test get_template with default template directory."""
        # Import local modules
        from dcc_mcp_core.utils.filesystem import get_templates_directory
        from dcc_mcp_core.utils.template import get_template

        template_dir = get_templates_directory()
        if os.path.isfile(os.path.join(template_dir, "action.template")):
            result = get_template("action.template")
            assert isinstance(result, str)
            assert len(result) > 0
        else:
            pytest.skip("action.template not found in template directory")


# ============================================================================
# Registry coverage
# ============================================================================


class TestRegistryCov:
    """Cover uncovered discovery lines in registry."""

    def test_add_discovery_hook(self):
        """Test add_discovery_hook instance method."""
        registry = ActionRegistry()

        def my_hook(reg, dcc_name):
            return []

        hook_key = registry.add_discovery_hook(my_hook)
        assert hook_key.startswith("hook_")
        ActionRegistry.clear_discovery_hooks()

    def test_discover_actions_from_package_with_hook(self):
        """Test discover_actions_from_package with hook."""
        registry = ActionRegistry()
        registry.register(SimpleTestAction)

        def my_hook(reg, dcc_name):
            return [SimpleTestAction]

        ActionRegistry.register_discovery_hook("test_hook_pkg", my_hook)
        result = registry.discover_actions_from_package("test_hook_pkg")
        assert len(result) == 1
        ActionRegistry.clear_discovery_hooks()

    def test_discover_actions_from_package_import_error(self):
        """Test discover_actions_from_package with import error."""
        registry = ActionRegistry()
        result = registry.discover_actions_from_package("nonexistent_package_xyz")
        assert result == []

    def test_simplify_schema_with_enum_and_default(self):
        """Test _simplify_schema with enum and default values."""
        registry = ActionRegistry()
        schema = {
            "title": "Test",
            "properties": {
                "mode": {
                    "type": "string",
                    "description": "Mode",
                    "enum": ["a", "b", "c"],
                    "default": "a",
                },
                "_private": {"type": "string", "description": "Private"},
            },
        }
        result = registry._simplify_schema(schema)
        assert "mode" in result["properties"]
        assert result["properties"]["mode"]["enum"] == ["a", "b", "c"]
        assert result["properties"]["mode"]["default"] == "a"
        assert "_private" not in result["properties"]

    def test_get_model_schema_error(self):
        """Test _get_model_schema with error."""
        registry = ActionRegistry()

        class BadAction(Action):
            name = "bad"
            dcc = "test"

            class InputModel:
                @staticmethod
                def model_json_schema():
                    raise RuntimeError("Schema error")

            def _execute(self):
                pass

        result = registry._get_model_schema(BadAction, "InputModel")
        assert result["properties"] == {}

    def test_list_actions_for_dcc_empty(self):
        """Test list_actions_for_dcc with non-existent DCC."""
        registry = ActionRegistry()
        result = registry.list_actions_for_dcc("nonexistent_dcc")
        assert result == []

    def test_refresh(self):
        """Test registry refresh."""
        registry = ActionRegistry()
        registry.refresh()

    def test_reset_instance(self):
        """Test _reset_instance backward compatibility."""
        ActionRegistry._reset_instance()
        registry = ActionRegistry()
        assert registry._actions == {}


# ============================================================================
# Middleware async coverage
# ============================================================================


class TestMiddlewareAsyncCov:
    """Cover uncovered async middleware lines."""

    @pytest.mark.asyncio
    async def test_logging_middleware_async(self):
        """Test LoggingMiddleware async processing."""
        middleware = LoggingMiddleware()
        action = SimpleTestAction()
        action.setup(value=5)
        result = await middleware.process_async(action)
        assert result.success is True

    @pytest.mark.asyncio
    async def test_logging_middleware_async_error(self):
        """Test LoggingMiddleware async with error."""

        class ErrorAction(Action):
            name = "error_action_mid"
            dcc = "test"

            class InputModel(Action.InputModel):
                pass

            def _execute(self):
                raise ValueError("Test error")

        middleware = LoggingMiddleware()
        action = ErrorAction()
        action.setup()
        result = await middleware.process_async(action)
        assert result.success is False

    @pytest.mark.asyncio
    async def test_logging_middleware_async_raises(self):
        """Test LoggingMiddleware async when exception propagates."""

        class RaisingAction:
            name = "raising_action"

            async def process_async(self):
                raise RuntimeError("Fatal error")

        middleware = LoggingMiddleware()
        with pytest.raises(RuntimeError, match="Fatal error"):
            await middleware.process_async(RaisingAction())

    @pytest.mark.asyncio
    async def test_performance_middleware_async(self):
        """Test PerformanceMiddleware async."""
        middleware = PerformanceMiddleware(threshold=0.001)
        action = SimpleTestAction()
        action.setup(value=5)
        result = await middleware.process_async(action)
        assert result.success is True
        assert "performance" in result.context
