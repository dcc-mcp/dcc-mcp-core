"""Tests for agent feedback and rationale utilities (issues #433, #434).

Covers:
- extract_rationale: extracts _meta.dcc.rationale from params dict
- extract_rationale: returns None when missing or malformed
- make_rationale_meta: builds correct _meta fragment
- register_feedback_tool: registers tool on a mock server
- get_feedback_entries: returns stored entries
- get_feedback_entries: filter by tool_name and severity
- clear_feedback: empties the store
- Feedback entries are capped at MAX_FEEDBACK_ENTRIES
- Public API importable from top-level dcc_mcp_core
"""

from __future__ import annotations

import json
from unittest.mock import MagicMock
import urllib.error

import pytest

# ── extract_rationale ──────────────────────────────────────────────────────


def test_extract_rationale_from_dict():
    from dcc_mcp_core.feedback import extract_rationale

    params = {
        "name": "create_sphere",
        "arguments": {"radius": 1.0},
        "_meta": {"dcc": {"rationale": "User wants a reference sphere."}},
    }
    assert extract_rationale(params) == "User wants a reference sphere."


def test_extract_rationale_from_json_string():
    from dcc_mcp_core.feedback import extract_rationale

    params_str = json.dumps({"_meta": {"dcc": {"rationale": "Scale check"}}})
    assert extract_rationale(params_str) == "Scale check"


def test_extract_rationale_missing_returns_none():
    from dcc_mcp_core.feedback import extract_rationale

    assert extract_rationale({}) is None
    assert extract_rationale({"_meta": {}}) is None
    assert extract_rationale({"_meta": {"dcc": {}}}) is None


def test_extract_rationale_invalid_json_returns_none():
    from dcc_mcp_core.feedback import extract_rationale

    assert extract_rationale("not json") is None
    assert extract_rationale(None) is None


# ── make_rationale_meta ────────────────────────────────────────────────────


def test_make_rationale_meta_structure():
    from dcc_mcp_core.feedback import make_rationale_meta

    meta = make_rationale_meta("Creating a sphere for scale reference.")
    assert meta == {"_meta": {"dcc": {"rationale": "Creating a sphere for scale reference."}}}


def test_make_rationale_meta_round_trip():
    from dcc_mcp_core.feedback import extract_rationale
    from dcc_mcp_core.feedback import make_rationale_meta

    meta = make_rationale_meta("Test intent")
    assert extract_rationale(meta) == "Test intent"


# ── feedback store ─────────────────────────────────────────────────────────


def setup_function():
    """Clear feedback store before each test."""
    from dcc_mcp_core.feedback import reset_default_feedback_store_for_tests

    reset_default_feedback_store_for_tests()


def test_feedback_stores_are_isolated() -> None:
    from dcc_mcp_core.feedback import FeedbackStore
    from dcc_mcp_core.feedback import _handle_feedback_report
    from dcc_mcp_core.feedback import get_feedback_entries

    maya = FeedbackStore()
    blender = FeedbackStore()
    payload = json.dumps(
        {
            "tool_name": "maya_geometry__create_sphere",
            "intent": "Create a sphere",
            "blocker": "No active scene",
            "severity": "blocked",
        }
    )

    _handle_feedback_report(payload, store=maya)

    assert len(get_feedback_entries(store=maya)) == 1
    assert get_feedback_entries(store=blender) == []


def test_feedback_store_flushes_each_append_to_bounded_jsonl(tmp_path) -> None:
    from dcc_mcp_core.feedback import FeedbackStore

    path = tmp_path / "feedback" / "maya-4242.jsonl"
    store = FeedbackStore(path=path, max_bytes=4096, backup_count=2)
    entry = {
        "id": "feedback-1",
        "timestamp": 1.0,
        "tool_name": "maya_scene__save",
        "severity": "blocked",
    }

    store.append(entry)

    assert store.path == path
    assert [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines()] == [entry]


def test_feedback_store_rotates_and_prunes_jsonl_backups(tmp_path) -> None:
    from dcc_mcp_core.feedback import FeedbackStore

    path = tmp_path / "feedback" / "houdini-5252.jsonl"
    store = FeedbackStore(path=path, max_bytes=220, backup_count=2)
    for index in range(8):
        store.append(
            {
                "id": f"feedback-{index}",
                "tool_name": "houdini_scene__save",
                "blocker": "x" * 72,
                "severity": "blocked",
            }
        )

    files = sorted(path.parent.glob(f"{path.name}*"))
    assert files == [path, path.with_name(f"{path.name}.1"), path.with_name(f"{path.name}.2")]
    persisted = [json.loads(line) for file in files for line in file.read_text(encoding="utf-8").splitlines()]
    assert all(isinstance(entry, dict) for entry in persisted)
    assert {entry["id"] for entry in persisted} < {f"feedback-{index}" for index in range(8)}
    assert json.loads(path.read_text(encoding="utf-8"))["id"] == "feedback-7"


def test_feedback_store_fails_closed_when_one_entry_exceeds_bound(tmp_path) -> None:
    from dcc_mcp_core.feedback import FeedbackPersistenceError
    from dcc_mcp_core.feedback import FeedbackStore

    path = tmp_path / "feedback" / "blender-6262.jsonl"
    store = FeedbackStore(path=path, max_bytes=64, backup_count=2)

    with pytest.raises(FeedbackPersistenceError, match="exceeds max_bytes"):
        store.append({"id": "feedback-large", "blocker": "x" * 128})

    assert store.recent() == []
    assert not path.exists()


def test_feedback_report_and_retrieve():
    from dcc_mcp_core.feedback import _handle_feedback_report
    from dcc_mcp_core.feedback import get_feedback_entries

    params = json.dumps(
        {
            "tool_name": "maya_geometry__create_sphere",
            "intent": "Create a sphere",
            "attempt": "radius=1.0",
            "blocker": "Sphere not visible",
            "severity": "blocked",
        }
    )
    result = json.loads(_handle_feedback_report(params))
    assert result["success"] is True
    feedback_id = result["context"]["feedback_id"]

    entries = get_feedback_entries()
    assert len(entries) == 1
    assert entries[0]["id"] == feedback_id
    assert entries[0]["tool_name"] == "maya_geometry__create_sphere"
    assert entries[0]["severity"] == "blocked"


def test_filter_by_tool_name():
    from dcc_mcp_core.feedback import _handle_feedback_report
    from dcc_mcp_core.feedback import get_feedback_entries

    for tool in ["tool_a", "tool_b", "tool_a"]:
        _handle_feedback_report(
            json.dumps(
                {
                    "tool_name": tool,
                    "intent": "intent",
                    "blocker": "blocker",
                    "severity": "blocked",
                }
            )
        )

    assert len(get_feedback_entries(tool_name="tool_a")) == 2
    assert len(get_feedback_entries(tool_name="tool_b")) == 1


def test_filter_by_severity():
    from dcc_mcp_core.feedback import _handle_feedback_report
    from dcc_mcp_core.feedback import get_feedback_entries

    for severity in ["blocked", "suggestion", "blocked"]:
        _handle_feedback_report(
            json.dumps(
                {
                    "tool_name": "t",
                    "intent": "i",
                    "blocker": "b",
                    "severity": severity,
                }
            )
        )

    assert len(get_feedback_entries(severity="blocked")) == 2
    assert len(get_feedback_entries(severity="suggestion")) == 1


def test_clear_feedback_returns_count():
    from dcc_mcp_core.feedback import _handle_feedback_report
    from dcc_mcp_core.feedback import clear_feedback

    for _ in range(3):
        _handle_feedback_report(
            json.dumps(
                {
                    "tool_name": "t",
                    "intent": "i",
                    "blocker": "b",
                    "severity": "blocked",
                }
            )
        )
    count = clear_feedback()
    assert count == 3


def test_feedback_invalid_params():
    from dcc_mcp_core.feedback import _handle_feedback_report

    result = json.loads(_handle_feedback_report("not valid json {"))
    assert result["success"] is False


# ── register_feedback_tool ────────────────────────────────────────────────


@pytest.mark.parametrize(
    ("host", "expected"),
    [
        ("0.0.0.0", "http://127.0.0.1:19765/v1/feedback"),
        ("::", "http://[::1]:19765/v1/feedback"),
        ("::1", "http://[::1]:19765/v1/feedback"),
    ],
)
def test_gateway_feedback_endpoint_uses_a_connectable_loopback(host, expected):
    from dcc_mcp_core.feedback import _build_gateway_feedback_endpoint

    assert _build_gateway_feedback_endpoint(gateway_host=host, gateway_port=19765) == expected


def test_register_feedback_tool_registers_name():
    from dcc_mcp_core.feedback import register_feedback_tool

    registry = MagicMock()
    server = MagicMock()
    server.registry = registry

    register_feedback_tool(server, dcc_name="maya")

    registry.register.assert_called_once()
    call_kwargs = registry.register.call_args.kwargs
    assert call_kwargs["name"] == "dcc_feedback__report"
    assert call_kwargs["dcc"] == "maya"
    assert server.register_handler.call_count == 1
    name_arg = server.register_handler.call_args[0][0]
    assert name_arg == "dcc_feedback__report"


def test_registered_feedback_handler_forwards_a_complete_finding_v1(monkeypatch):
    import dcc_mcp_core.feedback as feedback_module
    from dcc_mcp_core.feedback import FeedbackStore
    from dcc_mcp_core.feedback import register_feedback_tool
    from dcc_mcp_core.schemas.finding import FindingRuntimeContext

    requests = []

    def _urlopen(request, *, timeout):
        requests.append((request, timeout))
        headers = {name.lower(): value for name, value in request.header_items()}
        submitted = json.loads(request.data.decode("utf-8"))
        return _GatewayResponse(
            {
                "ok": True,
                "success": True,
                "feedback_id": "11111111-1111-4111-8111-111111111111",
                "recorded_at": "2026-08-24T00:00:00.000Z",
                "event_resource_uri": "resources://gateway/events",
                "schema_version": 1,
                "fingerprint": submitted["fingerprint"],
            },
            request_id=headers["x-request-id"],
        )

    monkeypatch.setattr(feedback_module, "urlopen", _urlopen)
    store = FeedbackStore()
    server = MagicMock()
    server.registry = MagicMock()
    register_feedback_tool(
        server,
        dcc_name="photoshop",
        store=store,
        gateway_endpoint="http://127.0.0.1:19765/v1/feedback",
        instance_id_provider=lambda: "photoshop-instance-1",
        finding_context_provider=lambda: FindingRuntimeContext(
            dcc_type="photoshop",
            adapter="dcc-mcp-photoshop",
            adapter_version="0.9.7",
            core_version="0.20.11",
            host_version="26.4.1",
            os="windows",
            owning_repo="dcc-mcp/dcc-mcp-photoshop",
        ),
    )
    handler = server.register_handler.call_args.args[1]

    result = handler(
        {
            "phase": "skill",
            "severity": "degraded",
            "tool_slug": "photoshop_layers__merge",
            "intent": "Merge the selected layers",
            "observed": "The document remained locked",
            "expected": "The selected layers are merged",
            "repro": {"steps": ["Open a layered document", "Call merge"]},
            "evidence": {"error_kind": "document_locked", "request_id": "request-42"},
        }
    )

    assert result["success"] is True
    assert result["context"]["schema_version"] == 1
    assert result["context"]["fingerprint"].startswith("sha256:")
    body = json.loads(requests[0][0].data.decode("utf-8"))
    assert body["schema_version"] == 1
    assert body["dcc_type"] == "photoshop"
    assert body["adapter_version"] == "0.9.7"
    assert body["core_version"] == "0.20.11"
    assert body["host_version"] == "26.4.1"
    assert body["phase"] == "skill"
    assert body["severity"] == "degraded"
    assert body["tool_slug"] == "photoshop_layers__merge"
    assert body["evidence"]["instance_id"] == "photoshop-instance-1"
    assert store.recent()[0]["fingerprint"] == body["fingerprint"]


class _GatewayResponse:
    def __init__(self, payload, *, request_id):
        self.status = 201
        self.headers = {"X-Request-ID": request_id}
        self._payload = json.dumps(payload).encode("utf-8")

    def __enter__(self):
        return self

    def __exit__(self, *_args):
        return False

    def read(self):
        return self._payload


@pytest.mark.parametrize("dcc_name", ["maya", "3dsmax", "houdini"])
def test_registered_feedback_handler_forwards_through_one_gateway_implementation(monkeypatch, dcc_name):
    import dcc_mcp_core.feedback as feedback_module
    from dcc_mcp_core.feedback import FeedbackStore
    from dcc_mcp_core.feedback import get_feedback_entries
    from dcc_mcp_core.feedback import register_feedback_tool

    requests = []

    def _urlopen(request, *, timeout):
        requests.append((request, timeout))
        headers = {name.lower(): value for name, value in request.header_items()}
        submitted = json.loads(request.data.decode("utf-8"))
        return _GatewayResponse(
            {
                "ok": True,
                "success": True,
                "feedback_id": "11111111-1111-4111-8111-111111111111",
                "recorded_at": "2026-08-24T00:00:00.000Z",
                "event_resource_uri": "resources://gateway/events",
                "schema_version": submitted["schema_version"],
                "fingerprint": submitted["fingerprint"],
            },
            request_id=headers["x-request-id"],
        )

    monkeypatch.setattr(feedback_module, "urlopen", _urlopen)
    store = FeedbackStore()
    server = MagicMock()
    server.registry = MagicMock()
    register_feedback_tool(
        server,
        dcc_name=dcc_name,
        store=store,
        gateway_endpoint="http://127.0.0.1:19765/v1/feedback",
        instance_id_provider=lambda: f"{dcc_name}-instance-1",
    )
    handler = server.register_handler.call_args.args[1]

    result = handler(
        {
            "tool_name": f"{dcc_name}_scene__save",
            "intent": "Save the scene",
            "blocker": "The save action returned no artifact",
            "severity": "blocked",
            "request_id": "failed-request-42",
            "job_id": "failed-job-42",
        }
    )

    assert result["success"] is True
    assert result["context"]["feedback_id"] == "11111111-1111-4111-8111-111111111111"
    assert result["context"]["event_resource_uri"] == "resources://gateway/events"
    assert len(requests) == 1
    request, timeout = requests[0]
    assert request.full_url == "http://127.0.0.1:19765/v1/feedback"
    assert timeout == 5.0
    body = json.loads(request.data.decode("utf-8"))
    assert body["schema_version"] == 1
    assert body["dcc_type"] == dcc_name
    assert body["tool_slug"] == f"{dcc_name}_scene__save"
    assert body["phase"] == "dispatch"
    assert body["severity"] == "blocker"
    assert body["observed"] == "The save action returned no artifact"
    assert body["evidence"] == {
        "error_kind": "legacy_feedback",
        "request_id": "failed-request-42",
        "job_id": "failed-job-42",
        "instance_id": f"{dcc_name}-instance-1",
    }
    assert get_feedback_entries(store=store)[0]["id"] == "11111111-1111-4111-8111-111111111111"


def test_registered_feedback_handler_fails_closed_when_gateway_is_unavailable(monkeypatch):
    import dcc_mcp_core.feedback as feedback_module
    from dcc_mcp_core.feedback import FeedbackStore
    from dcc_mcp_core.feedback import get_feedback_entries
    from dcc_mcp_core.feedback import register_feedback_tool

    def _urlopen(*_args, **_kwargs):
        raise urllib.error.URLError("gateway offline")

    monkeypatch.setattr(feedback_module, "urlopen", _urlopen)
    store = FeedbackStore()
    server = MagicMock()
    server.registry = MagicMock()
    register_feedback_tool(
        server,
        dcc_name="photoshop",
        store=store,
        gateway_endpoint="http://127.0.0.1:19765/v1/feedback",
    )

    result = server.register_handler.call_args.args[1](
        {
            "tool_name": "photoshop_layers__merge",
            "intent": "Merge layers",
            "blocker": "Document is locked",
            "severity": "blocked",
        }
    )

    assert result["success"] is False
    assert result["error"] == "gateway_feedback_unavailable"
    assert get_feedback_entries(store=store) == []


def test_registered_feedback_handler_surfaces_local_persistence_failure(monkeypatch, tmp_path):
    import dcc_mcp_core.feedback as feedback_module
    from dcc_mcp_core.feedback import FeedbackStore
    from dcc_mcp_core.feedback import register_feedback_tool

    def _urlopen(request, *, timeout):
        headers = {name.lower(): value for name, value in request.header_items()}
        submitted = json.loads(request.data.decode("utf-8"))
        return _GatewayResponse(
            {
                "ok": True,
                "success": True,
                "feedback_id": "11111111-1111-4111-8111-111111111111",
                "recorded_at": "2026-08-24T00:00:00.000Z",
                "event_resource_uri": "resources://gateway/events",
                "schema_version": submitted["schema_version"],
                "fingerprint": submitted["fingerprint"],
            },
            request_id=headers["x-request-id"],
        )

    monkeypatch.setattr(feedback_module, "urlopen", _urlopen)
    store = FeedbackStore(
        path=tmp_path / "feedback" / "photoshop-7272.jsonl",
        max_bytes=64,
        backup_count=2,
    )
    server = MagicMock()
    server.registry = MagicMock()
    register_feedback_tool(
        server,
        dcc_name="photoshop",
        store=store,
        gateway_endpoint="http://127.0.0.1:19765/v1/feedback",
    )

    result = server.register_handler.call_args.args[1](
        {
            "tool_name": "photoshop_layers__merge",
            "intent": "Merge layers for a very long compositing operation",
            "blocker": "The document remained locked after the merge request",
            "severity": "blocked",
        }
    )

    assert result["success"] is False
    assert result["error"] == "feedback_persistence_failed"
    assert result["context"]["feedback_id"] == "11111111-1111-4111-8111-111111111111"
    assert store.recent() == []


def test_registered_feedback_handler_rejects_desynchronized_gateway_receipt(monkeypatch):
    import dcc_mcp_core.feedback as feedback_module
    from dcc_mcp_core.feedback import register_feedback_tool

    monkeypatch.setattr(
        feedback_module,
        "urlopen",
        lambda *_args, **_kwargs: _GatewayResponse(
            {
                "ok": True,
                "success": True,
                "feedback_id": "11111111-1111-4111-8111-111111111111",
                "recorded_at": "2026-08-24T00:00:00.000Z",
                "event_resource_uri": "resources://gateway/events",
            },
            request_id="stale-request-id",
        ),
    )
    server = MagicMock()
    server.registry = MagicMock()
    register_feedback_tool(
        server,
        dcc_name="zbrush",
        gateway_endpoint="http://127.0.0.1:19765/v1/feedback",
    )

    result = server.register_handler.call_args.args[1](
        {
            "tool_name": "zbrush_document__save",
            "intent": "Save the document",
            "blocker": "The response belonged to an earlier call",
            "severity": "blocked",
        }
    )

    assert result["success"] is False
    assert result["error"] == "transport_desync"


def test_registered_feedback_handler_rejects_invalid_gateway_receipt(monkeypatch):
    import dcc_mcp_core.feedback as feedback_module
    from dcc_mcp_core.feedback import register_feedback_tool

    def _urlopen(request, **_kwargs):
        headers = {name.lower(): value for name, value in request.header_items()}
        return _GatewayResponse(
            {
                "ok": False,
                "success": True,
                "feedback_id": "not-a-feedback-uuid",
                "recorded_at": "2026-08-24T00:00:00.000Z",
                "event_resource_uri": "resources://gateway/events",
            },
            request_id=headers["x-request-id"],
        )

    monkeypatch.setattr(feedback_module, "urlopen", _urlopen)
    server = MagicMock()
    server.registry = MagicMock()
    register_feedback_tool(
        server,
        dcc_name="maya",
        gateway_endpoint="http://127.0.0.1:19765/v1/feedback",
    )

    result = server.register_handler.call_args.args[1](
        {
            "tool_name": "maya_scene__save",
            "intent": "Save the scene",
            "blocker": "The gateway returned an invalid receipt",
            "severity": "blocked",
        }
    )

    assert result["success"] is False
    assert result["error"] == "gateway_feedback_invalid_receipt"


def test_registered_feedback_handler_rejects_mismatched_finding_fingerprint(monkeypatch):
    import dcc_mcp_core.feedback as feedback_module
    from dcc_mcp_core.feedback import register_feedback_tool

    def _urlopen(request, **_kwargs):
        headers = {name.lower(): value for name, value in request.header_items()}
        return _GatewayResponse(
            {
                "ok": True,
                "success": True,
                "feedback_id": "11111111-1111-4111-8111-111111111111",
                "recorded_at": "2026-08-24T00:00:00.000Z",
                "event_resource_uri": "resources://gateway/events",
                "schema_version": 1,
                "fingerprint": "sha256:" + "0" * 64,
            },
            request_id=headers["x-request-id"],
        )

    monkeypatch.setattr(feedback_module, "urlopen", _urlopen)
    server = MagicMock()
    server.registry = MagicMock()
    register_feedback_tool(
        server,
        dcc_name="maya",
        gateway_endpoint="http://127.0.0.1:19765/v1/feedback",
    )

    result = server.register_handler.call_args.args[1](
        {
            "tool_name": "maya_scene__save",
            "intent": "Save the scene",
            "blocker": "The receipt fingerprint was changed",
            "severity": "blocked",
        }
    )

    assert result["success"] is False
    assert result["error"] == "gateway_feedback_invalid_receipt"


def test_registered_feedback_handler_surfaces_correlated_gateway_rejection(monkeypatch):
    import io

    import dcc_mcp_core.feedback as feedback_module
    from dcc_mcp_core.feedback import register_feedback_tool

    def _urlopen(request, **_kwargs):
        headers = {name.lower(): value for name, value in request.header_items()}
        raise urllib.error.HTTPError(
            request.full_url,
            400,
            "Bad Request",
            {"X-Request-ID": headers["x-request-id"]},
            io.BytesIO(
                json.dumps(
                    {
                        "ok": False,
                        "success": False,
                        "error": {
                            "kind": "invalid-feedback",
                            "message": "gateway policy rejected this finding",
                        },
                    }
                ).encode("utf-8")
            ),
        )

    monkeypatch.setattr(feedback_module, "urlopen", _urlopen)
    server = MagicMock()
    server.registry = MagicMock()
    register_feedback_tool(
        server,
        dcc_name="maya",
        gateway_endpoint="http://127.0.0.1:19765/v1/feedback",
    )

    result = server.register_handler.call_args.args[1](
        {
            "tool_name": "maya_scene__save",
            "intent": "Save the scene",
            "blocker": "The report is invalid",
            "severity": "blocked",
        }
    )

    assert result["success"] is False
    assert result["error"] == "gateway_feedback_rejected"
    assert result["message"] == "gateway policy rejected this finding"
    assert result["context"]["status_code"] == 400


def test_registered_feedback_handler_fails_closed_for_malformed_gateway_endpoint():
    from dcc_mcp_core.feedback import register_feedback_tool

    server = MagicMock()
    server.registry = MagicMock()
    register_feedback_tool(
        server,
        dcc_name="custom-host",
        gateway_endpoint="not a valid gateway URL",
    )

    result = server.register_handler.call_args.args[1](
        {
            "tool_name": "custom_scene__save",
            "intent": "Save the scene",
            "blocker": "No receipt was returned",
            "severity": "blocked",
        }
    )

    assert result["success"] is False
    assert result["error"] == "gateway_feedback_unavailable"


def test_registered_feedback_handler_requires_bound_instance_when_provider_is_configured(monkeypatch):
    import dcc_mcp_core.feedback as feedback_module
    from dcc_mcp_core.feedback import register_feedback_tool

    monkeypatch.setattr(
        feedback_module,
        "urlopen",
        lambda *_args, **_kwargs: pytest.fail("unbound feedback must not reach the gateway"),
    )
    server = MagicMock()
    server.registry = MagicMock()
    register_feedback_tool(
        server,
        dcc_name="maya",
        gateway_endpoint="http://127.0.0.1:19765/v1/feedback",
        instance_id_provider=lambda: None,
    )

    result = server.register_handler.call_args.args[1](
        {
            "tool_name": "maya_scene__save",
            "intent": "Save the scene",
            "blocker": "The adapter instance is not bound",
            "severity": "blocked",
        }
    )

    assert result["success"] is False
    assert result["error"] == "feedback_instance_unavailable"


def test_register_feedback_tool_no_registry():
    from dcc_mcp_core.feedback import register_feedback_tool

    class _NoRegistry:
        @property
        def registry(self):
            raise AttributeError("no registry")

        def register_handler(self, *args, **kwargs):
            pass

    register_feedback_tool(_NoRegistry())


# ── public API ────────────────────────────────────────────────────────────


def test_importable_from_top_level():
    import dcc_mcp_core

    assert hasattr(dcc_mcp_core, "extract_rationale")
    assert hasattr(dcc_mcp_core, "make_rationale_meta")
    assert hasattr(dcc_mcp_core, "register_feedback_tool")
    assert hasattr(dcc_mcp_core, "get_feedback_entries")
    assert hasattr(dcc_mcp_core, "clear_feedback")
    assert hasattr(dcc_mcp_core, "FeedbackStore")
    assert hasattr(dcc_mcp_core, "get_default_feedback_store")
    assert hasattr(dcc_mcp_core, "reset_default_feedback_store_for_tests")
    assert dcc_mcp_core.FINDING_V1_SCHEMA_VERSION == 1
    assert hasattr(dcc_mcp_core, "FindingRuntimeContext")
    assert hasattr(dcc_mcp_core, "FindingValidationError")
    assert hasattr(dcc_mcp_core, "build_finding_v1")
    assert hasattr(dcc_mcp_core, "finding_fingerprint")
    assert hasattr(dcc_mcp_core, "finding_v1_json_schema")
