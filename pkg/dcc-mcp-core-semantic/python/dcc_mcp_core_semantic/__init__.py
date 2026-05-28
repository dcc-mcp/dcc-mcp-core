"""Native Rust semantic embeddings for ``dcc-mcp-core``.

This companion package is installed by ``pip install 'dcc-mcp-core[semantic]'``
or directly via ``pip install dcc-mcp-core-semantic``. It ships a single
native extension :mod:`dcc_mcp_core_semantic._native` (built by maturin
from ``crates/dcc-mcp-semantic``) that wraps `fastembed-rs
<https://github.com/Anush008/fastembed-rs>`_ for ONNX-backed sentence
embeddings.

Adapters do **not** import from this module directly; the public surface
is :class:`dcc_mcp_core.OnnxEmbedder`, which prefers this Rust backend
when available and falls back to the Python ``fastembed`` package
otherwise. See :mod:`dcc_mcp_core.vector_embedder` for the full fallback
chain.

Re-exports:

* :data:`native` — the compiled ``_native`` submodule with
  ``NativeEmbedder``, ``DEFAULT_MODEL``, and ``SUPPORTED_MODELS``.
"""

from __future__ import annotations

from dcc_mcp_core_semantic import _native as native

__all__ = ["native"]
__version__ = native.__version__
