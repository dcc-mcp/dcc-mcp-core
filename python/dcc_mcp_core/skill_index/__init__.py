"""Canonical Python API for optional in-process skill indexes.

The production Rust catalog and gateway continue to use
`dcc-mcp-gateway-search`. These Python indexes are application utilities for
offline lexical/vector retrieval and fusion.
"""

from dcc_mcp_core.skill_index._fusion import RrfFusionIndex
from dcc_mcp_core.skill_index._lexical import LexicalSkillIndex
from dcc_mcp_core.skill_index._protocol import SemanticSkillIndex
from dcc_mcp_core.skill_index._protocol import SkillDocument
from dcc_mcp_core.skill_index._protocol import SkillSearchHit
from dcc_mcp_core.skill_index._vector import InMemoryVectorStore
from dcc_mcp_core.skill_index._vector import VectorSkillIndex
from dcc_mcp_core.skill_index._vector import VectorStore

__all__ = [
    "InMemoryVectorStore",
    "LexicalSkillIndex",
    "RrfFusionIndex",
    "SemanticSkillIndex",
    "SkillDocument",
    "SkillSearchHit",
    "VectorSkillIndex",
    "VectorStore",
]
