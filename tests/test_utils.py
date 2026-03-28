"""Tests for utility functions: filesystem, type wrappers, constants."""

import os
import tempfile

import dcc_mcp_core


class TestConstants:
    def test_app_name(self):
        assert dcc_mcp_core.APP_NAME == "dcc-mcp"

    def test_app_author(self):
        assert dcc_mcp_core.APP_AUTHOR == "dcc-mcp"

    def test_default_dcc(self):
        assert dcc_mcp_core.DEFAULT_DCC == "python"

    def test_skill_metadata_file(self):
        assert dcc_mcp_core.SKILL_METADATA_FILE == "SKILL.md"

    def test_skill_scripts_dir(self):
        assert dcc_mcp_core.SKILL_SCRIPTS_DIR == "scripts"

    def test_env_skill_paths(self):
        assert dcc_mcp_core.ENV_SKILL_PATHS == "DCC_MCP_SKILL_PATHS"

    def test_env_log_level(self):
        assert dcc_mcp_core.ENV_LOG_LEVEL == "MCP_LOG_LEVEL"

    def test_default_log_level(self):
        assert dcc_mcp_core.DEFAULT_LOG_LEVEL == "DEBUG"


class TestFilesystem:
    def test_get_config_dir(self):
        path = dcc_mcp_core.get_config_dir()
        assert "dcc-mcp" in path

    def test_get_data_dir(self):
        path = dcc_mcp_core.get_data_dir()
        assert "dcc-mcp" in path

    def test_get_log_dir(self):
        path = dcc_mcp_core.get_log_dir()
        assert "dcc-mcp" in path
        assert "log" in path

    def test_get_platform_dir_config(self):
        path = dcc_mcp_core.get_platform_dir("config")
        assert "dcc-mcp" in path

    def test_get_platform_dir_data(self):
        path = dcc_mcp_core.get_platform_dir("data")
        assert "dcc-mcp" in path

    def test_get_platform_dir_cache(self):
        path = dcc_mcp_core.get_platform_dir("cache")
        assert "dcc-mcp" in path

    def test_get_platform_dir_log(self):
        path = dcc_mcp_core.get_platform_dir("log")
        assert "dcc-mcp" in path

    def test_get_platform_dir_state(self):
        path = dcc_mcp_core.get_platform_dir("state")
        assert "dcc-mcp" in path

    def test_get_platform_dir_documents(self):
        path = dcc_mcp_core.get_platform_dir("documents")
        assert "dcc-mcp" in path

    def test_get_platform_dir_invalid(self):
        import pytest

        with pytest.raises(ValueError):
            dcc_mcp_core.get_platform_dir("invalid_type")

    def test_get_actions_dir(self):
        path = dcc_mcp_core.get_actions_dir("maya")
        assert "maya" in path
        assert "actions" in path

    def test_get_actions_dir_different_dcc(self):
        path = dcc_mcp_core.get_actions_dir("blender")
        assert "blender" in path

    def test_get_skills_dir_no_dcc(self):
        path = dcc_mcp_core.get_skills_dir()
        assert isinstance(path, str)
        assert "skills" in path

    def test_get_skills_dir_with_dcc(self):
        path = dcc_mcp_core.get_skills_dir("maya")
        assert "maya" in path.lower()
        assert "skills" in path

    def test_get_skill_paths_from_env_empty(self):
        paths = dcc_mcp_core.get_skill_paths_from_env()
        assert isinstance(paths, list)

    def test_get_skill_paths_from_env_with_var(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            old = os.environ.get("DCC_MCP_SKILL_PATHS")
            try:
                os.environ["DCC_MCP_SKILL_PATHS"] = tmpdir
                paths = dcc_mcp_core.get_skill_paths_from_env()
                assert tmpdir in paths
            finally:
                if old is None:
                    os.environ.pop("DCC_MCP_SKILL_PATHS", None)
                else:
                    os.environ["DCC_MCP_SKILL_PATHS"] = old


class TestTypeWrappers:
    def test_boolean_wrapper(self):
        w = dcc_mcp_core.BooleanWrapper(True)
        assert w.value is True
        assert bool(w) is True

    def test_boolean_wrapper_false(self):
        w = dcc_mcp_core.BooleanWrapper(False)
        assert w.value is False
        assert bool(w) is False

    def test_boolean_wrapper_repr(self):
        assert "True" in repr(dcc_mcp_core.BooleanWrapper(True))
        assert "False" in repr(dcc_mcp_core.BooleanWrapper(False))

    def test_boolean_wrapper_eq(self):
        w = dcc_mcp_core.BooleanWrapper(True)
        assert w == True  # noqa: E712
        assert not (w == False)  # noqa: E712
        assert not (w == "not a bool")

    def test_int_wrapper(self):
        w = dcc_mcp_core.IntWrapper(42)
        assert w.value == 42
        assert int(w) == 42

    def test_int_wrapper_index(self):
        w = dcc_mcp_core.IntWrapper(3)
        # __index__ makes it usable as list index
        lst = [10, 20, 30, 40]
        assert lst[w] == 40  # index 3

    def test_int_wrapper_negative(self):
        w = dcc_mcp_core.IntWrapper(-5)
        assert w.value == -5
        assert int(w) == -5

    def test_int_wrapper_repr(self):
        assert "42" in repr(dcc_mcp_core.IntWrapper(42))

    def test_float_wrapper(self):
        w = dcc_mcp_core.FloatWrapper(3.14)
        assert w.value == 3.14
        assert float(w) == 3.14

    def test_float_wrapper_repr(self):
        assert "3.14" in repr(dcc_mcp_core.FloatWrapper(3.14))

    def test_string_wrapper(self):
        w = dcc_mcp_core.StringWrapper("hello")
        assert w.value == "hello"
        assert str(w) == "hello"

    def test_string_wrapper_empty(self):
        w = dcc_mcp_core.StringWrapper("")
        assert w.value == ""
        assert str(w) == ""

    def test_string_wrapper_repr(self):
        assert "hello" in repr(dcc_mcp_core.StringWrapper("hello"))

    # unwrap_value
    def test_unwrap_value_bool(self):
        v = dcc_mcp_core.unwrap_value(dcc_mcp_core.BooleanWrapper(True))
        assert v is True

    def test_unwrap_value_int(self):
        v = dcc_mcp_core.unwrap_value(dcc_mcp_core.IntWrapper(99))
        assert v == 99

    def test_unwrap_value_float(self):
        v = dcc_mcp_core.unwrap_value(dcc_mcp_core.FloatWrapper(2.718))
        assert v == 2.718

    def test_unwrap_value_string(self):
        v = dcc_mcp_core.unwrap_value(dcc_mcp_core.StringWrapper("hi"))
        assert v == "hi"

    def test_unwrap_value_passthrough_str(self):
        v = dcc_mcp_core.unwrap_value("plain")
        assert v == "plain"

    def test_unwrap_value_passthrough_int(self):
        v = dcc_mcp_core.unwrap_value(42)
        assert v == 42

    def test_unwrap_value_passthrough_none(self):
        v = dcc_mcp_core.unwrap_value(None)
        assert v is None

    # unwrap_parameters
    def test_unwrap_parameters(self):
        result = dcc_mcp_core.unwrap_parameters({
            "bool_key": dcc_mcp_core.BooleanWrapper(True),
            "int_key": dcc_mcp_core.IntWrapper(10),
            "float_key": dcc_mcp_core.FloatWrapper(1.5),
            "str_key": dcc_mcp_core.StringWrapper("val"),
            "plain": "hello",
        })
        assert result["bool_key"] is True
        assert result["int_key"] == 10
        assert result["float_key"] == 1.5
        assert result["str_key"] == "val"
        assert result["plain"] == "hello"

    def test_unwrap_parameters_empty(self):
        result = dcc_mcp_core.unwrap_parameters({})
        assert result == {}

    # wrap_value
    def test_wrap_value_bool(self):
        w = dcc_mcp_core.wrap_value(True)
        assert isinstance(w, dcc_mcp_core.BooleanWrapper)
        assert w.value is True

    def test_wrap_value_int(self):
        w = dcc_mcp_core.wrap_value(42)
        assert isinstance(w, dcc_mcp_core.IntWrapper)
        assert w.value == 42

    def test_wrap_value_float(self):
        w = dcc_mcp_core.wrap_value(3.14)
        assert isinstance(w, dcc_mcp_core.FloatWrapper)
        assert w.value == 3.14

    def test_wrap_value_string(self):
        w = dcc_mcp_core.wrap_value("hello")
        assert isinstance(w, dcc_mcp_core.StringWrapper)
        assert w.value == "hello"

    def test_wrap_value_passthrough(self):
        lst = [1, 2, 3]
        v = dcc_mcp_core.wrap_value(lst)
        assert v == [1, 2, 3]  # unsupported types pass through


class TestVersion:
    def test_version_exists(self):
        assert hasattr(dcc_mcp_core, "__version__")
        assert isinstance(dcc_mcp_core.__version__, str)
        assert dcc_mcp_core.__version__ != ""

    def test_all_exports(self):
        assert hasattr(dcc_mcp_core, "__all__")
        for name in dcc_mcp_core.__all__:
            assert hasattr(dcc_mcp_core, name), f"Missing export: {name}"

    def test_core_module_accessible(self):
        assert hasattr(dcc_mcp_core, "_core")

    def test_author(self):
        assert dcc_mcp_core._core.__author__ == "Hal Long <hal.long@outlook.com>"
