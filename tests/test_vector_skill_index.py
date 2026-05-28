"""Tests for VectorSkillIndex and its zero-dep default backends (issue #1393)."""

from __future__ import annotations

from array import array
import math

import pytest

from dcc_mcp_core.semantic_skill_index import LexicalSkillIndex
from dcc_mcp_core.semantic_skill_index import RrfFusionIndex
from dcc_mcp_core.semantic_skill_index import SkillDocument
from dcc_mcp_core.vector_embedder import DEFAULT_DIM
from dcc_mcp_core.vector_embedder import Embedder
from dcc_mcp_core.vector_embedder import EmbedderError
from dcc_mcp_core.vector_embedder import HashedEmbedder
from dcc_mcp_core.vector_embedder import OnnxEmbedder
from dcc_mcp_core.vector_skill_index import InMemoryVectorStore
from dcc_mcp_core.vector_skill_index import VectorSkillIndex
from dcc_mcp_core.vector_skill_index import VectorStore


def _l2_norm(vec: array) -> float:
    return math.sqrt(sum(x * x for x in vec))


# ── HashedEmbedder ──────────────────────────────────────────────────────


def test_hashed_embedder_defaults_to_256_dim() -> None:
    emb = HashedEmbedder()
    assert emb.dim == DEFAULT_DIM == 256


def test_hashed_embedder_rejects_invalid_params() -> None:
    with pytest.raises(ValueError):
        HashedEmbedder(dim=0)
    with pytest.raises(ValueError):
        HashedEmbedder(char_n=0)
    with pytest.raises(ValueError):
        HashedEmbedder(token_weight=0.0)
    with pytest.raises(ValueError):
        HashedEmbedder(char_weight=-1.0)


def test_hashed_embedder_is_deterministic() -> None:
    emb = HashedEmbedder()
    v1 = emb.embed("create polygon sphere")
    v2 = emb.embed("create polygon sphere")
    assert list(v1) == list(v2)


def test_hashed_embedder_produces_unit_vector_for_nontrivial_input() -> None:
    emb = HashedEmbedder()
    vec = emb.embed("render the current frame")
    assert pytest.approx(_l2_norm(vec), abs=1e-9) == 1.0


def test_hashed_embedder_returns_zero_vector_on_empty_input() -> None:
    emb = HashedEmbedder()
    for empty in ("", "   ", "\t\n", "!!!"):
        vec = emb.embed(empty)
        assert len(vec) == emb.dim
        assert _l2_norm(vec) == 0.0


def test_hashed_embedder_different_text_gives_different_vectors() -> None:
    emb = HashedEmbedder()
    a = emb.embed("render the frame")
    b = emb.embed("export the scene")
    assert list(a) != list(b)


def test_hashed_embedder_morphology_aware_via_char_ngrams() -> None:
    """``render`` and ``rendering`` share most 3-grams, so their vectors
    should be measurably closer than ``render`` vs an unrelated word.
    """
    emb = HashedEmbedder()
    render = emb.embed("render")
    rendering = emb.embed("rendering")
    unrelated = emb.embed("polygon")
    sim_morph = sum(x * y for x, y in zip(render, rendering))
    sim_unrelated = sum(x * y for x, y in zip(render, unrelated))
    assert sim_morph > sim_unrelated


def test_hashed_embedder_batch_matches_singletons() -> None:
    emb = HashedEmbedder()
    inputs = ["create sphere", "delete cube", ""]
    batch = emb.embed_batch(inputs)
    assert len(batch) == 3
    for got, text in zip(batch, inputs):
        assert list(got) == list(emb.embed(text))


def test_hashed_embedder_no_char_weight_falls_back_to_pure_token_hashing() -> None:
    emb = HashedEmbedder(char_weight=0.0)
    a = emb.embed("render")
    b = emb.embed("rendering")
    # Without char n-grams, no overlap → orthogonal under unbiased hash trick.
    sim = sum(x * y for x, y in zip(a, b))
    assert abs(sim) < 0.1


def test_hashed_embedder_conforms_to_embedder_protocol() -> None:
    assert isinstance(HashedEmbedder(), Embedder)


# ── InMemoryVectorStore ─────────────────────────────────────────────────


def test_in_memory_store_add_len_and_clear() -> None:
    store = InMemoryVectorStore()
    assert len(store) == 0
    store.add("a", array("d", [1.0, 0.0]))
    store.add("b", array("d", [0.0, 1.0]))
    assert len(store) == 2
    store.clear()
    assert len(store) == 0


def test_in_memory_store_remove_returns_existence() -> None:
    store = InMemoryVectorStore()
    store.add("a", array("d", [1.0, 0.0]))
    assert store.remove("a") is True
    assert store.remove("a") is False


def test_in_memory_store_search_ranks_by_cosine() -> None:
    store = InMemoryVectorStore()
    store.add("close", array("d", [0.99, 0.14]))
    store.add("middle", array("d", [0.71, 0.71]))
    store.add("orthogonal", array("d", [0.0, 1.0]))  # dot=0 → filtered out
    hits = store.search(array("d", [1.0, 0.0]), k=3)
    assert [sid for sid, _ in hits] == ["close", "middle"]
    assert hits[0][1] > hits[1][1]


def test_in_memory_store_search_filters_non_positive_cosine() -> None:
    """Orthogonal and opposite vectors are not relevant — drop them."""
    store = InMemoryVectorStore()
    store.add("orth", array("d", [0.0, 1.0]))
    store.add("opposite", array("d", [-1.0, 0.0]))
    hits = store.search(array("d", [1.0, 0.0]), k=5)
    assert hits == []


def test_in_memory_store_search_handles_dim_mismatch_gracefully() -> None:
    store = InMemoryVectorStore()
    store.add("ok", array("d", [1.0, 0.0]))
    store.add("wrong_dim", array("d", [1.0, 0.0, 0.0]))
    hits = store.search(array("d", [1.0, 0.0]), k=5)
    assert [sid for sid, _ in hits] == ["ok"]


def test_in_memory_store_search_k_zero_returns_empty() -> None:
    store = InMemoryVectorStore()
    store.add("a", array("d", [1.0, 0.0]))
    assert store.search(array("d", [1.0, 0.0]), k=0) == []


def test_in_memory_store_conforms_to_vector_store_protocol() -> None:
    assert isinstance(InMemoryVectorStore(), VectorStore)


# ── VectorSkillIndex ────────────────────────────────────────────────────


def _make_docs() -> list[SkillDocument]:
    return [
        SkillDocument(
            skill_id="modeling.polygon-sphere",
            name="Polygon Sphere",
            summary="Create a polygon sphere primitive in the scene.",
            tags=("modeling", "primitive"),
        ),
        SkillDocument(
            skill_id="modeling.bevel",
            name="Bevel Edges",
            summary="Bevel selected edges with adjustable width.",
            tags=("modeling", "edit"),
        ),
        SkillDocument(
            skill_id="rendering.render-current-frame",
            name="Render Current Frame",
            summary="Render the current frame using the active renderer.",
            tags=("rendering", "io"),
        ),
        SkillDocument(
            skill_id="io.export-fbx",
            name="Export FBX",
            summary="Export the current selection to an FBX file.",
            tags=("io", "fbx"),
        ),
    ]


def test_vector_index_default_backends_are_zero_dep() -> None:
    idx = VectorSkillIndex()
    assert isinstance(idx.embedder, HashedEmbedder)
    assert isinstance(idx.store, InMemoryVectorStore)


def test_vector_index_indexes_documents_and_reports_count() -> None:
    idx = VectorSkillIndex()
    docs = _make_docs()
    assert idx.index(docs) == len(docs)
    assert len(idx) == len(docs)


def test_vector_index_search_empty_query_returns_empty() -> None:
    idx = VectorSkillIndex()
    idx.index(_make_docs())
    assert idx.search("") == ()
    assert idx.search("   ") == ()
    assert idx.search("anything", k=0) == ()


def test_vector_index_search_ranks_top_hit_first() -> None:
    idx = VectorSkillIndex()
    idx.index(_make_docs())
    hits = idx.search("render the current frame", k=2)
    assert hits, "expected at least one hit"
    assert hits[0].skill_id == "rendering.render-current-frame"
    assert hits[0].rank == 0
    assert "vec:cosine" in hits[0].match_reasons


def test_vector_index_search_uses_morphology_via_char_ngrams() -> None:
    """Query uses inflected verb (``rendering``) — lexical BM25 would miss
    it on the documents we index (none mention ``rendering`` literally),
    but the char-3-gram component of HashedEmbedder pushes the rendering
    skill to the top.
    """
    idx = VectorSkillIndex()
    idx.index(_make_docs())
    hits = idx.search("rendering frames", k=3)
    assert hits
    top_ids = [hit.skill_id for hit in hits]
    assert "rendering.render-current-frame" in top_ids[:2]


def test_vector_index_reindex_replaces_existing_row() -> None:
    idx = VectorSkillIndex()
    idx.index(
        [
            SkillDocument(skill_id="x", name="initial body", summary="initial body"),
        ]
    )
    idx.index(
        [
            SkillDocument(skill_id="x", name="render export pipeline", summary="render export pipeline"),
        ]
    )
    assert len(idx) == 1
    hits = idx.search("render export", k=1)
    assert hits and hits[0].skill_id == "x"


def test_vector_index_remove_drops_row_from_search() -> None:
    idx = VectorSkillIndex()
    idx.index(_make_docs())
    assert idx.remove("io.export-fbx") is True
    hits = idx.search("export fbx", k=4)
    assert all(hit.skill_id != "io.export-fbx" for hit in hits)


def test_vector_index_accepts_custom_embedder_and_store() -> None:
    """User-supplied collaborators are honoured (Protocol-based DI)."""

    class _StubEmbedder:
        dim = 4

        def embed(self, text: str) -> array:
            v = array("d", [0.0, 0.0, 0.0, 0.0])
            v[len(text) % 4] = 1.0
            return v

        def embed_batch(self, texts):
            return [self.embed(t) for t in texts]

    store = InMemoryVectorStore()
    idx = VectorSkillIndex(embedder=_StubEmbedder(), store=store)
    idx.index([SkillDocument(skill_id="a", name="four", summary="four")])
    assert len(idx) == 1
    assert idx.store is store


# ── OnnxEmbedder gating ────────────────────────────────────────────────


def test_onnx_embedder_raises_when_extra_missing() -> None:
    """When neither backend is installed, the three-tier fallback must
    raise a clear ``EmbedderError`` pointing at the install string. Skip
    when either backend is actually installed so the negative path is not
    silently weakened.
    """
    native_available = False
    fastembed_available = False
    try:
        import dcc_mcp_core_semantic

        native_available = True
    except ImportError:
        pass
    try:
        import fastembed

        fastembed_available = True
    except ImportError:
        pass
    if native_available or fastembed_available:
        pytest.skip(
            "a semantic backend is installed in this venv "
            f"(native={native_available}, python-fastembed={fastembed_available}); "
            "the extra-missing path cannot be exercised here"
        )
    with pytest.raises(EmbedderError) as exc_info:
        OnnxEmbedder()
    assert "pip install 'dcc-mcp-core[semantic]'" in str(exc_info.value)


def test_onnx_embedder_resolves_config_with_defaults() -> None:
    name, cache = OnnxEmbedder._resolve_config(None, None, env={})
    assert name == OnnxEmbedder.DEFAULT_MODEL
    assert cache is None


def test_onnx_embedder_resolves_config_from_env_vars() -> None:
    env = {
        OnnxEmbedder.ENV_MODEL: "BAAI/bge-base-en-v1.5",
        OnnxEmbedder.ENV_MODEL_DIR: "/srv/shared/models",
    }
    name, cache = OnnxEmbedder._resolve_config(None, None, env=env)
    assert name == "BAAI/bge-base-en-v1.5"
    assert cache == "/srv/shared/models"


def test_onnx_embedder_constructor_args_beat_env_vars() -> None:
    """Explicit ``model_name=`` / ``cache_dir=`` must win over env vars,
    so adapter code can pin a specific model regardless of operator settings.
    """
    env = {
        OnnxEmbedder.ENV_MODEL: "ignored-from-env",
        OnnxEmbedder.ENV_MODEL_DIR: "/ignored",
    }
    name, cache = OnnxEmbedder._resolve_config("explicit/model", "/explicit/cache", env=env)
    assert name == "explicit/model"
    assert cache == "/explicit/cache"


def test_onnx_embedder_uses_os_environ_when_env_omitted(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setenv(OnnxEmbedder.ENV_MODEL, "from-os-environ")
    monkeypatch.delenv(OnnxEmbedder.ENV_MODEL_DIR, raising=False)
    name, cache = OnnxEmbedder._resolve_config(None, None)
    assert name == "from-os-environ"
    assert cache is None


class _FakeFastEmbed:
    """Test seam that mimics fastembed's ``TextEmbedding.embed`` shape."""

    def __init__(self, dim: int = 4) -> None:
        self.dim = dim
        self.calls: list[list[str]] = []

    def embed(self, texts):
        materialised = list(texts)
        self.calls.append(materialised)
        for text in materialised:
            # Deterministic toy vector: distribute mass to bucket(len % dim).
            vec = [0.0] * self.dim
            vec[len(text) % self.dim] = 2.0  # unnormalised on purpose
            yield vec


class _StubOnnxEmbedder(OnnxEmbedder):
    """Bypass the fastembed import so the wrapper logic is testable."""

    def __init__(self, fake: _FakeFastEmbed, model_name: str | None = None) -> None:
        self._fake = fake
        super().__init__(model_name=model_name or "fake/model")

    def _load_backend(self, model_name, cache_dir):
        return self._fake


def test_onnx_embedder_wrapper_probes_dim_and_normalises_output() -> None:
    fake = _FakeFastEmbed(dim=8)
    emb = _StubOnnxEmbedder(fake)
    assert emb.dim == 8
    assert emb.model_name == "fake/model"
    vec = emb.embed("hello world")
    # Fake returns [0,0,…,2.0,…,0]; OnnxEmbedder must L2-normalise → unit length.
    assert pytest.approx(math.sqrt(sum(x * x for x in vec)), abs=1e-9) == 1.0
    assert max(vec) == pytest.approx(1.0)


def test_onnx_embedder_empty_input_returns_zero_vector_without_calling_backend() -> None:
    fake = _FakeFastEmbed(dim=4)
    emb = _StubOnnxEmbedder(fake)
    fake.calls.clear()
    vec = emb.embed("   ")
    assert all(x == 0.0 for x in vec)
    # The dim probe in __init__ already populated calls; no further calls for empty input.
    assert all("dimension probe" in batch[0] for batch in fake.calls)


def test_onnx_embedder_embed_batch_normalises_and_zero_pads_empty_rows() -> None:
    fake = _FakeFastEmbed(dim=4)
    emb = _StubOnnxEmbedder(fake)
    batch = emb.embed_batch(["alpha", "", "bravo"])
    assert len(batch) == 3
    assert all(x == 0.0 for x in batch[1])  # empty input → zero vector
    for vec in (batch[0], batch[2]):
        assert pytest.approx(math.sqrt(sum(x * x for x in vec)), abs=1e-9) == 1.0


def test_onnx_embedder_embed_batch_empty_list_returns_empty() -> None:
    fake = _FakeFastEmbed(dim=4)
    emb = _StubOnnxEmbedder(fake)
    assert emb.embed_batch([]) == []


def test_onnx_embedder_propagates_backend_failure_as_embedder_error() -> None:
    class _Boom:
        def embed(self, texts):
            raise RuntimeError("ort kernel failed")

    class _BoomEmbedder(OnnxEmbedder):
        def _load_backend(self, model_name, cache_dir):
            return _Boom()

        def _probe_dim(self) -> int:
            return 8  # skip the probe so __init__ does not raise

    emb = _BoomEmbedder()
    with pytest.raises(EmbedderError, match="ort kernel failed"):
        emb.embed("anything")


# ── Three-tier _load_backend resolution (#1395) ────────────────────────


class _FakeNativeEmbedderModule:
    """In-memory stand-in for ``dcc_mcp_core_semantic.native`` so the
    three-tier resolver tests run without the companion wheel installed.

    Mirrors the public surface of the Rust ``NativeEmbedder`` pyclass:
    ``__init__(model_name=..., cache_dir=...)`` plus ``embed_batch``.
    """

    def __init__(self, *, raise_unknown: bool = False, raise_fatal: bool = False) -> None:
        self.raise_unknown = raise_unknown
        self.raise_fatal = raise_fatal
        self.constructed: list[tuple[str, object]] = []

    def NativeEmbedder(self, model_name, cache_dir=None):
        self.constructed.append((model_name, cache_dir))
        if self.raise_unknown:
            raise RuntimeError(f"unknown embedding model {model_name!r}; supported models: a, b")
        if self.raise_fatal:
            raise RuntimeError("ort model download failed")
        return _FakeNativeEmbedderInstance()


class _FakeNativeEmbedderInstance:
    dim = 4
    model_name = "fake-native"

    def embed_batch(self, texts):
        out = []
        for text in texts:
            vec = [0.0, 0.0, 0.0, 0.0]
            if text:
                vec[len(text) % 4] = 1.0
            out.append(vec)
        return out


def _inject_native(monkeypatch: pytest.MonkeyPatch, fake_native_module) -> None:
    """Install a fake ``dcc_mcp_core_semantic`` package into ``sys.modules``."""
    import sys
    import types

    pkg = types.ModuleType("dcc_mcp_core_semantic")
    pkg.native = fake_native_module
    monkeypatch.setitem(sys.modules, "dcc_mcp_core_semantic", pkg)


def test_load_backend_prefers_native_when_available(monkeypatch: pytest.MonkeyPatch) -> None:
    fake_native = _FakeNativeEmbedderModule()
    _inject_native(monkeypatch, fake_native)
    emb = OnnxEmbedder(model_name="BAAI/bge-small-en-v1.5")
    assert emb.backend_name == "native"
    assert fake_native.constructed == [("BAAI/bge-small-en-v1.5", None)]
    # Wire-shape sanity: embed produces an L2-normalised vector of the
    # backend's dim, through the _NativeBackendAdapter.
    vec = emb.embed("anything")
    assert len(vec) == 4
    assert pytest.approx(math.sqrt(sum(x * x for x in vec)), abs=1e-9) == 1.0


def test_load_backend_falls_back_to_python_when_native_does_not_know_model(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """If the native backend reports the model as unknown, the wrapper must
    fall through to the Python backend rather than failing — the Python
    `fastembed` catalogue is wider than the curated Rust registry.
    """
    fake_native = _FakeNativeEmbedderModule(raise_unknown=True)
    _inject_native(monkeypatch, fake_native)

    fake_python_module = type(
        "_FakeFastembed",
        (),
        {
            "TextEmbedding": lambda model_name, cache_dir=None: _FakeFastEmbed(dim=8),
        },
    )
    import sys

    monkeypatch.setitem(sys.modules, "fastembed", fake_python_module)
    emb = OnnxEmbedder(model_name="some/unsupported-by-native-but-known-to-python")
    assert emb.backend_name == "python-fastembed"
    assert emb.dim == 8


def test_load_backend_native_fatal_error_does_not_silently_fall_back(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Genuine native-backend failures (model download fails, ORT crash)
    must NOT silently degrade to Python — operators need to see the real
    cause.
    """
    fake_native = _FakeNativeEmbedderModule(raise_fatal=True)
    _inject_native(monkeypatch, fake_native)
    with pytest.raises(EmbedderError, match="dcc-mcp-core-semantic native backend failed"):
        OnnxEmbedder()


def test_load_backend_uses_python_fastembed_when_native_not_installed(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The most common case: the Rust companion wheel is not installed but
    the Python ``fastembed`` package is — tier 2 wins.
    """
    import sys

    # Ensure the native module is NOT present even if a previous test injected it.
    monkeypatch.delitem(sys.modules, "dcc_mcp_core_semantic", raising=False)

    fake_python_module = type(
        "_FakeFastembed",
        (),
        {
            "TextEmbedding": lambda model_name, cache_dir=None: _FakeFastEmbed(dim=6),
        },
    )
    monkeypatch.setitem(sys.modules, "fastembed", fake_python_module)
    emb = OnnxEmbedder()
    assert emb.backend_name == "python-fastembed"
    assert emb.dim == 6


# ── Fusion: VectorSkillIndex + LexicalSkillIndex via RrfFusionIndex ────


def test_fusion_promotes_consensus_hits_across_lex_and_vec() -> None:
    """Lexical and vector backends both agree on the FBX skill for an FBX
    query — RRF must promote it to rank 0 with both backends contributing
    to match_reasons.
    """
    docs = _make_docs()
    lex = LexicalSkillIndex()
    vec = VectorSkillIndex()
    fused = RrfFusionIndex().register("lex", lex).register("vec", vec)
    fused.index(docs)
    hits = fused.search("export fbx", k=4)
    assert hits and hits[0].skill_id == "io.export-fbx"
    backends_in_top_reason = set()
    for reason in hits[0].match_reasons:
        backends_in_top_reason.add(reason.split(":", 1)[0])
    assert {"lex", "vec"}.issubset(backends_in_top_reason), (
        f"expected both backends to vote for top hit; got {hits[0].match_reasons!r}"
    )


def test_fusion_recovers_when_lexical_misses_on_morphology() -> None:
    """For queries using inflected forms not present literally in the
    corpus, ``LexicalSkillIndex`` alone returns no hits because BM25 does
    no stemming; the vector backend pulls the right skill in via shared
    character 3-grams, and RRF surfaces it.
    """
    # Synthesize a doc whose only token is the un-inflected verb, so BM25
    # cannot tokenise its way to a match for the inflected query.
    docs = [
        SkillDocument(skill_id="m.bevel", name="bevel", summary="bevel edges"),
        SkillDocument(skill_id="m.chamfer", name="chamfer", summary="chamfer corners"),
    ]
    lex = LexicalSkillIndex()
    vec = VectorSkillIndex()
    fused = RrfFusionIndex().register("lex", lex, weight=0.5).register("vec", vec, weight=1.0)
    fused.index(docs)

    # Query uses an inflected form. BM25 tokenises both query and corpus,
    # but neither stems "bevelling" → "bevel", so lexical returns nothing.
    lex_only = lex.search("bevelling", k=4)
    fused_hits = fused.search("bevelling", k=4)

    assert not any(h.skill_id == "m.bevel" for h in lex_only)
    assert fused_hits and fused_hits[0].skill_id == "m.bevel"
