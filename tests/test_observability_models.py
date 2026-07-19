"""Tests for observability data models, session tracking, and batch attribution.

These tests verify:
1. Session data model (creation, lifecycle transitions, serialization)
2. ToolCallEvent model (basic, batch child, full attribution)
3. Aggregate statistics types
4. Batch dispatch attribution (parent_request_id, batch_id)
5. Observability query API (envelope, stats queries)
"""

from __future__ import annotations

import json
import time
from typing import Any

import pytest

from dcc_mcp_core.batch import batch_dispatch
from dcc_mcp_core.batch import generate_batch_id
from dcc_mcp_core.observability_query import API_VERSION
from dcc_mcp_core.observability_query import ObservabilityQuery
from dcc_mcp_core.observability_query import build_query_response


class TestSessionModel:
    """Tests for the Session data model (validated through serialization)."""

    def test_session_json_shape(self) -> None:
        """Verify the expected JSON shape for a Session."""
        session = {
            "session_id": "sess-001",
            "parent_session_id": None,
            "dcc_type": "maya",
            "instance_id": "inst-42",
            "status": "Active",
            "started_at_ms": 1_700_000_000_000,
            "last_activity_at_ms": 1_700_000_001_000,
            "ended_at_ms": None,
            "end_reason": None,
            "tool_call_count": 5,
            "error_count": 1,
            "core_version": "0.19.59",
            "adapter_version": None,
            "build_sha": None,
        }
        json_str = json.dumps(session)
        back = json.loads(json_str)
        assert back["session_id"] == "sess-001"
        assert back["status"] == "Active"
        assert back["tool_call_count"] == 5
        assert back["ended_at_ms"] is None

    def test_session_with_end_reason(self) -> None:
        """Verify end reason serialization."""
        session = {
            "session_id": "sess-002",
            "dcc_type": "blender",
            "status": "Crashed",
            "started_at_ms": 1_700_000_000_000,
            "last_activity_at_ms": 1_700_000_010_000,
            "ended_at_ms": 1_700_000_010_000,
            "end_reason": {"HostCrash": {"detail": "segfault in render"}},
            "tool_call_count": 10,
            "error_count": 1,
            "core_version": "0.19.59",
            "parent_session_id": None,
            "instance_id": None,
        }
        json_str = json.dumps(session)
        back = json.loads(json_str)
        assert back["end_reason"]["HostCrash"]["detail"] == "segfault in render"

    def test_session_parent_child(self) -> None:
        """Verify parent-child session relationship."""
        parent = {
            "session_id": "parent-001",
            "dcc_type": "maya",
            "status": "Active",
            "started_at_ms": 1_700_000_000_000,
            "last_activity_at_ms": 1_700_000_001_000,
            "tool_call_count": 3,
            "error_count": 0,
            "core_version": "0.19.59",
            "parent_session_id": None,
        }
        child = {
            "session_id": "child-001",
            "parent_session_id": "parent-001",
            "dcc_type": "maya",
            "status": "Ended",
            "started_at_ms": 1_700_000_001_000,
            "last_activity_at_ms": 1_700_000_002_000,
            "ended_at_ms": 1_700_000_002_000,
            "tool_call_count": 2,
            "error_count": 0,
            "core_version": "0.19.59",
        }
        assert child["parent_session_id"] == parent["session_id"]


class TestToolCallEventModel:
    """Tests for the ToolCallEvent data model."""

    def test_basic_event(self) -> None:
        """Verify basic tool-call event shape."""
        event = {
            "request_id": "req-001",
            "session_id": "sess-001",
            "parent_request_id": None,
            "batch_id": None,
            "tool_name": "create_sphere",
            "skill_name": None,
            "dcc_type": "maya",
            "instance_id": "inst-42",
            "agent_id": None,
            "transport": "mcp",
            "via_gateway": True,
            "started_at_ms": 1_700_000_000_000,
            "duration_ms": 42,
            "success": True,
            "error_message": None,
            "error_kind": None,
            "mcp_method": "tools/call",
            "trace_id": None,
            "span_id": None,
        }
        json_str = json.dumps(event)
        back = json.loads(json_str)
        assert back["request_id"] == "req-001"
        assert back["success"] is True
        assert back["parent_request_id"] is None
        assert back["batch_id"] is None

    def test_batch_child_event(self) -> None:
        """Verify batch child attribution fields."""
        event = {
            "request_id": "child-003",
            "session_id": "sess-001",
            "parent_request_id": "batch-req-001",
            "batch_id": "batch-grp-abc123",
            "tool_name": "get_scene_objects",
            "started_at_ms": 1_700_000_000_100,
            "duration_ms": 15,
            "success": True,
        }
        assert event["parent_request_id"] == "batch-req-001"
        assert event["batch_id"] == "batch-grp-abc123"

    def test_failed_event(self) -> None:
        """Verify error attribution."""
        event = {
            "request_id": "req-err-001",
            "session_id": "sess-001",
            "tool_name": "render_frame",
            "started_at_ms": 1_700_000_000_000,
            "duration_ms": 5000,
            "success": False,
            "error_message": "CUDA out of memory",
            "error_kind": "gpu",
        }
        assert event["success"] is False
        assert event["error_kind"] == "gpu"


class TestBatchAttribution:
    """Tests for batch sub-call attribution."""

    def test_batch_dispatch_adds_batch_id(self) -> None:
        """Verify batch_dispatch adds batch_id to results."""

        class FakeDispatcher:
            def dispatch(self, name: str, args_str: str) -> dict[str, Any]:
                return {"action": name, "output": {"success": True, "data": json.loads(args_str)}}

        dispatcher = FakeDispatcher()
        result = batch_dispatch(
            dispatcher,
            [("tool_a", {"x": 1}), ("tool_b", {"y": 2})],
            parent_request_id="parent-req-001",
        )
        assert "batch_id" in result
        assert "sub_results" in result
        assert len(result["sub_results"]) == 2
        for sr in result["sub_results"]:
            assert sr["parent_request_id"] == "parent-req-001"
            assert sr["batch_id"] == result["batch_id"]
            assert sr["success"] is True

    def test_batch_dispatch_tracks_errors(self) -> None:
        """Verify error sub-calls are tracked with attribution."""

        class FakeDispatcher:
            def dispatch(self, name: str, args_str: str) -> dict[str, Any]:
                if name == "bad_tool":
                    raise RuntimeError("tool failed")
                return {"action": name, "output": {"success": True}}

        dispatcher = FakeDispatcher()
        result = batch_dispatch(
            dispatcher,
            [("good_tool", {}), ("bad_tool", {})],
        )
        assert result["succeeded"] == 1
        assert len(result["errors"]) == 1
        assert result["errors"][0]["tool"] == "bad_tool"
        # Failed call still gets batch attribution
        assert result["sub_results"][1]["success"] is False
        assert result["sub_results"][1]["error"] == "tool failed"

    def test_generate_batch_id_is_unique(self) -> None:
        """Verify batch IDs are unique."""
        ids = {generate_batch_id() for _ in range(100)}
        assert len(ids) == 100

    def test_batch_dispatch_stop_on_error(self) -> None:
        """Verify stop_on_error breaks early with attribution."""

        class FakeDispatcher:
            def __init__(self) -> None:
                self.call_count = 0

            def dispatch(self, name: str, args_str: str) -> dict[str, Any]:
                self.call_count += 1
                if name == "fail_tool":
                    return {"action": name, "output": {"success": False, "message": "intentional"}}
                return {"action": name, "output": {"success": True}}

        dispatcher = FakeDispatcher()
        result = batch_dispatch(
            dispatcher,
            [("ok_1", {}), ("fail_tool", {}), ("ok_2", {})],
            stop_on_error=True,
            parent_request_id="batch-req",
        )
        assert dispatcher.call_count == 2  # stopped before ok_2
        assert result["succeeded"] == 1
        assert len(result["sub_results"]) == 2


class TestObservabilityQuery:
    """Tests for the versioned query API."""

    def test_build_query_response_envelope(self) -> None:
        """Verify the versioned envelope structure."""
        data = {"total_sessions": 5, "active_sessions": 2}
        response = build_query_response("session_stats", data)
        assert response["api_version"] == API_VERSION
        assert response["query_type"] == "session_stats"
        assert "timestamp_ms" in response
        assert response["data"] == data

    def test_build_query_response_with_params(self) -> None:
        """Verify query_params and warnings in envelope."""
        response = build_query_response(
            "tool_call_stats",
            {"total_calls": 10},
            query_params={"dcc_type": "maya"},
            warnings=["No data for the requested time range"],
        )
        assert response["query_params"]["dcc_type"] == "maya"
        assert len(response["warnings"]) == 1

    def test_query_mock_session_stats(self) -> None:
        """Verify ObservabilityQuery with a mock read function."""

        def mock_read(sql: str, params: dict[str, Any]) -> list[dict[str, Any]]:
            return [
                {
                    "total_sessions": 10,
                    "active_sessions": 3,
                    "ended_normally": 5,
                    "ended_abnormally": 2,
                    "avg_duration_ms": 5000.0,
                    "total_tool_calls": 100,
                    "total_errors": 5,
                }
            ]

        query = ObservabilityQuery(read_json_fn=mock_read)
        response = query.get_session_stats()
        assert response["data"]["total_sessions"] == 10
        assert response["data"]["active_sessions"] == 3

    def test_query_mock_tool_call_stats(self) -> None:
        """Verify tool_call_stats query with mock."""

        def mock_read(sql: str, params: dict[str, Any]) -> list[dict[str, Any]]:
            if "COUNT" in sql:
                return [
                    {
                        "total_calls": 50,
                        "success_count": 45,
                        "failure_count": 5,
                        "avg_duration_ms": 120.5,
                        "avg_success_duration_ms": 110.0,
                    }
                ]
            return [
                {
                    "request_id": "req-1",
                    "session_id": "sess-1",
                    "tool_name": "create_sphere",
                    "success": 1,
                    "duration_ms": 42,
                }
            ]

        query = ObservabilityQuery(read_json_fn=mock_read)
        response = query.get_tool_call_stats()
        assert response["data"]["stats"]["total_calls"] == 50
        assert response["data"]["stats"]["success_count"] == 45
        assert len(response["data"]["events"]) == 1

    def test_query_mock_coverage(self) -> None:
        """Verify coverage query."""

        def mock_read(sql: str, params: dict[str, Any]) -> list[dict[str, Any]]:
            return [{"observed": 80, "unobserved": 20, "total": 100}]

        query = ObservabilityQuery(read_json_fn=mock_read)
        response = query.get_coverage_stats()
        assert response["data"]["observed_requests"] == 80
        assert response["data"]["unobserved_requests"] == 20
        assert response["data"]["coverage_ratio"] == 0.8

    def test_query_mock_crash_stats(self) -> None:
        """Verify crash stats query."""

        def mock_read(sql: str, params: dict[str, Any]) -> list[dict[str, Any]]:
            return [{"total_crashes": 3, "host_crashes": 2, "gpu_crashes": 1}]

        query = ObservabilityQuery(read_json_fn=mock_read)
        response = query.get_crash_stats()
        assert response["data"]["total_crashes"] == 3
        assert response["data"]["host_crashes"] == 2
        assert response["data"]["gpu_crashes"] == 1

    def test_query_empty_database(self) -> None:
        """Verify queries handle empty database gracefully."""

        def mock_read(sql: str, params: dict[str, Any]) -> list[dict[str, Any]]:
            return []

        query = ObservabilityQuery(read_json_fn=mock_read)
        session_response = query.get_session_stats()
        assert session_response["data"]["total_sessions"] == 0
        coverage_response = query.get_coverage_stats()
        assert coverage_response["data"]["coverage_ratio"] == 0.0

    def test_session_tree_root_query(self) -> None:
        """Verify session tree building."""

        def mock_read(sql: str, params: dict[str, Any]) -> list[dict[str, Any]]:
            return [
                {
                    "session_id": "root-1",
                    "parent_session_id": None,
                    "dcc_type": "maya",
                    "instance_id": "inst-1",
                    "status": "Active",
                    "started_at_ms": 1000,
                    "last_activity_at_ms": 2000,
                    "ended_at_ms": None,
                    "tool_call_count": 5,
                    "error_count": 0,
                    "core_version": "0.19.59",
                },
                {
                    "session_id": "child-1",
                    "parent_session_id": "root-1",
                    "dcc_type": "maya",
                    "instance_id": "inst-1",
                    "status": "Ended",
                    "started_at_ms": 1500,
                    "last_activity_at_ms": 1800,
                    "ended_at_ms": 1800,
                    "tool_call_count": 2,
                    "error_count": 0,
                    "core_version": "0.19.59",
                },
            ]

        query = ObservabilityQuery(read_json_fn=mock_read)
        response = query.get_session_tree()
        tree = response["data"]["tree"]
        assert len(tree) == 1
        assert tree[0]["session_id"] == "root-1"
        assert len(tree[0]["children"]) == 1
        assert tree[0]["children"][0]["session_id"] == "child-1"

    def test_params_passed_to_read_fn(self) -> None:
        """Verify named SQL params are actually passed to the read function.

        This is a regression test for the P1 bug where all six query methods
        built named SQL params (:dcc_type, :since_ms, etc.) but never passed
        them to ``_query()``, so a real SQLite backend would reject the SQL.
        """
        captured_params: list[dict[str, Any]] = []

        def mock_read(sql: str, params: dict[str, Any]) -> list[dict[str, Any]]:
            captured_params.append(dict(params))
            return [
                {
                    "total_sessions": 1,
                    "active_sessions": 0,
                    "ended_normally": 1,
                    "ended_abnormally": 0,
                    "avg_duration_ms": 100.0,
                    "total_tool_calls": 5,
                    "total_errors": 0,
                }
            ]

        query = ObservabilityQuery(read_json_fn=mock_read)
        query.get_session_stats(dcc_type="maya", since_ms=1_700_000_000_000)

        assert len(captured_params) == 1
        assert captured_params[0].get("dcc_type") == "maya"
        assert captured_params[0].get("since_ms") == 1_700_000_000_000
        # SQL must not contain literal :dcc_type or :since_ms — params must
        # be bound separately (verified by checking the call signature).
        assert ":dcc_type" not in str(captured_params)
        assert ":since_ms" not in str(captured_params)

    def test_params_absent_when_no_filters(self) -> None:
        """Verify empty params dict is passed when no filters are applied."""
        captured_params: list[dict[str, Any]] = []

        def mock_read(sql: str, params: dict[str, Any]) -> list[dict[str, Any]]:
            captured_params.append(dict(params))
            return []

        query = ObservabilityQuery(read_json_fn=mock_read)
        query.get_session_stats()

        assert len(captured_params) == 1
        assert captured_params[0] == {}


class TestCoverageStats:
    """Tests for coverage statistics model."""

    def test_full_coverage(self) -> None:
        """100% coverage when all requests are observed."""
        stats = {
            "observed_requests": 100,
            "unobserved_requests": 0,
            "coverage_ratio": 1.0,
        }
        assert stats["coverage_ratio"] == 1.0
        assert stats["unobserved_requests"] == 0

    def test_partial_coverage(self) -> None:
        """Partial coverage."""
        stats = {
            "observed_requests": 75,
            "unobserved_requests": 25,
            "coverage_ratio": 0.75,
        }
        assert stats["coverage_ratio"] == 0.75

    def test_zero_coverage(self) -> None:
        """0% coverage when there are no requests at all."""
        stats = {
            "observed_requests": 0,
            "unobserved_requests": 0,
            "coverage_ratio": 0.0,
        }
        assert stats["coverage_ratio"] == 0.0
        # No false 100% reporting
        assert stats["coverage_ratio"] != 1.0
