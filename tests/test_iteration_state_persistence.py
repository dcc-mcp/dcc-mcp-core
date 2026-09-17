"""Iteration state must survive restarts (issue #2300).

Three acceptance criteria, one section each:

1. A DCC restart preserves checkpoints without any adapter opt-in, and
   ``jobs_resume_context`` returns saved state after the restart.
2. Consecutive ``dcc_execute`` calls in one session can share namespace state
   when the caller opts in, with the sandbox restrictions unchanged.
3. A second process start with unchanged skill docs performs zero embedding
   computations.
"""

from __future__ import annotations

import json
from pathlib import Path
from unittest.mock import MagicMock

import pytest

from dcc_mcp_core.checkpoint import CHECKPOINT_FILE_NAME
from dcc_mcp_core.checkpoint import CheckpointStore
from dcc_mcp_core.checkpoint import default_checkpoint_dir
from dcc_mcp_core.checkpoint import default_checkpoint_path
from dcc_mcp_core.checkpoint import register_checkpoint_tools
from dcc_mcp_core.checkpoint import resolve_checkpoint_path
from dcc_mcp_core.constants import ENV_CHECKPOINT_DIR
from dcc_mcp_core.constants import ENV_CHECKPOINT_IN_MEMORY
from dcc_mcp_core.constants import ENV_EMBEDDING_CACHE_DIR
from dcc_mcp_core.skill_index import SkillDocument
from dcc_mcp_core.skill_index import VectorSkillIndex
from dcc_mcp_core.vector_embedder import default_embedding_cache_path

# ── 1. Durable checkpoint default ──────────────────────────────────────────


class TestDurableCheckpointPath:
    def test_default_path_is_durable_and_per_dcc(self, monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
        monkeypatch.setenv(ENV_CHECKPOINT_DIR, str(tmp_path))
        path = default_checkpoint_path("maya")

        assert path.parent == tmp_path / "maya"
        assert path.name == CHECKPOINT_FILE_NAME

    def test_default_path_is_stable_across_restarts(self, monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
        """No per-process segment: the same DCC resolves to the same file."""
        monkeypatch.setenv(ENV_CHECKPOINT_DIR, str(tmp_path))
        assert default_checkpoint_path("maya") == default_checkpoint_path("maya")

    def test_default_dir_falls_back_to_user_profile(self, monkeypatch: pytest.MonkeyPatch) -> None:
        monkeypatch.delenv(ENV_CHECKPOINT_DIR, raising=False)
        assert default_checkpoint_dir() == Path("~").expanduser() / ".dcc-mcp"

    def test_dcc_name_is_sanitised(self, monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
        monkeypatch.setenv(ENV_CHECKPOINT_DIR, str(tmp_path))
        path = default_checkpoint_path("../../etc/passwd")
        assert tmp_path in path.parents

    def test_resolve_defaults_to_durable(self, monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
        monkeypatch.setenv(ENV_CHECKPOINT_DIR, str(tmp_path))
        assert resolve_checkpoint_path("maya") == default_checkpoint_path("maya")

    def test_resolve_honours_explicit_path(self, tmp_path: Path) -> None:
        explicit = tmp_path / "explicit.json"
        assert resolve_checkpoint_path("maya", path=str(explicit)) == explicit

    def test_resolve_in_memory_opt_out(self) -> None:
        assert resolve_checkpoint_path("maya", in_memory=True) is None

    def test_resolve_in_memory_env_opt_out(self, monkeypatch: pytest.MonkeyPatch) -> None:
        monkeypatch.setenv(ENV_CHECKPOINT_IN_MEMORY, "1")
        assert resolve_checkpoint_path("maya") is None

    def test_resolve_reads_env_mapping(self) -> None:
        assert resolve_checkpoint_path("maya", env={ENV_CHECKPOINT_IN_MEMORY: "true"}) is None
        assert resolve_checkpoint_path("maya", env={ENV_CHECKPOINT_IN_MEMORY: "0"}) is not None


class TestCheckpointStoreDurability:
    def test_store_reports_durability(self, tmp_path: Path) -> None:
        durable = CheckpointStore(path=str(tmp_path / "cp.json"))
        assert durable.is_durable is True
        assert durable.path == tmp_path / "cp.json"

        memory = CheckpointStore()
        assert memory.is_durable is False
        assert memory.path is None

    def test_restart_preserves_checkpoint(self, tmp_path: Path) -> None:
        path = tmp_path / "cp.json"
        first = CheckpointStore(path=str(path))
        first.save("job-1", {"count": 41}, progress_hint="41/100")

        # Simulated process restart: a brand new store over the same file.
        second = CheckpointStore(path=str(path))
        restored = second.get("job-1")

        assert restored is not None
        assert restored["context"] == {"count": 41}
        assert restored["progress_hint"] == "41/100"

    def test_flush_is_atomic(self, tmp_path: Path) -> None:
        """A partially written file must never be observable."""
        path = tmp_path / "cp.json"
        store = CheckpointStore(path=str(path))
        store.save("job-1", {"count": 1})
        assert list(path.parent.glob("*.tmp")) == []
        assert json.loads(path.read_text(encoding="utf-8"))["job-1"]["context"] == {"count": 1}


class TestServerCheckpointDefault:
    """``DccServerBase`` must be durable with no adapter opt-in."""

    @staticmethod
    def _build(monkeypatch: pytest.MonkeyPatch, tmp_path: Path, dcc_name: str = "maya", **kwargs):
        from dcc_mcp_core._server.options import DccServerOptions
        from dcc_mcp_core.server_base import DccServerBase

        monkeypatch.setattr("dcc_mcp_core.server_base.create_adapter_server", MagicMock())
        monkeypatch.setattr(
            "dcc_mcp_core._server.skill_discovery.register_all_builtin_skills",
            lambda *args, **kwargs: None,
        )
        skills_dir = tmp_path / "skills"
        skills_dir.mkdir(exist_ok=True)
        options = DccServerOptions.from_env(
            dcc_name,
            skills_dir,
            port=0,
            gateway_port=0,
            enable_file_logging=False,
            enable_job_persistence=False,
            enable_telemetry=False,
            **kwargs,
        )
        return DccServerBase(options)

    def test_server_defaults_to_a_durable_store(self, monkeypatch, tmp_path: Path) -> None:
        monkeypatch.setenv(ENV_CHECKPOINT_DIR, str(tmp_path / "profile"))
        server = self._build(monkeypatch, tmp_path)

        assert server.checkpoint_store.is_durable is True
        assert server.checkpoint_path == str(default_checkpoint_path("maya"))

    def test_checkpoints_survive_a_simulated_restart(self, monkeypatch, tmp_path: Path) -> None:
        monkeypatch.setenv(ENV_CHECKPOINT_DIR, str(tmp_path / "profile"))

        before = self._build(monkeypatch, tmp_path)
        before.checkpoint_store.save("job-1", {"count": 41}, progress_hint="41/100")

        after = self._build(monkeypatch, tmp_path)
        restored = after.checkpoint_store.get("job-1")

        assert restored is not None
        assert restored["context"] == {"count": 41}

    def test_resume_context_returns_state_after_restart(self, monkeypatch, tmp_path: Path) -> None:
        """``jobs_resume_context`` is registered by default and reads the file."""
        monkeypatch.setenv(ENV_CHECKPOINT_DIR, str(tmp_path / "profile"))

        before = self._build(monkeypatch, tmp_path)
        before.checkpoint_store.save("job-1", {"count": 41}, progress_hint="41/100")

        restarted = self._build(monkeypatch, tmp_path)
        server = MagicMock()
        registry = MagicMock()
        server.registry = registry
        handlers: dict = {}
        server.register_handler.side_effect = lambda name, fn: handlers.__setitem__(name, fn)
        register_checkpoint_tools(server, dcc_name="maya", store=restarted.checkpoint_store)

        assert "jobs_resume_context" in handlers
        result = handlers["jobs_resume_context"](json.dumps({"job_id": "job-1"}))

        assert result["success"] is True
        assert result["context"]["has_checkpoint"] is True
        assert result["context"]["resume_state"] == {"count": 41}
        assert result["context"]["progress_hint"] == "41/100"

    def test_registers_checkpoint_tools_by_default(self, monkeypatch, tmp_path: Path) -> None:
        monkeypatch.setenv(ENV_CHECKPOINT_DIR, str(tmp_path / "profile"))
        server = self._build(monkeypatch, tmp_path)
        registered = {call.kwargs.get("name") for call in server._server.registry.register.call_args_list}
        assert "jobs_resume_context" in registered

    def test_in_memory_opt_out_via_options(self, monkeypatch, tmp_path: Path) -> None:
        monkeypatch.setenv(ENV_CHECKPOINT_DIR, str(tmp_path / "profile"))
        server = self._build(monkeypatch, tmp_path, enable_checkpoint_persistence=False)

        assert server.checkpoint_path is None
        assert server.checkpoint_store.is_durable is False

    def test_in_memory_opt_out_via_env(self, monkeypatch, tmp_path: Path) -> None:
        monkeypatch.setenv(ENV_CHECKPOINT_DIR, str(tmp_path / "profile"))
        monkeypatch.setenv(ENV_CHECKPOINT_IN_MEMORY, "1")
        server = self._build(monkeypatch, tmp_path)

        assert server.checkpoint_path is None

    def test_explicit_checkpoint_path_wins(self, monkeypatch, tmp_path: Path) -> None:
        monkeypatch.setenv(ENV_CHECKPOINT_DIR, str(tmp_path / "profile"))
        explicit = tmp_path / "explicit" / "cp.json"
        server = self._build(monkeypatch, tmp_path, checkpoint_path=str(explicit))

        assert server.checkpoint_path == str(explicit)

    def test_checkpoint_tools_can_be_disabled(self, monkeypatch, tmp_path: Path) -> None:
        monkeypatch.setenv(ENV_CHECKPOINT_DIR, str(tmp_path / "profile"))
        server = self._build(monkeypatch, tmp_path, enable_checkpoint_tools=False)
        registered = {call.kwargs.get("name") for call in server._server.registry.register.call_args_list}
        assert "jobs_resume_context" not in registered


# ── 2. ``dcc_execute`` persistent namespace ────────────────────────────────


class TestDccExecutePersistentNamespace:
    @staticmethod
    def _executor(**kwargs):
        """Build a ``dcc_execute`` executor with its own namespace (no test crosstalk)."""
        from dcc_mcp_core.dcc_api_executor import DccApiExecutor
        from dcc_mcp_core.script_execution import ScriptExecutionContext

        kwargs.setdefault("script_execution_context", ScriptExecutionContext())
        return DccApiExecutor("maya", **kwargs)

    def test_consecutive_calls_share_state_when_opted_in(self) -> None:
        executor = self._executor(persistent_namespace=True)

        first = executor.execute_params({"code": "counter = 6\nreturn counter"})
        assert first["success"] is True, first
        assert first["output"] == 6

        second = executor.execute_params({"code": "counter = counter * 2\nreturn counter"})
        assert second["success"] is True, second
        assert second["output"] == 12

    def test_state_does_not_leak_without_opt_in(self) -> None:
        executor = self._executor()

        assert executor.execute_params({"code": "leaked = 1\nreturn leaked"})["output"] == 1
        third = executor.execute_params({"code": "return leaked"})
        assert third["success"] is False

    def test_per_call_opt_in(self) -> None:
        executor = self._executor()

        assert executor.execute_params({"code": "v = 5", "persistent_namespace": True})["success"] is True
        assert executor.execute_params({"code": "return v * 3", "persistent_namespace": True})["output"] == 15

    def test_reported_variables(self) -> None:
        executor = self._executor(persistent_namespace=True)
        result = executor.execute_params({"code": "items = [1, 2, 3]\nreturn len(items)"})

        assert result["context"]["persistent_namespace"]["enabled"] is True
        assert result["context"]["persistent_namespace"]["variables"] == ["items"]

    def test_clear_script_namespace_resets_state(self) -> None:
        from dcc_mcp_core.script_execution import ScriptExecutionContext
        from dcc_mcp_core.script_execution import clear_script_namespace

        context = ScriptExecutionContext()
        executor = self._executor(persistent_namespace=True, script_execution_context=context)
        assert executor.execute_params({"code": "cache = {'a': 1}\nreturn cache"})["success"] is True

        clear_script_namespace(context=context)

        assert executor.execute_params({"code": "return cache"})["success"] is False

    def test_sandbox_restrictions_are_unchanged(self) -> None:
        executor = self._executor(persistent_namespace=True)
        result = executor.execute_params({"code": "return open('C:/tmp/dcc-mcp-should-not-open.txt')"})

        assert result["success"] is False
        assert "open" in result["error"]

    def test_dispatch_is_not_persisted(self) -> None:
        """The sandbox injects ``dispatch``/``json`` per run; they never persist."""
        executor = self._executor(persistent_namespace=True)
        result = executor.execute_params({"code": "dispatch = 1\nreturn dispatch"})

        assert result["success"] is True
        assert result["context"]["persistent_namespace"]["variables"] == []

    def test_nested_scope_locals_do_not_leak(self) -> None:
        executor = self._executor(persistent_namespace=True)
        result = executor.execute_params({"code": "def f():\n    inner = 7\n    return inner\nreturn f()"})

        assert result["success"] is True, result
        assert result["output"] == 7
        assert result["context"]["persistent_namespace"]["variables"] == ["f"]

    def test_schema_and_description_advertise_the_flag(self) -> None:
        from unittest.mock import MagicMock

        from dcc_mcp_core.dcc_api_executor import register_dcc_api_executor

        server = MagicMock()
        register_dcc_api_executor(server, self._executor())

        registered = {call.args[0]: call.kwargs for call in server.registry.register.call_args_list}
        properties = json.loads(registered["dcc_execute"]["input_schema"])["properties"]

        assert properties["persistent_namespace"]["type"] == "boolean"
        assert "persistent_namespace" in registered["dcc_execute"]["description"]

    def test_invalid_flag_value_is_rejected(self) -> None:
        executor = self._executor()
        result = executor.execute_params({"code": "return 1", "persistent_namespace": "maybe"})

        assert result["success"] is False
        assert "invalid" in result["message"].lower()


# ── 3. Embedding warm start ────────────────────────────────────────────────


def _make_docs() -> list[SkillDocument]:
    return [
        SkillDocument(
            skill_id=f"modeling.skill-{index}",
            name=f"Skill {index}",
            summary=f"Does thing number {index} in the scene.",
            tags=("modeling",),
        )
        for index in range(5)
    ]


class TestEmbeddingWarmStart:
    def test_second_start_computes_nothing(self, monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
        monkeypatch.setenv(ENV_EMBEDDING_CACHE_DIR, str(tmp_path))
        docs = _make_docs()

        cold = VectorSkillIndex(cache_path=default_embedding_cache_path("maya"))
        assert cold.index(docs) == 5
        assert cold.embedding_stats is not None
        assert cold.embedding_stats.misses == 5
        assert cold.flush_embedding_cache() is True

        # Simulated second process start: same docs, fresh index object.
        warm = VectorSkillIndex(cache_path=default_embedding_cache_path("maya"))
        warm.embedding_cache.reset_stats()  # type: ignore[union-attr]
        assert warm.index(docs) == 5

        stats = warm.embedding_stats
        assert stats is not None
        assert stats.hits == 5
        assert stats.misses == 0
        assert stats.computes == 0

    def test_warm_start_preserves_search_quality(self, monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
        monkeypatch.setenv(ENV_EMBEDDING_CACHE_DIR, str(tmp_path))
        docs = _make_docs()

        cold = VectorSkillIndex(cache_path=default_embedding_cache_path("maya"))
        cold.index(docs)
        cold.flush_embedding_cache()
        cold_hits = cold.search("skill 3 does thing number", k=1)

        warm = VectorSkillIndex(cache_path=default_embedding_cache_path("maya"))
        warm.index(docs)
        warm_hits = warm.search("skill 3 does thing number", k=1)

        assert cold_hits[0].skill_id == warm_hits[0].skill_id
        assert cold_hits[0].score == pytest.approx(warm_hits[0].score)

    def test_changed_document_is_recomputed(self, monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
        monkeypatch.setenv(ENV_EMBEDDING_CACHE_DIR, str(tmp_path))
        cache_path = default_embedding_cache_path("maya")
        docs = _make_docs()

        VectorSkillIndex(cache_path=cache_path).index(docs)

        edited = list(docs)
        edited[0] = SkillDocument(
            skill_id=edited[0].skill_id,
            name=edited[0].name,
            summary="Completely different summary text.",
            tags=("modeling",),
        )
        second = VectorSkillIndex(cache_path=cache_path)
        second.embedding_cache.reset_stats()  # type: ignore[union-attr]
        second.index(edited)

        stats = second.embedding_stats
        assert stats is not None
        assert stats.hits == 4
        assert stats.misses == 1

    def test_no_cache_path_means_no_persistence(self, tmp_path: Path) -> None:
        index = VectorSkillIndex()
        assert index.embedding_cache is None
        assert index.embedding_stats is None
        assert index.flush_embedding_cache() is False

    def test_embedder_change_invalidates_the_cache(self, monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
        from dcc_mcp_core.vector_embedder import HashedEmbedder
        from dcc_mcp_core.vector_embedder import embedder_fingerprint

        monkeypatch.setenv(ENV_EMBEDDING_CACHE_DIR, str(tmp_path))
        cache_path = default_embedding_cache_path("maya")
        docs = _make_docs()

        assert embedder_fingerprint(HashedEmbedder()) != embedder_fingerprint(HashedEmbedder(dim=64))

        VectorSkillIndex(embedder=HashedEmbedder(), cache_path=cache_path).index(docs)

        swapped = VectorSkillIndex(embedder=HashedEmbedder(dim=64), cache_path=cache_path)
        swapped.embedding_cache.reset_stats()  # type: ignore[union-attr]
        swapped.index(docs)

        stats = swapped.embedding_stats
        assert stats is not None
        assert stats.misses == 5

    def test_corrupt_cache_file_degrades_to_recompute(self, monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
        monkeypatch.setenv(ENV_EMBEDDING_CACHE_DIR, str(tmp_path))
        cache_path = default_embedding_cache_path("maya")
        cache_path.parent.mkdir(parents=True, exist_ok=True)
        cache_path.write_text("not-json", encoding="utf-8")

        index = VectorSkillIndex(cache_path=cache_path)
        assert index.index(_make_docs()) == 5
        assert index.embedding_stats is not None
        assert index.embedding_stats.misses == 5

    def test_default_cache_path_is_per_dcc(self, monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
        monkeypatch.setenv(ENV_EMBEDDING_CACHE_DIR, str(tmp_path))
        assert default_embedding_cache_path("maya") == tmp_path / "maya" / "skill-embeddings.json"

    def test_dcc_name_enables_warm_start(self, monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
        """The one-arg adapter path: ``VectorSkillIndex(dcc_name="maya")``."""
        monkeypatch.setenv(ENV_EMBEDDING_CACHE_DIR, str(tmp_path))
        docs = _make_docs()

        first = VectorSkillIndex(dcc_name="maya")
        assert first.embedding_cache is not None
        first.index(docs)
        first.flush_embedding_cache()

        second = VectorSkillIndex(dcc_name="maya")
        second.embedding_cache.reset_stats()  # type: ignore[union-attr]
        second.index(docs)

        stats = second.embedding_stats
        assert stats is not None
        assert stats.hits == 5
        assert stats.computes == 0
        assert default_embedding_cache_path("maya").exists()
