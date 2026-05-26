"""Tests for the canonical skill-helper namespace."""

from __future__ import annotations

import pytest

import dcc_mcp_core
from dcc_mcp_core import skills_helper
from dcc_mcp_core.skills_helper import ToolValidator
from dcc_mcp_core.skills_helper import normalize_tool_arguments
from dcc_mcp_core.skills_helper import skill_error_from_exception
from dcc_mcp_core.skills_helper import skill_success


def test_skills_helper_json_yaml_codecs_roundtrip() -> None:
    payload = {"name": "café", "frames": [1, 2, 3], "enabled": True}

    encoded = skills_helper.json_dumps(payload, ensure_ascii=False)
    assert "café" in encoded
    assert skills_helper.json_loads(encoded) == payload

    yaml_encoded = skills_helper.yaml_dumps(payload)
    assert skills_helper.yaml_loads(yaml_encoded) == payload


def test_legacy_top_level_codecs_reexport_skills_helper() -> None:
    assert dcc_mcp_core.json_dumps is skills_helper.json_dumps
    assert dcc_mcp_core.json_loads is skills_helper.json_loads
    assert dcc_mcp_core.yaml_dumps is skills_helper.yaml_dumps
    assert dcc_mcp_core.yaml_loads is skills_helper.yaml_loads

    assert dcc_mcp_core.json_loads(dcc_mcp_core.json_dumps({"ok": True})) == {"ok": True}


def test_skills_helper_reexports_validation_and_normalization() -> None:
    validator = ToolValidator.from_schema_json(
        skills_helper.json_dumps(
            {
                "type": "object",
                "required": ["name"],
                "properties": {"name": {"type": "string"}},
            }
        )
    )

    ok, errors = validator.validate(skills_helper.json_dumps({"name": "maya"}))

    assert ok is True
    assert errors == []
    assert normalize_tool_arguments('{"name":"maya"}') == {"name": "maya"}


def test_skills_helper_reexports_skill_result_helpers() -> None:
    result = skill_success("Created cube", object_name="cube1")

    assert result["success"] is True
    assert result["message"] == "Created cube"
    assert result["context"] == {"object_name": "cube1"}


def test_skill_error_from_exception_uses_standard_skill_error_shape() -> None:
    exc = ValueError("bad radius")

    result = skill_error_from_exception(exc, prompt="Use a positive radius.", radius=-1)

    assert result["success"] is False
    assert result["message"] == "bad radius"
    assert result["error"] == "ValueError"
    assert result["prompt"] == "Use a positive radius."
    assert result["context"] == {"radius": -1}


def test_skills_helper_reports_invalid_json_errors() -> None:
    with pytest.raises(ValueError):
        skills_helper.json_loads("{not json}")
