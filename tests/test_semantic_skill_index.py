"""Tests for the semantic skill index (issue #1333)."""

from __future__ import annotations

import pytest

from dcc_mcp_core import LexicalSkillIndex
from dcc_mcp_core import RrfFusionIndex
from dcc_mcp_core import SkillDocument
from dcc_mcp_core import SkillSearchHit


def _doc(skill_id: str, name: str, *, intent: str = "", summary: str = "", tags=(), aliases=()):
    return SkillDocument(
        skill_id=skill_id,
        name=name,
        intent=intent,
        summary=summary,
        tags=tuple(tags),
        search_aliases=tuple(aliases),
    )


class TestLexicalSkillIndex:
    def test_rejects_invalid_hyperparameters(self) -> None:
        with pytest.raises(ValueError):
            LexicalSkillIndex(k1=0)
        with pytest.raises(ValueError):
            LexicalSkillIndex(b=1.5)

    def test_indexes_and_searches_returns_typed_hit(self) -> None:
        idx = LexicalSkillIndex()
        idx.index(
            [
                _doc("usd-import", "usd_import", intent="import a USD layer"),
                _doc("fbx-import", "fbx_import", intent="import an FBX file"),
            ]
        )
        hits = idx.search("import usd", k=2)
        assert len(hits) >= 1
        assert isinstance(hits[0], SkillSearchHit)
        assert hits[0].skill_id == "usd-import"
        assert any(r.startswith("lex:") for r in hits[0].match_reasons)

    def test_search_returns_empty_for_unknown_terms(self) -> None:
        idx = LexicalSkillIndex()
        idx.index([_doc("a", "alpha")])
        assert idx.search("zzz", k=5) == ()

    def test_search_empty_index_returns_empty(self) -> None:
        idx = LexicalSkillIndex()
        assert idx.search("anything") == ()

    def test_zero_k_returns_empty(self) -> None:
        idx = LexicalSkillIndex()
        idx.index([_doc("a", "alpha")])
        assert idx.search("alpha", k=0) == ()

    def test_reindex_replaces_existing_document(self) -> None:
        idx = LexicalSkillIndex()
        idx.index([_doc("usd", "usd_import", intent="import")])
        idx.index([_doc("usd", "usd_export", intent="export")])
        # Old intent must not match
        hits = idx.search("import", k=5)
        assert hits == ()
        hits2 = idx.search("export", k=5)
        assert hits2[0].skill_id == "usd"

    def test_remove_drops_document(self) -> None:
        idx = LexicalSkillIndex()
        idx.index([_doc("a", "alpha"), _doc("b", "beta")])
        assert idx.remove("a") is True
        assert idx.remove("a") is False
        hits = idx.search("alpha", k=5)
        assert hits == ()

    def test_clear_resets_index(self) -> None:
        idx = LexicalSkillIndex()
        idx.index([_doc("a", "alpha")])
        idx.clear()
        assert len(idx) == 0
        assert idx.search("alpha") == ()

    def test_aliases_and_tags_contribute_to_recall(self) -> None:
        idx = LexicalSkillIndex()
        idx.index([_doc("io", "io_module", tags=("interchange",), aliases=("fbx",))])
        hits = idx.search("interchange", k=3)
        assert hits and hits[0].skill_id == "io"
        hits = idx.search("fbx", k=3)
        assert hits and hits[0].skill_id == "io"

    def test_idf_demotes_extremely_common_terms(self) -> None:
        idx = LexicalSkillIndex()
        idx.index(
            [
                _doc("a", "import_one", intent="import shared"),
                _doc("b", "import_two", intent="import shared"),
                _doc("c", "load_one", intent="load shared"),
            ]
        )
        # "import" matches a + b; "shared" matches all three.
        # BM25 IDF should favour the discriminating term.
        hits = idx.search("import shared", k=3)
        assert hits[0].skill_id in {"a", "b"}


class TestRrfFusionIndex:
    def test_rejects_invalid_rrf_k(self) -> None:
        with pytest.raises(ValueError):
            RrfFusionIndex(rrf_k=0)

    def test_register_rejects_empty_name(self) -> None:
        f = RrfFusionIndex()
        with pytest.raises(ValueError):
            f.register("", LexicalSkillIndex())

    def test_register_rejects_non_positive_weight(self) -> None:
        f = RrfFusionIndex()
        with pytest.raises(ValueError):
            f.register("lex", LexicalSkillIndex(), weight=0.0)

    def test_search_with_no_backends_returns_empty(self) -> None:
        f = RrfFusionIndex()
        f.index([_doc("a", "alpha")])
        assert f.search("alpha") == ()

    def test_index_fans_out_to_backends(self) -> None:
        lex_a = LexicalSkillIndex()
        lex_b = LexicalSkillIndex()
        f = RrfFusionIndex().register("a", lex_a).register("b", lex_b)
        added = f.index([_doc("x", "alpha"), _doc("y", "beta")])
        assert added == 2
        assert len(lex_a) == 2
        assert len(lex_b) == 2

    def test_fusion_promotes_doc_ranked_high_by_multiple_backends(self) -> None:
        lex_a = LexicalSkillIndex()
        lex_b = LexicalSkillIndex()
        docs = [
            _doc("usd", "usd_import", intent="import a usd layer"),
            _doc("fbx", "fbx_import", intent="import an fbx file"),
        ]
        lex_a.index(docs)
        lex_b.index(docs)
        f = RrfFusionIndex().register("a", lex_a).register("b", lex_b)
        hits = f.search("import usd", k=2)
        assert hits[0].skill_id == "usd"
        # Match reasons must carry both backend tags
        names = {r.split(":")[0] for r in hits[0].match_reasons}
        assert names == {"a", "b"}

    def test_clear_fans_out_to_backends(self) -> None:
        lex_a = LexicalSkillIndex()
        lex_a.index([_doc("a", "alpha")])
        f = RrfFusionIndex().register("a", lex_a)
        f.clear()
        assert len(lex_a) == 0
