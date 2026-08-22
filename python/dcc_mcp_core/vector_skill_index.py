"""Compatibility imports for vector indexes in :mod:`dcc_mcp_core.skill_index`."""

from dcc_mcp_core.skill_index import InMemoryVectorStore
from dcc_mcp_core.skill_index import VectorSkillIndex
from dcc_mcp_core.skill_index import VectorStore

__all__ = ["InMemoryVectorStore", "VectorSkillIndex", "VectorStore"]
