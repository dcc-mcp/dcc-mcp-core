"""Tests for ToolsListStubPolicy env resolution (issues #174 / #238)."""

from __future__ import annotations

import os

import pytest

from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import ToolsListStubPolicy
from dcc_mcp_core import apply_tools_list_stub_policy
from dcc_mcp_core import dcc_exclude_stubs_env_name
from dcc_mcp_core import resolve_tools_list_stub_policy


def test_dcc_exclude_stubs_env_name_normalises_slug():
    assert dcc_exclude_stubs_env_name("maya") == "DCC_MCP_MAYA_EXCLUDE_STUBS_FROM_TOOLS_LIST"
    assert dcc_exclude_stubs_env_name("3ds-max") == "DCC_MCP_3DS_MAX_EXCLUDE_STUBS_FROM_TOOLS_LIST"


def test_resolve_per_dcc_env_wins_over_global(monkeypatch):
    monkeypatch.delenv("DCC_MCP_MAYA_EXCLUDE_STUBS_FROM_TOOLS_LIST", raising=False)
    monkeypatch.setenv("DCC_MCP_EXCLUDE_STUBS_FROM_TOOLS_LIST", "0")
    monkeypatch.setenv("DCC_MCP_BLENDER_EXCLUDE_STUBS_FROM_TOOLS_LIST", "1")
    policy = resolve_tools_list_stub_policy("blender")
    assert policy.exclude_skill_stubs is True
    assert policy.exclude_group_stubs is True


def test_resolve_maya_per_dcc_env(monkeypatch):
    monkeypatch.delenv("DCC_MCP_MAYA_EXCLUDE_STUBS_FROM_TOOLS_LIST", raising=False)
    monkeypatch.delenv("DCC_MCP_EXCLUDE_STUBS_FROM_TOOLS_LIST", raising=False)
    monkeypatch.setenv("DCC_MCP_MAYA_EXCLUDE_STUBS_FROM_TOOLS_LIST", "yes")
    policy = resolve_tools_list_stub_policy("maya")
    assert policy == ToolsListStubPolicy.exclude_all_progressive_stubs()


def test_apply_writes_mcp_http_config():
    cfg = McpHttpConfig(port=0)
    policy = apply_tools_list_stub_policy(
        cfg,
        "houdini",
        explicit=ToolsListStubPolicy(exclude_skill_stubs=True, exclude_group_stubs=False),
    )
    assert policy.exclude_skill_stubs is True
    assert policy.exclude_group_stubs is False
    assert cfg.exclude_skill_stubs_from_tools_list is True
    assert cfg.exclude_group_stubs_from_tools_list is False


def test_explicit_overrides_env(monkeypatch):
    monkeypatch.setenv("DCC_MCP_EXCLUDE_STUBS_FROM_TOOLS_LIST", "1")
    policy = resolve_tools_list_stub_policy(
        "maya",
        explicit=ToolsListStubPolicy(),
    )
    assert policy.exclude_skill_stubs is False


def test_dcc_name_required_for_env_helper():
    with pytest.raises(ValueError):
        dcc_exclude_stubs_env_name("   ")
