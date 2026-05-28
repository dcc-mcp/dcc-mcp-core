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
    """The optional [semantic] extra is not installed in the test venv; the
    constructor must raise a clear ``EmbedderError`` pointing at the install
    string. If the extra ever lands in the dev venv, skip the test instead of
    silently weakening it.
    """
    try:
        import fastembed
    except ImportError:
        pass
    else:
        pytest.skip("fastembed installed — extra-missing path cannot be exercised here")
    with pytest.raises(EmbedderError) as exc_info:
        OnnxEmbedder()
    assert "pip install 'dcc-mcp-core[semantic]'" in str(exc_info.value)


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
