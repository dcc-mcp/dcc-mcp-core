from __future__ import annotations

import json
from threading import Event

import pytest

from dcc_mcp_core import MemoryProvider
from dcc_mcp_core.runtime.memory_provider import LocalMemoryProvider
from dcc_mcp_core.runtime.memory_provider import MemoryCoordinator
from dcc_mcp_core.runtime.memory_provider import MemoryForgetRequest
from dcc_mcp_core.runtime.memory_provider import MemoryHealth
from dcc_mcp_core.runtime.memory_provider import MemoryProviderStatus
from dcc_mcp_core.runtime.memory_provider import MemoryRecallRequest
from dcc_mcp_core.runtime.memory_provider import MemoryRecord
from dcc_mcp_core.runtime.memory_provider import MemoryRememberRequest
from dcc_mcp_core.runtime.memory_provider import NoopMemoryProvider
from dcc_mcp_core.runtime.memory_provider import discover_memory_provider_factories


def _record() -> MemoryRecord:
    return MemoryRecord(
        record_id="decision-1",
        scope="project",
        kind="decision",
        content="Use centimetres for Maya exports.",
        source="adapter:lifecycle",
        metadata={"dcc_name": "maya", "confidence": 0.9},
    )


def test_memory_schemas_are_json_safe() -> None:
    record = _record()
    assert json.loads(json.dumps(record.to_dict()))["record_id"] == "decision-1"

    with pytest.raises(TypeError, match="JSON-safe"):
        MemoryRecord(
            record_id="bad",
            scope="project",
            kind="fact",
            content="bad metadata",
            source="test",
            metadata={"opaque": object()},
        )


def test_noop_provider_has_explicit_operations() -> None:
    provider = NoopMemoryProvider()
    assert provider.recall(MemoryRecallRequest(query="units")) == ()
    assert provider.remember(MemoryRememberRequest(records=(_record(),))).accepted == 0
    assert provider.forget(MemoryForgetRequest(record_ids=("decision-1",))).deleted == 0
    assert provider.health() == MemoryHealth(status=MemoryProviderStatus.DISABLED, provider="noop")


def test_local_provider_round_trip_and_forget(tmp_path) -> None:
    provider = LocalMemoryProvider(tmp_path / "provider.sqlite")
    receipt = provider.remember(MemoryRememberRequest(records=(_record(),)))
    assert receipt.accepted == 1
    assert provider.recall(MemoryRecallRequest(query="centimetres", scope="project")) == (_record(),)
    assert provider.forget(MemoryForgetRequest(record_ids=("decision-1",))).deleted == 1
    assert provider.recall(MemoryRecallRequest(query="centimetres")) == ()


def test_coordinator_runs_provider_work_off_caller_thread() -> None:
    caller_started = Event()
    release_provider = Event()

    class BlockingProvider(NoopMemoryProvider):
        def recall(self, request):
            caller_started.set()
            release_provider.wait(timeout=2)
            return ()

    coordinator = MemoryCoordinator(BlockingProvider(), timeout_secs=1)
    future = coordinator.recall(MemoryRecallRequest(query="anything"))
    assert caller_started.wait(timeout=1)
    assert not future.done()
    release_provider.set()
    assert future.result(timeout=1) == ()
    coordinator.close()


def test_coordinator_bounds_a_stalled_provider() -> None:
    release_provider = Event()

    class StalledProvider(NoopMemoryProvider):
        def recall(self, request):
            release_provider.wait(timeout=1)
            return ()

    coordinator = MemoryCoordinator(StalledProvider(), timeout_secs=0.02)
    future = coordinator.recall(MemoryRecallRequest(query="anything"))
    with pytest.raises(TimeoutError, match="memory provider timed out"):
        future.result(timeout=0.5)
    release_provider.set()
    coordinator.close()


def test_entry_point_discovery_is_lazy_and_group_scoped() -> None:
    class EntryPoint:
        name = "memcode"

        def load(self):
            return lambda **_kwargs: NoopMemoryProvider()

    seen = []

    def entry_points(*, group):
        seen.append(group)
        return [EntryPoint()]

    factories = discover_memory_provider_factories(entry_points=entry_points)
    assert seen == ["dcc_mcp.memory_providers"]
    assert factories["memcode"]().health().status is MemoryProviderStatus.DISABLED


def test_provider_contract_is_available_from_public_package() -> None:
    assert MemoryProvider.__name__ == "MemoryProvider"
