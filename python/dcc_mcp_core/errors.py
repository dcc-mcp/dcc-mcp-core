"""Shared Python exception hierarchy for dcc-mcp-core."""

from __future__ import annotations


class DccMcpError(Exception):
    """Base class for public dcc-mcp-core Python exceptions."""


__all__ = ["DccMcpError"]
