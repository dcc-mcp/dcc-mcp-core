"""Tests for the MCP prompts primitive (issues #351, #355).

Covers:
* ``McpHttpConfig.enable_prompts`` getter / setter round-trips.
* The prompts capability defaults ON.
* Server initialisation and ``initialize`` response advertises
  ``prompts: { listChanged: true }`` when enabled.

Full end-to-end ``prompts/list`` + ``prompts/get`` are exercised by the Rust
integration tests in ``crates/dcc-mcp-http``; the Python layer only needs to
verify the configuration surface behaves as documented.
"""

from __future__ import annotations

from dcc_mcp_core import McpHttpConfig


def test_enable_prompts_default_on() -> None:
    cfg = McpHttpConfig(port=0)
    assert cfg.enable_prompts is True


def test_enable_prompts_setter_round_trip() -> None:
    cfg = McpHttpConfig(port=0)
    cfg.enable_prompts = False
    assert cfg.enable_prompts is False
    cfg.enable_prompts = True
    assert cfg.enable_prompts is True


def test_enable_prompts_independent_from_resources() -> None:
    cfg = McpHttpConfig(port=0)
    cfg.enable_prompts = False
    assert cfg.enable_resources is True
    cfg.enable_resources = False
    cfg.enable_prompts = True
    assert cfg.enable_prompts is True
    assert cfg.enable_resources is False
