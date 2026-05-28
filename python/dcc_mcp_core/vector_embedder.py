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
* :class:`OnnxEmbedder` (optional, via ``[semantic]`` extra) — wraps
  ``fastembed``; ships ONNX Runtime, the HuggingFace tokeniser, and a small
  pre-trained encoder (``BAAI/bge-small-en-v1.5``, 384-dim, ~25 MB). Model
  downloads to ``~/.cache/fastembed/`` on first use and runs fully offline
  afterwards. Model name and cache directory can be overridden via
  ``DCC_MCP_EMBED_MODEL`` / ``DCC_MCP_EMBED_MODEL_DIR`` env vars so studios
  can pre-place the model on a shared mount.

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
import os
import re
import sys
from typing import Iterable
from typing import Mapping

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

    Wraps `fastembed <https://github.com/qdrant/fastembed>`_, which ships ONNX
    Runtime, the HuggingFace tokeniser, and a small pre-trained sentence
    encoder (``BAAI/bge-small-en-v1.5`` by default, 384-dim, ~25 MB
    quantised). The model auto-downloads on first use; once cached it runs
    fully offline.

    Configuration is precedence-ordered:

    1. Constructor argument (``model_name=``, ``cache_dir=``).
    2. Environment variable (:attr:`ENV_MODEL`, :attr:`ENV_MODEL_DIR`).
    3. Built-in default (:attr:`DEFAULT_MODEL`; fastembed's cache directory).

    The env-var path lets studios pre-place the model on a shared mount and
    point every adapter at it via ``DCC_MCP_EMBED_MODEL_DIR`` without
    touching adapter code — useful for firewalled deployments where
    runtime HuggingFace downloads are not allowed.

    Raises :class:`EmbedderError` when the ``[semantic]`` extra is missing
    or when fastembed fails to load the requested model.
    """

    #: Default embedding model. 384-dim, ~25 MB quantised, English-focused.
    DEFAULT_MODEL = "BAAI/bge-small-en-v1.5"

    #: Fallback vector dimension used only when the loaded model fails to
    #: report its embedding size via a probe. Matches :attr:`DEFAULT_MODEL`.
    DEFAULT_DIM = 384

    #: Override the model name. Any fastembed-supported model id is valid.
    ENV_MODEL = "DCC_MCP_EMBED_MODEL"

    #: Override the on-disk model cache directory. When unset, fastembed
    #: writes to its own platform-default cache (typically ``~/.cache/fastembed``).
    ENV_MODEL_DIR = "DCC_MCP_EMBED_MODEL_DIR"

    _INSTALL_HINT = "OnnxEmbedder requires the 'semantic' extra. Install with: pip install 'dcc-mcp-core[semantic]'"

    def __init__(
        self,
        model_name: str | None = None,
        cache_dir: str | None = None,
    ) -> None:
        resolved_name, resolved_cache = self._resolve_config(model_name, cache_dir)
        self._model = self._load_backend(resolved_name, resolved_cache)
        self._model_name = resolved_name
        self._cache_dir = resolved_cache
        self._dim = self._probe_dim()

    @classmethod
    def _resolve_config(
        cls,
        model_name: str | None,
        cache_dir: str | None,
        env: Mapping[str, str] | None = None,
    ) -> tuple[str, str | None]:
        """Compute ``(model_name, cache_dir)`` from args → env → defaults.

        Pulled out into a classmethod so the precedence logic is testable
        without instantiating the embedder (i.e. without requiring the
        ``[semantic]`` extra to be installed).
        """
        environ = env if env is not None else os.environ
        resolved_name = model_name or environ.get(cls.ENV_MODEL) or cls.DEFAULT_MODEL
        resolved_cache = cache_dir or environ.get(cls.ENV_MODEL_DIR) or None
        return resolved_name, resolved_cache

    def _load_backend(self, model_name: str, cache_dir: str | None) -> object:
        """Construct the underlying fastembed ``TextEmbedding`` instance.

        Subclasses may override this to inject a fake backend for tests
        without taking on the ``[semantic]`` extra.
        """
        try:
            from fastembed import TextEmbedding
        except ImportError as exc:
            raise EmbedderError(self._INSTALL_HINT) from exc
        try:
            return TextEmbedding(model_name=model_name, cache_dir=cache_dir)
        except Exception as exc:
            raise EmbedderError(f"failed to load embedding model {model_name!r}: {exc}") from exc

    def _probe_dim(self) -> int:
        """Run a one-off embedding to discover the model's output dimension."""
        try:
            probe = next(iter(self._model.embed(["dimension probe"])))  # type: ignore[attr-defined]
            return len(probe)
        except Exception:
            return self.DEFAULT_DIM

    @property
    def dim(self) -> int:
        return self._dim

    @property
    def model_name(self) -> str:
        return self._model_name

    @property
    def cache_dir(self) -> str | None:
        return self._cache_dir

    def embed(self, text: str) -> array[float]:
        if not text.strip():
            return array("d", (0.0 for _ in range(self._dim)))
        try:
            vec = next(iter(self._model.embed([text])))  # type: ignore[attr-defined]
        except Exception as exc:
            raise EmbedderError(f"OnnxEmbedder.embed failed: {exc}") from exc
        out = array("d", (float(x) for x in vec))
        return _l2_normalise(out)

    def embed_batch(self, texts: Iterable[str]) -> list[array[float]]:
        materialised = list(texts)
        if not materialised:
            return []
        try:
            raw = list(self._model.embed(materialised))  # type: ignore[attr-defined]
        except Exception as exc:
            raise EmbedderError(f"OnnxEmbedder.embed_batch failed: {exc}") from exc
        out: list[array[float]] = []
        for text, vec in zip(materialised, raw):
            if not text.strip():
                out.append(array("d", (0.0 for _ in range(self._dim))))
                continue
            out.append(_l2_normalise(array("d", (float(x) for x in vec))))
        return out
