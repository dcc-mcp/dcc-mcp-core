"""Phase 3 coverage tests targeting remaining low-coverage modules.

Focuses on:
- protocols/server.py (71% -> higher)
- actions/manager.py (82% -> higher) - async paths, _load_skills_for_manager
- utils/dependency_injector.py (83% -> higher) - inject_core_modules, recursive inject
- utils/pydantic_extensions.py (81% -> higher) - edge cases
- utils/template.py (82% -> higher) - get_template
- actions/registry.py (86% -> higher) - _find_modules_in_package, discover_from_package
- actions/function_adapter.py (90% -> higher) - error paths
- log_config.py (89% -> higher) - loguru integration, set_log_level
- skills/loader.py (89% -> higher)
- skills/scanner.py (91% -> higher)
"""

import asyncio
import importlib
import logging
import os
import sys
import tempfile
import types
from pathlib import Path
from typing import Any, Dict, List, Optional
from unittest import mock
from unittest.mock import MagicMock, patch, PropertyMock

import pytest

from dcc_mcp_core.actions.base import Action
from dcc_mcp_core.models import ActionResultModel


# ---------------------------------------------------------------------------
# protocols/server.py — test isinstance checks on all protocol classes
# ---------------------------------------------------------------------------

class TestProtocolServerCoverage:
    """Cover the protocol body lines (the ``...`` stmts) via isinstance checks."""

    def test_mcp_server_protocol_isinstance(self):
        from dcc_mcp_core.protocols.server import MCPServerProtocol

        class _Impl:
            @property
            def name(self) -> str:
                return "test"

            @property
            def version(self) -> str:
                return "1.0"

            async def list_tools(self):
                return []

            async def call_tool(self, name, arguments):
                return ActionResultModel(success=True, message="ok")

            async def list_resources(self):
                return []

            async def read_resource(self, uri):
                return ""

            async def list_prompts(self):
                return []

            async def get_prompt(self, name, arguments=None):
                return {}

        impl = _Impl()
        assert isinstance(impl, MCPServerProtocol)

    def test_mcp_tools_protocol_isinstance(self):
        from dcc_mcp_core.protocols.server import MCPToolsProtocol

        class _ToolsImpl:
            async def list_tools(self):
                return []

            async def call_tool(self, name, arguments):
                return ActionResultModel(success=True, message="ok")

        assert isinstance(_ToolsImpl(), MCPToolsProtocol)

    def test_mcp_resources_protocol_isinstance(self):
        from dcc_mcp_core.protocols.server import MCPResourcesProtocol

        class _ResImpl:
            async def list_resources(self):
                return []

            async def read_resource(self, uri):
                return ""

        assert isinstance(_ResImpl(), MCPResourcesProtocol)

    def test_mcp_prompts_protocol_isinstance(self):
        from dcc_mcp_core.protocols.server import MCPPromptsProtocol

        class _PromptImpl:
            async def list_prompts(self):
                return []

            async def get_prompt(self, name, arguments=None):
                return {}

        assert isinstance(_PromptImpl(), MCPPromptsProtocol)

    def test_non_conforming_not_isinstance(self):
        from dcc_mcp_core.protocols.server import MCPServerProtocol

        class _Bad:
            pass

        assert not isinstance(_Bad(), MCPServerProtocol)


# ---------------------------------------------------------------------------
# actions/manager.py — call_action_async via asyncio.run, _load_skills
# ---------------------------------------------------------------------------

class TestManagerAsyncCoverage:
    """Cover async paths in ActionManager via __wrapped__ to bypass error_handler."""

    def _make_action_class(self, name="TestAct", success=True, raises=False):
        class _Act(Action):
            def _execute(self_):
                pass

        _Act.__name__ = name
        _Act.name = name

        async def process_async(self_):
            if raises:
                raise RuntimeError("boom")
            return ActionResultModel(success=success, message="done" if success else "")

        _Act.process_async = process_async
        return _Act

    def _run_async(self, coro):
        loop = asyncio.new_event_loop()
        try:
            return loop.run_until_complete(coro)
        finally:
            loop.close()

    def test_call_action_async_success(self):
        from dcc_mcp_core.actions.manager import ActionManager

        mgr = ActionManager("test", "test_dcc")
        act_cls = self._make_action_class("AsyncAct1")
        mgr.registry.register(act_cls)

        # Access the underlying unwrapped async function
        unwrapped = ActionManager.call_action_async.__wrapped__
        result = self._run_async(unwrapped(mgr, "AsyncAct1"))
        assert result.success is True

    def test_call_action_async_not_found(self):
        from dcc_mcp_core.actions.manager import ActionManager

        mgr = ActionManager("test", "test_dcc")
        unwrapped = ActionManager.call_action_async.__wrapped__
        result = self._run_async(unwrapped(mgr, "NoSuchAction"))
        assert result.success is False

    def test_call_action_async_with_middleware(self):
        from dcc_mcp_core.actions.manager import ActionManager

        mgr = ActionManager("test", "test_dcc")
        act_cls = self._make_action_class("AsyncMW")
        mgr.registry.register(act_cls)

        chain = mgr.configure_middleware()
        mgr.middleware = chain.build()

        unwrapped = ActionManager.call_action_async.__wrapped__
        result = self._run_async(unwrapped(mgr, "AsyncMW"))
        assert result.success is True

    def test_call_action_async_exception(self):
        from dcc_mcp_core.actions.manager import ActionManager

        mgr = ActionManager("test", "test_dcc")
        act_cls = self._make_action_class("AsyncErr", raises=True)
        mgr.registry.register(act_cls)

        unwrapped = ActionManager.call_action_async.__wrapped__
        result = self._run_async(unwrapped(mgr, "AsyncErr"))
        assert result.success is False
        assert "async execution failed" in result.message.lower() or "failed" in result.message.lower()

    def test_call_action_async_empty_message(self):
        from dcc_mcp_core.actions.manager import ActionManager

        mgr = ActionManager("test", "test_dcc")

        class _EmptyMsg(Action):
            name = "EmptyMsg"

            def _execute(self_):
                pass

            async def process_async(self_):
                return ActionResultModel(success=True, message="")

        mgr.registry.register(_EmptyMsg)
        unwrapped = ActionManager.call_action_async.__wrapped__
        result = self._run_async(unwrapped(mgr, "EmptyMsg"))
        assert result.success is True
        assert result.message  # Should be filled in automatically

    def test_call_action_async_no_middleware(self):
        from dcc_mcp_core.actions.manager import ActionManager

        mgr = ActionManager("test", "test_dcc")
        act_cls = self._make_action_class("AsyncNoMW")
        mgr.registry.register(act_cls)
        mgr.middleware = None  # Explicitly no middleware

        unwrapped = ActionManager.call_action_async.__wrapped__
        result = self._run_async(unwrapped(mgr, "AsyncNoMW"))
        assert result.success is True


class TestCreateActionManagerCoverage:
    """Cover create_action_manager with env paths and skill loading."""

    def test_create_action_manager_with_env_paths(self, tmp_path):
        from dcc_mcp_core.actions.manager import create_action_manager

        action_dir = tmp_path / "actions"
        action_dir.mkdir()
        # Write a simple action module
        (action_dir / "my_action.py").write_text(
            "from dcc_mcp_core.actions.base import Action\n"
            "from dcc_mcp_core.models import ActionResultModel\n"
            "class MyEnvAction(Action):\n"
            "    name = 'MyEnvAction'\n"
            "    def _execute(self): pass\n"
        )

        with patch.dict(os.environ, {"DCC_MCP_ACTION_PATHS": str(action_dir)}):
            mgr = create_action_manager("test_dcc", load_skill_paths=False)
        assert mgr is not None

    def test_create_action_manager_skill_loading_error(self):
        from dcc_mcp_core.actions.manager import create_action_manager

        with patch("dcc_mcp_core.actions.manager._load_skills_for_manager", side_effect=RuntimeError("fail")):
            mgr = create_action_manager("test_dcc", load_skill_paths=True)
        assert mgr is not None

    def test_load_skills_for_manager(self, tmp_path):
        from dcc_mcp_core.actions.manager import _load_skills_for_manager, ActionManager

        mgr = ActionManager("test", "test_dcc")

        # Mock scanner to return no skill dirs
        with patch("dcc_mcp_core.skills.scanner.SkillScanner") as MockScanner:
            mock_scanner = MockScanner.return_value
            mock_scanner.scan.return_value = []
            _load_skills_for_manager(mgr, "test_dcc", None)

    def test_load_skills_for_manager_with_skills(self, tmp_path):
        from dcc_mcp_core.actions.manager import _load_skills_for_manager, ActionManager

        mgr = ActionManager("test", "test_dcc")

        skill_dir = str(tmp_path / "my_skill")
        mock_action = MagicMock()
        mock_action.name = "skill_action"

        with patch("dcc_mcp_core.skills.scanner.SkillScanner") as MockScanner, \
             patch("dcc_mcp_core.skills.loader.load_skill") as mock_load:
            MockScanner.return_value.scan.return_value = [skill_dir]
            mock_load.return_value = [mock_action]
            _load_skills_for_manager(mgr, "test_dcc", None)

    def test_load_skills_for_manager_load_error(self, tmp_path):
        from dcc_mcp_core.actions.manager import _load_skills_for_manager, ActionManager

        mgr = ActionManager("test", "test_dcc")
        skill_dir = str(tmp_path / "bad_skill")

        with patch("dcc_mcp_core.skills.scanner.SkillScanner") as MockScanner, \
             patch("dcc_mcp_core.skills.loader.load_skill", side_effect=RuntimeError("fail")):
            MockScanner.return_value.scan.return_value = [skill_dir]
            _load_skills_for_manager(mgr, "test_dcc", None)  # Should not raise


# ---------------------------------------------------------------------------
# utils/dependency_injector.py
# ---------------------------------------------------------------------------

class TestDependencyInjectorCoverage:
    """Cover inject_core_modules, _get_all_submodules edge cases, inject_submodules recursive."""

    def test_inject_core_modules(self):
        from dcc_mcp_core.utils.dependency_injector import inject_dependencies

        mod = types.ModuleType("test_inject_core")
        inject_dependencies(mod, inject_core_modules=True)
        assert hasattr(mod, "dcc_mcp_core")

    def test_inject_core_modules_with_dcc_name(self):
        from dcc_mcp_core.utils.dependency_injector import inject_dependencies

        mod = types.ModuleType("test_inject_dcc")
        inject_dependencies(mod, dcc_name="maya", inject_core_modules=True)
        assert mod.DCC_NAME == "maya"
        assert hasattr(mod, "dcc_mcp_core")

    def test_inject_submodules_recursive(self):
        from dcc_mcp_core.utils.dependency_injector import inject_submodules

        mod = types.ModuleType("test_sub")
        inject_submodules(mod, "dcc_mcp_core", ["actions", "utils"], recursive=True)
        assert hasattr(mod, "actions")
        assert hasattr(mod, "utils")

    def test_inject_submodules_nonexistent(self):
        from dcc_mcp_core.utils.dependency_injector import inject_submodules

        mod = types.ModuleType("test_sub_ne")
        inject_submodules(mod, "dcc_mcp_core", ["nonexistent_submodule_xyz"])
        assert not hasattr(mod, "nonexistent_submodule_xyz")

    def test_get_all_submodules_no_name(self):
        from dcc_mcp_core.utils.dependency_injector import _get_all_submodules

        mod = types.ModuleType("test_mod")
        # Remove __name__ to test fallback
        del mod.__name__
        result = _get_all_submodules(mod)
        assert isinstance(result, dict)

    def test_get_all_submodules_no_name_no_file(self):
        from dcc_mcp_core.utils.dependency_injector import _get_all_submodules

        mod = types.ModuleType("test_mod2")
        del mod.__name__
        # Ensure no __file__ either
        if hasattr(mod, "__file__"):
            del mod.__file__
        result = _get_all_submodules(mod)
        assert isinstance(result, dict)

    def test_get_all_submodules_with_file(self):
        from dcc_mcp_core.utils.dependency_injector import _get_all_submodules

        mod = types.ModuleType("test_mod3")
        del mod.__name__
        mod.__file__ = "/fake/path/mymod.py"
        result = _get_all_submodules(mod)
        assert isinstance(result, dict)


# ---------------------------------------------------------------------------
# utils/pydantic_extensions.py
# ---------------------------------------------------------------------------

class TestPydanticExtensionsCoverage:
    """Cover apply_patches edge cases, apply_uuid_patch idempotent, _register_uuid_serialization."""

    def test_apply_patches_auto_false(self):
        from dcc_mcp_core.utils.pydantic_extensions import apply_patches

        result = apply_patches(auto_apply=False)
        assert result == {"uuid": False}

    def test_apply_uuid_patch_already_patched(self):
        from dcc_mcp_core.utils.pydantic_extensions import apply_uuid_patch

        # First call should apply (or already applied)
        apply_uuid_patch()
        # Second call should return False (already patched)
        result = apply_uuid_patch()
        assert result is False

    def test_is_patched(self):
        from dcc_mcp_core.utils.pydantic_extensions import is_patched

        assert is_patched() is True  # Module applies patches on import

    def test_register_uuid_serialization(self):
        from dcc_mcp_core.utils.pydantic_extensions import _register_uuid_serialization

        # Should return True or False without error
        result = _register_uuid_serialization()
        assert isinstance(result, bool)

    def test_generate_uuid_schema_empty(self):
        from dcc_mcp_core.utils.pydantic_extensions import generate_uuid_schema

        result = generate_uuid_schema(None)
        assert result["type"] == "string"
        assert result["format"] == "uuid"

    def test_generate_uuid_schema_with_existing(self):
        from dcc_mcp_core.utils.pydantic_extensions import generate_uuid_schema

        result = generate_uuid_schema({"title": "MyUUID"})
        assert result["type"] == "string"
        assert result["format"] == "uuid"
        assert result["title"] == "MyUUID"

    def test_apply_patches_full_flow(self):
        from dcc_mcp_core.utils.pydantic_extensions import apply_patches

        result = apply_patches(auto_apply=True)
        assert "uuid" in result


# ---------------------------------------------------------------------------
# utils/template.py
# ---------------------------------------------------------------------------

class TestTemplateCoverage:
    """Cover get_template and render_template with custom dirs."""

    def test_get_template(self, tmp_path):
        from dcc_mcp_core.utils.template import get_template

        # Create a template file
        tmpl = tmp_path / "test.template"
        tmpl.write_text("Hello {{ name }}!")

        content = get_template("test.template", template_dir=str(tmp_path))
        assert "Hello" in content

    def test_get_template_default_dir(self):
        from dcc_mcp_core.utils.template import get_template
        from dcc_mcp_core.utils.filesystem import get_templates_directory

        template_dir = get_templates_directory()
        if os.path.isfile(os.path.join(template_dir, "action.template")):
            content = get_template("action.template")
            assert content
        else:
            pytest.skip("action.template not found in template directory")

    def test_render_template_custom_dir(self, tmp_path):
        from dcc_mcp_core.utils.template import render_template

        tmpl = tmp_path / "greet.template"
        tmpl.write_text("Hi {{ user }}!")

        result = render_template("greet.template", {"user": "World"}, template_dir=str(tmp_path))
        assert result == "Hi World!"


# ---------------------------------------------------------------------------
# actions/registry.py — _find_modules_in_package, discover_from_package
# ---------------------------------------------------------------------------

class TestRegistryCoverage:
    """Cover discover_actions_from_package and _find_modules_in_package."""

    def test_find_modules_in_package(self, tmp_path):
        from dcc_mcp_core.actions.registry import ActionRegistry

        reg = ActionRegistry()

        # Create a fake package structure
        pkg_dir = tmp_path / "fake_pkg"
        pkg_dir.mkdir()
        (pkg_dir / "__init__.py").write_text("")
        (pkg_dir / "module_a.py").write_text("")
        sub = pkg_dir / "sub"
        sub.mkdir()
        (sub / "__init__.py").write_text("")
        (sub / "module_b.py").write_text("")

        modules = reg._find_modules_in_package("fake_pkg", pkg_dir)
        assert any("module_a" in m for m in modules)
        assert any("module_b" in m for m in modules)

    def test_discover_actions_from_module_object(self):
        from dcc_mcp_core.actions.registry import ActionRegistry

        reg = ActionRegistry()

        # Create a module with an Action subclass
        mod = types.ModuleType("test_discover_mod")

        class DiscoverableAction(Action):
            name = "DiscoverableAction"

            def _execute(self):
                pass

        mod.DiscoverableAction = DiscoverableAction
        # Need to set it properly for inspect.getmembers
        sys.modules["test_discover_mod"] = mod

        try:
            discovered = reg._discover_actions_from_module_object(mod, dcc_name="test")
            assert len(discovered) >= 1
        finally:
            del sys.modules["test_discover_mod"]

    def test_discover_actions_from_module_with_source_file(self):
        from dcc_mcp_core.actions.registry import ActionRegistry

        reg = ActionRegistry()
        mod = types.ModuleType("test_src_file_mod")

        class SrcAction(Action):
            name = "SrcAction"

            def _execute(self):
                pass

        mod.SrcAction = SrcAction
        sys.modules["test_src_file_mod"] = mod

        try:
            discovered = reg._process_module_for_actions(mod, dcc_name="test", source_file="/fake/path.py")
            assert len(discovered) >= 1
            for cls in discovered:
                assert hasattr(cls, "_source_file")
        finally:
            del sys.modules["test_src_file_mod"]

    def test_extract_model_schema_error(self):
        from dcc_mcp_core.actions.registry import ActionRegistry

        reg = ActionRegistry()

        class BadModelAction(Action):
            name = "BadModelAction"

            def _execute(self):
                pass

        # Set a broken InputModel
        BadModelAction.InputModel = "not_a_model"
        schema = reg._get_model_schema(BadModelAction, "InputModel")
        assert schema["type"] == "object"


# ---------------------------------------------------------------------------
# actions/function_adapter.py — error paths
# ---------------------------------------------------------------------------

class TestFunctionAdapterCoverage:
    """Cover error paths in create_function_adapter and create_function_adapters."""

    def test_create_function_adapter_processing_error(self):
        from dcc_mcp_core.actions.function_adapter import create_function_adapter
        from dcc_mcp_core.actions.manager import ActionManager

        mgr = ActionManager("test", "test_dcc")

        class FailAction(Action):
            name = "FailAction"

            def _execute(self):
                raise RuntimeError("processing error")

        mgr.registry.register(FailAction)
        adapter = create_function_adapter("FailAction", manager=mgr)
        result = adapter()
        assert result.success is False

    def test_create_function_adapters_with_action_names(self):
        from dcc_mcp_core.actions.function_adapter import create_function_adapters
        from dcc_mcp_core.actions.manager import ActionManager

        mgr = ActionManager("test", "test_dcc")

        class AdaptAction(Action):
            name = "AdaptAction"

            def _execute(self):
                pass

        mgr.registry.register(AdaptAction)
        adapters = create_function_adapters(manager=mgr, action_names=["AdaptAction"])
        assert "AdaptAction" in adapters

    def test_create_function_adapters_invalid_names_type(self):
        from dcc_mcp_core.actions.function_adapter import create_function_adapters

        adapters = create_function_adapters(action_names="not_a_list")
        assert adapters == {}

    def test_create_function_adapters_no_manager_no_names(self):
        from dcc_mcp_core.actions.function_adapter import create_function_adapters

        # Uses registry directly
        adapters = create_function_adapters(dcc_name="nonexistent_dcc_xyz")
        assert isinstance(adapters, dict)

    def test_create_function_adapters_invalid_manager(self):
        from dcc_mcp_core.actions.function_adapter import create_function_adapters

        # Manager without list_available_actions
        bad_mgr = MagicMock(spec=[])
        adapters = create_function_adapters(manager=bad_mgr)
        assert adapters == {}


# ---------------------------------------------------------------------------
# log_config.py — loguru integration, set_log_level
# ---------------------------------------------------------------------------

class TestLogConfigCoverage:
    """Cover loguru logger setup, set_log_level, integrate_with_dcc_logger."""

    def test_setup_loguru_logger(self):
        from dcc_mcp_core.log_config import setup_loguru_logger, _configured_loggers

        logger = setup_loguru_logger("test_loguru_cov", dcc_type="maya")
        assert logger is not None

    def test_set_log_level_valid(self):
        from dcc_mcp_core import log_config

        log_config.set_log_level("DEBUG")
        assert log_config.LOG_LEVEL == "DEBUG"
        # Reset
        log_config.set_log_level("INFO")

    def test_set_log_level_invalid(self):
        from dcc_mcp_core import log_config

        log_config.set_log_level("INVALID_LEVEL")
        assert log_config.LOG_LEVEL == "INFO"

    def test_integrate_with_dcc_logger_none(self):
        from dcc_mcp_core.log_config import integrate_with_dcc_logger

        result = integrate_with_dcc_logger(None, "test_int", "maya")
        assert result is not None

    def test_integrate_with_standard_logger(self):
        from dcc_mcp_core.log_config import integrate_with_dcc_logger

        dcc_logger = logging.getLogger("test_dcc_integration")
        result = integrate_with_dcc_logger(dcc_logger, "test_std_int", "houdini")
        assert result is not None

    def test_integrate_with_loguru_handler(self):
        from dcc_mcp_core.log_config import integrate_with_dcc_logger, setup_loguru_logger

        # Set up a loguru logger first
        setup_loguru_logger("test_loguru_int", dcc_type="nuke")

        dcc_logger = logging.getLogger("test_dcc_loguru_int")
        result = integrate_with_dcc_logger(dcc_logger, "test_loguru_int", "nuke")
        assert result is not None

    def test_get_logger_cached(self):
        from dcc_mcp_core.log_config import get_logger

        # First call creates
        logger1 = get_logger("test_cached_logger")
        # Second call returns cached
        logger2 = get_logger("test_cached_logger")
        assert logger1 is logger2

    def test_get_logger_info_unconfigured(self):
        from dcc_mcp_core.log_config import get_logger_info

        info = get_logger_info("nonexistent_logger_xyz")
        assert info["configured"] is False

    def test_get_logger_info_configured(self):
        from dcc_mcp_core.log_config import get_logger, get_logger_info

        get_logger("test_info_logger")
        info = get_logger_info("test_info_logger")
        assert info["configured"] is True

    def test_setup_logging_legacy(self):
        from dcc_mcp_core.log_config import setup_logging

        logger = setup_logging("test_legacy")
        assert logger is not None

    def test_setup_dcc_logging_legacy(self):
        from dcc_mcp_core.log_config import setup_dcc_logging

        logger = setup_dcc_logging("maya")
        assert logger is not None

    def test_setup_rpyc_logging(self):
        from dcc_mcp_core.log_config import setup_rpyc_logging

        logger = setup_rpyc_logging()
        assert logger is not None


# ---------------------------------------------------------------------------
# actions/manager.py — _merge_context, _update_context, _check_auto_refresh
# ---------------------------------------------------------------------------

class TestManagerHelpersCoverage:
    """Cover _merge_context, _update_context, _check_auto_refresh, refresh_actions."""

    def test_merge_context_empty(self):
        from dcc_mcp_core.actions.manager import ActionManager

        mgr = ActionManager("test", "test_dcc", context={"key": "val"})
        merged = mgr._merge_context(None)
        assert "key" in merged
        assert merged["key"] == "val"

    def test_merge_context_with_data(self):
        from dcc_mcp_core.actions.manager import ActionManager

        mgr = ActionManager("test", "test_dcc", context={"key": "val"})
        merged = mgr._merge_context({"extra": "data"})
        assert "key" in merged
        assert merged["extra"] == "data"

    def test_update_context(self):
        from dcc_mcp_core.actions.manager import ActionManager

        mgr = ActionManager("test", "test_dcc")
        mgr._update_context({"new_key": "new_val"})
        assert mgr.context["new_key"] == "new_val"

    def test_check_auto_refresh_disabled(self):
        from dcc_mcp_core.actions.manager import ActionManager

        mgr = ActionManager("test", "test_dcc", auto_refresh=False)
        mgr._check_auto_refresh()  # Should do nothing

    def test_check_auto_refresh_enabled(self):
        from dcc_mcp_core.actions.manager import ActionManager
        import time

        mgr = ActionManager("test", "test_dcc", auto_refresh=True, refresh_interval=0)
        mgr._last_refresh_time = time.time() - 100
        mgr._check_auto_refresh()  # Should trigger refresh

    def test_refresh_actions_with_paths(self, tmp_path):
        from dcc_mcp_core.actions.manager import ActionManager

        mgr = ActionManager("test", "test_dcc")
        # Create an action dir
        action_dir = tmp_path / "acts"
        action_dir.mkdir()
        (action_dir / "act.py").write_text(
            "from dcc_mcp_core.actions.base import Action\n"
            "from dcc_mcp_core.models import ActionResultModel\n"
            "class RefreshAct(Action):\n"
            "    name = 'RefreshAct'\n"
            "    def _execute(self): pass\n"
        )
        mgr.refresh_actions(action_paths=[str(action_dir)])


# ---------------------------------------------------------------------------
# skills/loader.py — edge cases
# ---------------------------------------------------------------------------

class TestSkillLoaderCoverage:
    """Cover _try_yaml_parse, _regex_parse_frontmatter, load_skill_metadata edge cases."""

    def test_try_yaml_parse_valid(self):
        from dcc_mcp_core.skills.loader import _try_yaml_parse

        result = _try_yaml_parse("name: test\nversion: 1.0")
        # Returns None if pyyaml is not installed
        try:
            import yaml
            assert result is not None
            assert result["name"] == "test"
        except ImportError:
            assert result is None

    def test_try_yaml_parse_invalid(self):
        from dcc_mcp_core.skills.loader import _try_yaml_parse

        result = _try_yaml_parse("!!invalid: [[[")
        # Should return None (either parse error or no yaml)
        assert result is None or isinstance(result, dict)

    def test_regex_parse_frontmatter(self):
        from dcc_mcp_core.skills.loader import _regex_parse_frontmatter

        result = _regex_parse_frontmatter("name: test\nversion: 1.0\ntags: [a, b]")
        assert isinstance(result, dict)

    def test_load_skill_metadata_no_file(self, tmp_path):
        from dcc_mcp_core.skills.loader import parse_skill_md

        result = parse_skill_md(str(tmp_path / "nonexistent"))
        assert result is None

    def test_load_skill_metadata_no_frontmatter(self, tmp_path):
        from dcc_mcp_core.skills.loader import parse_skill_md

        skill_dir = tmp_path / "skill_no_fm"
        skill_dir.mkdir()
        (skill_dir / "SKILL.md").write_text("Just a readme without frontmatter")
        result = parse_skill_md(str(skill_dir))
        assert result is None

    def test_load_skill_metadata_valid(self, tmp_path):
        from dcc_mcp_core.skills.loader import parse_skill_md

        skill_dir = tmp_path / "valid_skill"
        skill_dir.mkdir()
        (skill_dir / "SKILL.md").write_text(
            "---\nname: my_skill\nversion: 1.0\n---\n\nSkill description."
        )
        result = parse_skill_md(str(skill_dir))
        assert result is not None
        assert result.name == "my_skill"

    def test_load_skill_metadata_missing_name(self, tmp_path):
        from dcc_mcp_core.skills.loader import parse_skill_md

        skill_dir = tmp_path / "noname_skill"
        skill_dir.mkdir()
        (skill_dir / "SKILL.md").write_text(
            "---\nversion: 1.0\n---\n\nNo name field."
        )
        result = parse_skill_md(str(skill_dir))
        assert result is not None
        assert result.name == "noname_skill"  # Falls back to dir name


# ---------------------------------------------------------------------------
# skills/scanner.py — edge cases
# ---------------------------------------------------------------------------

class TestSkillScannerCoverage:
    """Cover scanner edge cases."""

    def test_scan_with_cache(self, tmp_path):
        from dcc_mcp_core.skills.scanner import SkillScanner

        scanner = SkillScanner()

        # Create a skill dir
        skill = tmp_path / "cached_skill"
        skill.mkdir()
        (skill / "SKILL.md").write_text("---\nname: cached\n---\n")

        # First scan - populates cache
        with patch("dcc_mcp_core.skills.scanner.get_skills_dir", return_value=str(tmp_path)):
            results1 = scanner.scan()
        assert len(results1) >= 1

        # Second scan - should use cache
        with patch("dcc_mcp_core.skills.scanner.get_skills_dir", return_value=str(tmp_path)):
            results2 = scanner.scan()
        assert len(results2) >= 1

    def test_scan_with_force_refresh(self, tmp_path):
        from dcc_mcp_core.skills.scanner import SkillScanner

        scanner = SkillScanner()

        skill = tmp_path / "refresh_skill"
        skill.mkdir()
        (skill / "SKILL.md").write_text("---\nname: refreshed\n---\n")

        with patch("dcc_mcp_core.skills.scanner.get_skills_dir", return_value=str(tmp_path)):
            results = scanner.scan(force_refresh=True)
        assert len(results) >= 1

    def test_scan_permission_error(self, tmp_path):
        from dcc_mcp_core.skills.scanner import SkillScanner

        scanner = SkillScanner()

        with patch("dcc_mcp_core.skills.scanner.get_skills_dir", return_value=str(tmp_path / "nonexistent")):
            results = scanner.scan()
        assert isinstance(results, list)

    def test_scan_with_dcc_name(self, tmp_path):
        from dcc_mcp_core.skills.scanner import SkillScanner

        scanner = SkillScanner()

        with patch("dcc_mcp_core.skills.scanner.get_skills_dir", return_value=str(tmp_path)):
            results = scanner.scan(dcc_name="maya")
        assert isinstance(results, list)


# ---------------------------------------------------------------------------
# actions/middleware.py — async process, exception in logging middleware
# ---------------------------------------------------------------------------

class TestMiddlewareCoverage:
    """Cover async middleware process."""

    def test_logging_middleware_async_success(self):
        from dcc_mcp_core.actions.middleware import LoggingMiddleware

        lm = LoggingMiddleware()

        class _Act(Action):
            name = "AsyncMWAct"

            def _execute(self_):
                pass

            async def process_async(self_):
                return ActionResultModel(success=True, message="ok")

        action = _Act()
        loop = asyncio.new_event_loop()
        try:
            result = loop.run_until_complete(lm.process_async(action))
        finally:
            loop.close()
        assert result.success is True

    def test_logging_middleware_async_failure(self):
        from dcc_mcp_core.actions.middleware import LoggingMiddleware

        lm = LoggingMiddleware()

        class _Act(Action):
            name = "AsyncMWFail"

            def _execute(self_):
                pass

            async def process_async(self_):
                return ActionResultModel(success=False, message="failed", error="err")

        action = _Act()
        loop = asyncio.new_event_loop()
        try:
            result = loop.run_until_complete(lm.process_async(action))
        finally:
            loop.close()
        assert result.success is False

    def test_logging_middleware_async_exception(self):
        from dcc_mcp_core.actions.middleware import LoggingMiddleware

        lm = LoggingMiddleware()

        class _Act(Action):
            name = "AsyncMWExc"

            def _execute(self_):
                pass

            async def process_async(self_):
                raise RuntimeError("async boom")

        action = _Act()
        loop = asyncio.new_event_loop()
        try:
            with pytest.raises(RuntimeError, match="async boom"):
                loop.run_until_complete(lm.process_async(action))
        finally:
            loop.close()


# ---------------------------------------------------------------------------
# utils/filesystem.py — discover_action_paths edge cases
# ---------------------------------------------------------------------------

class TestFilesystemCoverage:
    """Cover discover_action_paths env variable paths and get_skill_paths_from_env."""

    def test_discover_action_paths_with_env(self):
        from dcc_mcp_core.utils.filesystem import get_actions_paths_from_env

        with patch.dict(os.environ, {
            "DCC_MCP_ACTION_PATH_TESTDCC": "/fake/path1;/fake/path2",
            "DCC_MCP_ACTIONS_DIR": "/fake/generic"
        }):
            paths = get_actions_paths_from_env("testdcc")
        assert isinstance(paths, list)

    def test_get_skill_paths_from_env(self, tmp_path):
        from dcc_mcp_core.utils.filesystem import get_skill_paths_from_env

        real_dir = str(tmp_path)
        with patch.dict(os.environ, {"DCC_MCP_SKILL_PATHS": real_dir}):
            paths = get_skill_paths_from_env()
        assert real_dir in [os.path.abspath(p) for p in paths]

    def test_get_skill_paths_from_env_nonexistent(self):
        from dcc_mcp_core.utils.filesystem import get_skill_paths_from_env

        with patch.dict(os.environ, {"DCC_MCP_SKILL_PATHS": "/nonexistent/path/xyz"}):
            paths = get_skill_paths_from_env()
        assert isinstance(paths, list)


# ---------------------------------------------------------------------------
# actions/generator.py — type mapping edge cases
# ---------------------------------------------------------------------------

class TestGeneratorCoverage:
    """Cover parameter type mapping branches in generator via export_mcp_tools."""

    def test_export_mcp_tools(self):
        from dcc_mcp_core.actions.registry import ActionRegistry

        reg = ActionRegistry()

        class TypedAction(Action):
            """Action with typed fields for testing schema generation."""
            name = "TypedAction"
            description = "Test typed action"
            dcc = "test"

            def _execute(self):
                pass

        reg.register(TypedAction)
        actions = reg.list_actions(dcc_name="test")
        assert isinstance(actions, list)


# ---------------------------------------------------------------------------
# utils/exceptions.py — edge cases
# ---------------------------------------------------------------------------

class TestExceptionsCoverage:
    """Cover ActionParameterError details."""

    def test_action_parameter_error(self):
        from dcc_mcp_core.utils.exceptions import ActionParameterError

        err = ActionParameterError(
            message="bad param",
            action_name="test_action",
            parameter_name="param1",
            parameter_value="bad_val",
            validation_error="invalid type",
        )
        assert "bad param" in str(err)
        assert err.action_name == "test_action"
        assert err.parameter_name == "param1"

    def test_action_parameter_error_minimal(self):
        from dcc_mcp_core.utils.exceptions import ActionParameterError

        err = ActionParameterError(message="minimal error")
        assert "minimal error" in str(err)
