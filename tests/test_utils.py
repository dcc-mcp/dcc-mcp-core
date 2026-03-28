"""Tests for utility functions: filesystem, type wrappers, constants."""

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

    def test_env_vars(self):
        assert isinstance(dcc_mcp_core.ENV_SKILL_PATHS, str)
        assert isinstance(dcc_mcp_core.ENV_LOG_LEVEL, str)
        assert isinstance(dcc_mcp_core.DEFAULT_LOG_LEVEL, str)
        assert isinstance(dcc_mcp_core.SKILL_SCRIPTS_DIR, str)


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

    def test_get_platform_dir_config(self):
        path = dcc_mcp_core.get_platform_dir("config")
        assert "dcc-mcp" in path

    def test_get_platform_dir_invalid(self):
        import pytest

        with pytest.raises(ValueError):
            dcc_mcp_core.get_platform_dir("invalid_type")

    def test_get_actions_dir(self):
        path = dcc_mcp_core.get_actions_dir("maya")
        assert "maya" in path

    def test_get_skills_dir(self):
        path = dcc_mcp_core.get_skills_dir()
        assert isinstance(path, str)

    def test_get_skill_paths_from_env(self):
        paths = dcc_mcp_core.get_skill_paths_from_env()
        assert isinstance(paths, list)


class TestTypeWrappers:
    def test_boolean_wrapper(self):
        w = dcc_mcp_core.BooleanWrapper(True)
        assert w.value is True
        assert bool(w) is True
        assert "BooleanWrapper" in repr(w)

    def test_int_wrapper(self):
        w = dcc_mcp_core.IntWrapper(42)
        assert w.value == 42
        assert int(w) == 42
        assert "IntWrapper" in repr(w)

    def test_float_wrapper(self):
        w = dcc_mcp_core.FloatWrapper(3.14)
        assert w.value == 3.14
        assert float(w) == 3.14
        assert "FloatWrapper" in repr(w)

    def test_string_wrapper(self):
        w = dcc_mcp_core.StringWrapper("hello")
        assert w.value == "hello"
        assert str(w) == "hello"
        assert "StringWrapper" in repr(w)

    def test_unwrap_value_bool(self):
        w = dcc_mcp_core.BooleanWrapper(True)
        v = dcc_mcp_core.unwrap_value(w)
        assert v is True

    def test_unwrap_value_int(self):
        w = dcc_mcp_core.IntWrapper(99)
        v = dcc_mcp_core.unwrap_value(w)
        assert v == 99

    def test_unwrap_value_passthrough(self):
        v = dcc_mcp_core.unwrap_value("plain string")
        assert v == "plain string"

    def test_unwrap_parameters(self):
        w = dcc_mcp_core.IntWrapper(10)
        result = dcc_mcp_core.unwrap_parameters({"key": w, "name": "test"})
        assert result["key"] == 10
        assert result["name"] == "test"

    def test_wrap_value_bool(self):
        w = dcc_mcp_core.wrap_value(True)
        assert isinstance(w, dcc_mcp_core.BooleanWrapper)
        assert w.value is True

    def test_wrap_value_int(self):
        w = dcc_mcp_core.wrap_value(42)
        assert isinstance(w, dcc_mcp_core.IntWrapper)

    def test_wrap_value_float(self):
        w = dcc_mcp_core.wrap_value(3.14)
        assert isinstance(w, dcc_mcp_core.FloatWrapper)

    def test_wrap_value_string(self):
        w = dcc_mcp_core.wrap_value("hello")
        assert isinstance(w, dcc_mcp_core.StringWrapper)


class TestVersion:
    def test_version_exists(self):
        assert hasattr(dcc_mcp_core, "__version__")
        assert isinstance(dcc_mcp_core.__version__, str)

    def test_all_exports(self):
        assert hasattr(dcc_mcp_core, "__all__")
        for name in dcc_mcp_core.__all__:
            assert hasattr(dcc_mcp_core, name), f"Missing export: {name}"
