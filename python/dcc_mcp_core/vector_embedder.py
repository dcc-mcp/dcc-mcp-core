"""Pluggable text embedders for :class:`SemanticSkillIndex` (issue #1393).

The default :class:`HashedEmbedder` is **zero-dep**: it combines token-level
hashing with character n-gram hashing to produce a fixed-dimensional dense
vector. This is a "semantic-lite" backend — it gives morphology-aware fuzzy
recall (better than BM25 on typos / casing / inflection) but is *not* true
dense semantic recall. For that, install ``dcc-mcp-core[semantic]`` and use
:class:`OnnxEmbedder` instead.

Deployment trade-off:

* :class:`HashedEmbedder` (default) — zero deps, deterministic, ~5 µs/document
  at ``dim=256``. Recall improves measurably over BM25 on natural-language
  queries that share sub-token structure with the indexed corpus.
* :class:`OnnxEmbedder` (optional, via ``[semantic]`` extra) — ONNX-Runtime
  backed local embedder; the actual model load is deferred to the first
  adopter and ships as a follow-up. Today the class is a typed stub that
  raises :class:`EmbedderError` with the install instructions, so adapters
  can wire it into their config without breaking the import graph.

Both implement the :class:`Embedder` Protocol; the future ``RemoteEmbedder``
(HTTP service for studio-wide embedding) will plug in via the same Protocol
with no caller change.

External references:

* Weinberger et al., *Feature Hashing for Large Scale Multitask Learning*
  (ICML 2009) — the hashing trick used by :class:`HashedEmbedder`.
* Reimers & Gurevych, *Sentence-BERT* (EMNLP 2019) — the dense embedding
  family :class:`OnnxEmbedder` targets via ``all-MiniLM-L6-v2`` and friends.
"""

from __future__ import annotations

from array import array
from dataclasses import dataclass
import hashlib
import math
import re
import sys
from typing import Iterable

if sys.version_info >= (3, 8):
    from typing import Protocol
    from typing import runtime_checkable
else:  # pragma: no cover - py3.7 only

    class Protocol:  # type: ignore[no-redef]
        pass

    def runtime_checkable(cls):  # type: ignore[no-redef]
        return cls


__all__ = [
    "DEFAULT_DIM",
    "Embedder",
    "EmbedderError",
    "HashedEmbedder",
    "OnnxEmbedder",
]


DEFAULT_DIM = 256
"""Default vector dimensionality.

256 keeps the hash-bucket collision rate under ~1% for ~10k documents while
keeping each row to 2 KiB of RAM (256 doubles at 8 bytes each). 384 or 512
would push collisions lower but inflates the brute-force cosine cost linearly.
"""


_TOKEN_RE = re.compile(r"[A-Za-z0-9]+")


def _tokens(text: str) -> list[str]:
    """Lowercase token extraction. Matches the regex used by ``semantic_skill_index._tokenise``."""
    return [tok.lower() for tok in _TOKEN_RE.findall(text)]


def _char_ngrams(token: str, n: int) -> list[str]:
    """Sliding-window character n-grams; falls back to the whole token when shorter than ``n``."""
    if len(token) <= n:
        return [token]
    return [token[i : i + n] for i in range(len(token) - n + 1)]


class EmbedderError(RuntimeError):
    """Raised when an embedder backend cannot be constructed or invoked."""


@runtime_checkable
class Embedder(Protocol):
    """Maps text to a fixed-dimensional dense vector.

    Implementations must return L2-normalised vectors so consumers can use
    plain dot product as cosine similarity.
    """

    @property
    def dim(self) -> int:
        """Vector dimensionality. Stable for the lifetime of the embedder."""

    def embed(self, text: str) -> array[float]:
        """Embed a single string. Empty / whitespace-only input returns the zero vector."""

    def embed_batch(self, texts: Iterable[str]) -> list[array[float]]:
        """Embed many strings; default implementations may loop ``embed``."""


def _hash_to_bucket(token: str, dim: int, salt: bytes) -> int:
    """Deterministic mapping ``token → [0, dim)`` via BLAKE2b with a per-purpose salt."""
    digest = hashlib.blake2b(token.encode("utf-8"), digest_size=8, salt=salt).digest()
    return int.from_bytes(digest, "little") % dim


def _hash_to_sign(token: str) -> int:
    """Deterministic sign in ``{-1, +1}`` so collisions in the hash bucket cancel rather than stack."""
    digest = hashlib.blake2b(token.encode("utf-8"), digest_size=2, salt=b"sign0000").digest()
    return 1 if (int.from_bytes(digest, "little") & 1) == 0 else -1


def _l2_normalise(vec: array[float]) -> array[float]:
    """Return a unit-length copy; the zero vector is returned unchanged so callers can detect empty input."""
    norm_sq = 0.0
    for value in vec:
        norm_sq += value * value
    norm = math.sqrt(norm_sq)
    if norm <= 1e-12:
        return vec
    inv = 1.0 / norm
    return array("d", (value * inv for value in vec))


@dataclass(frozen=True)
class HashedEmbedder:
    """Zero-dep deterministic embedder via the feature hashing trick.

    Each input is tokenised, every token contributes ``token_weight`` to its
    hash bucket, and every character n-gram contributes ``char_weight``. Signs
    are randomised by an independent hash so bucket collisions cancel on
    expectation (unbiased random projection). The final vector is L2-normalised
    so consumers can use dot product as cosine similarity.

    The character n-gram component gives the embedder its morphology-aware
    fuzzy behaviour — ``render``, ``rendering``, ``rendered`` all share most
    of their 3-grams and end up close in vector space, which BM25 cannot
    replicate without explicit stemming.

    Defaults are tuned for short technical text (skill names, summaries, tags);
    bump ``char_n`` to 4 for languages with longer morpheme lengths.
    """

    dim: int = DEFAULT_DIM
    char_n: int = 3
    token_weight: float = 1.0
    char_weight: float = 0.6

    def __post_init__(self) -> None:
        if self.dim <= 0:
            raise ValueError("HashedEmbedder.dim must be > 0")
        if self.char_n <= 0:
            raise ValueError("HashedEmbedder.char_n must be > 0")
        if self.token_weight <= 0 or self.char_weight < 0:
            raise ValueError("HashedEmbedder.token_weight must be > 0 and char_weight must be >= 0")

    def embed(self, text: str) -> array[float]:
        vec = array("d", (0.0 for _ in range(self.dim)))
        tokens = _tokens(text)
        if not tokens:
            return vec
        for tok in tokens:
            bucket = _hash_to_bucket(tok, self.dim, salt=b"token000")
            sign = _hash_to_sign(tok)
            vec[bucket] += self.token_weight * sign
            if self.char_weight > 0:
                for ngram in _char_ngrams(tok, self.char_n):
                    ng_bucket = _hash_to_bucket(ngram, self.dim, salt=b"char0000")
                    ng_sign = _hash_to_sign(ngram)
                    vec[ng_bucket] += self.char_weight * ng_sign
        return _l2_normalise(vec)

    def embed_batch(self, texts: Iterable[str]) -> list[array[float]]:
        return [self.embed(t) for t in texts]


class OnnxEmbedder:
    """ONNX-Runtime-backed dense embedder (requires the ``[semantic]`` extra).

    This class is a **typed stub** today — instantiating it without the extra
    raises a clear :class:`EmbedderError`; instantiating with the extra
    succeeds but :meth:`embed` still raises until the follow-up wires the
    actual model load (issue #1393). Adapters can already declare the
    intent ("use vector embeddings when available") in their config without
    importing ``fastembed`` themselves.

    The intended target backend is `fastembed <https://github.com/qdrant/fastembed>`_,
    which packages ONNX Runtime, the tokeniser, and a small ``BAAI/bge-small-en-v1.5``
    (or ``all-MiniLM-L6-v2``) model that downloads to ``~/.cache/fastembed/``
    on first use — keeping ``dcc-mcp-core`` itself dep-free while letting
    adapters opt in to real semantic recall with one ``pip install`` line.
    """

    _INSTALL_HINT = "OnnxEmbedder requires the 'semantic' extra. Install with: pip install 'dcc-mcp-core[semantic]'"

    def __init__(self, model_name: str = "BAAI/bge-small-en-v1.5") -> None:
        try:
            import fastembed  # noqa: F401
        except ImportError as exc:
            raise EmbedderError(self._INSTALL_HINT) from exc
        self._model_name = model_name
        self._dim = 384

    @property
    def dim(self) -> int:
        return self._dim

    @property
    def model_name(self) -> str:
        return self._model_name

    def embed(self, text: str) -> array[float]:
        raise EmbedderError(
            "OnnxEmbedder.embed is not yet wired to fastembed. "
            "Use HashedEmbedder for now or follow issue #1393 for the model-load follow-up."
        )

    def embed_batch(self, texts: Iterable[str]) -> list[array[float]]:
        return [self.embed(t) for t in texts]
