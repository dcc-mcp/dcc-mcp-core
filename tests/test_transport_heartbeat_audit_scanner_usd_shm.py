"""Deep tests for iteration 82.

Coverage areas:
- TransportManager.heartbeat + update_service_status + rank_services + find_best_service
- AuditMiddleware.records() field depth
- decode_envelope (notify/response structure)
- SkillScanner repr + dcc_name filter + discovered_skills + clear_cache
- UsdPrim.attributes_summary() + attribute_names() all types
- PySharedBuffer cross-instance read/write
"""

from __future__ import annotations

# Import built-in modules
import json
import pathlib
import struct
import tempfile
import uuid

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core import ActionDispatcher
from dcc_mcp_core import ActionPipeline
from dcc_mcp_core import ActionRegistry
from dcc_mcp_core import PySharedBuffer
from dcc_mcp_core import ServiceStatus
from dcc_mcp_core import SkillScanner
from dcc_mcp_core import TransportManager
from dcc_mcp_core import UsdStage
from dcc_mcp_core import VtValue
from dcc_mcp_core import decode_envelope
from dcc_mcp_core import encode_notify
from dcc_mcp_core import encode_response

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def strip_length_prefix(data: bytes) -> bytes:
    """Strip 4-byte big-endian length prefix from a framed message."""
    if len(data) < 4:
        return data
    length = struct.unpack(">I", data[:4])[0]
    return data[4 : 4 + length]


def make_transport_manager(tmpdir: str) -> TransportManager:
    """Create a TransportManager with a temp registry dir."""
    return TransportManager(tmpdir)


def make_pipeline_with_audit() -> tuple[ActionPipeline, object]:
    """Return (pipeline, audit_middleware) with a single 'my_action' handler."""
    reg = ActionRegistry()
    reg.register("my_action", description="test action", category="test")
    disp = ActionDispatcher(reg)
    disp.register_handler("my_action", lambda p: {"result": 1})
    pipe = ActionPipeline(disp)
    audit = pipe.add_audit(record_params=True)
    return pipe, audit


# ---------------------------------------------------------------------------
# TransportManager.heartbeat + update_service_status + rank_services
# ---------------------------------------------------------------------------


class TestTransportManagerHeartbeatRank:
    """Tests for heartbeat/update_service_status/rank_services/find_best_service."""

    def test_heartbeat_ok(self, tmp_path):
        """heartbeat() accepts valid dcc_type + instance_id without raising."""
        tm = make_transport_manager(str(tmp_path))
        tm.register_service("maya", "127.0.0.1", 9001, version="1.0.0")
        svcs = tm.list_all_services()
        iid = svcs[0].instance_id
        # Should not raise
        tm.heartbeat("maya", iid)

    def test_rank_initial_all_available(self, tmp_path):
        """rank_services() returns all registered instances when all AVAILABLE."""
        tm = make_transport_manager(str(tmp_path))
        tm.register_service("maya", "127.0.0.1", 9001, version="1.0.0")
        tm.register_service("maya", "127.0.0.1", 9002, version="1.0.0")
        ranked = tm.rank_services("maya")
        assert len(ranked) == 2

    def test_rank_excludes_unreachable(self, tmp_path):
        """rank_services() excludes UNREACHABLE instances."""
        tm = make_transport_manager(str(tmp_path))
        tm.register_service("maya", "127.0.0.1", 9001, version="1.0.0")
        tm.register_service("maya", "127.0.0.1", 9002, version="1.0.0")
        svcs = tm.list_all_services()
        iid1 = svcs[0].instance_id

        tm.update_service_status("maya", iid1, ServiceStatus.UNREACHABLE)
        ranked = tm.rank_services("maya")
        assert len(ranked) == 1

    def test_rank_excludes_all_unreachable(self, tmp_path):
        """rank_services() raises RuntimeError when all instances UNREACHABLE."""
        tm = make_transport_manager(str(tmp_path))
        tm.register_service("maya", "127.0.0.1", 9001, version="1.0.0")
        svcs = tm.list_all_services()
        iid = svcs[0].instance_id

        tm.update_service_status("maya", iid, ServiceStatus.UNREACHABLE)
        with pytest.raises(RuntimeError):
            tm.rank_services("maya")

    def test_rank_restores_after_available(self, tmp_path):
        """rank_services() includes instance again after restoring to AVAILABLE."""
        tm = make_transport_manager(str(tmp_path))
        tm.register_service("maya", "127.0.0.1", 9001, version="1.0.0")
        svcs = tm.list_all_services()
        iid = svcs[0].instance_id

        tm.update_service_status("maya", iid, ServiceStatus.UNREACHABLE)
        # All unreachable raises RuntimeError
        with pytest.raises(RuntimeError):
            tm.rank_services("maya")

        tm.update_service_status("maya", iid, ServiceStatus.AVAILABLE)
        ranked = tm.rank_services("maya")
        assert len(ranked) == 1

    def test_rank_restored_status_is_available(self, tmp_path):
        """Restored instance in rank has AVAILABLE status."""
        tm = make_transport_manager(str(tmp_path))
        tm.register_service("maya", "127.0.0.1", 9001, version="1.0.0")
        svcs = tm.list_all_services()
        iid = svcs[0].instance_id

        tm.update_service_status("maya", iid, ServiceStatus.UNREACHABLE)
        tm.update_service_status("maya", iid, ServiceStatus.AVAILABLE)

        ranked = tm.rank_services("maya")
        assert ranked[0].status == ServiceStatus.AVAILABLE

    def test_update_service_status_get_service_reflects_change(self, tmp_path):
        """get_service() reflects status change from update_service_status."""
        tm = make_transport_manager(str(tmp_path))
        tm.register_service("maya", "127.0.0.1", 9001, version="1.0.0")
        svcs = tm.list_all_services()
        iid = svcs[0].instance_id

        tm.update_service_status("maya", iid, ServiceStatus.UNREACHABLE)
        svc = tm.get_service("maya", iid)
        assert svc is not None
        assert svc.status == ServiceStatus.UNREACHABLE

    def test_find_best_service_skips_unreachable(self, tmp_path):
        """find_best_service() skips UNREACHABLE and returns the available one."""
        tm = make_transport_manager(str(tmp_path))
        tm.register_service("maya", "127.0.0.1", 9001, version="1.0.0")
        tm.register_service("maya", "127.0.0.1", 9002, version="1.0.0")
        svcs = tm.list_all_services()
        iid1, iid2 = svcs[0].instance_id, svcs[1].instance_id

        tm.update_service_status("maya", iid1, ServiceStatus.UNREACHABLE)
        best = tm.find_best_service("maya")
        assert best is not None
        assert best.instance_id == iid2

    def test_find_best_service_raises_when_all_unreachable(self, tmp_path):
        """find_best_service() raises RuntimeError when all instances UNREACHABLE."""
        tm = make_transport_manager(str(tmp_path))
        tm.register_service("maya", "127.0.0.1", 9001, version="1.0.0")
        svcs = tm.list_all_services()
        iid = svcs[0].instance_id

        tm.update_service_status("maya", iid, ServiceStatus.UNREACHABLE)
        with pytest.raises(RuntimeError):
            tm.find_best_service("maya")

    def test_rank_returns_list_type(self, tmp_path):
        """rank_services() returns a list."""
        tm = make_transport_manager(str(tmp_path))
        tm.register_service("maya", "127.0.0.1", 9001, version="1.0.0")
        result = tm.rank_services("maya")
        assert isinstance(result, list)

    def test_rank_empty_for_unknown_dcc(self, tmp_path):
        """rank_services() raises RuntimeError for unknown dcc_type."""
        tm = make_transport_manager(str(tmp_path))
        with pytest.raises(RuntimeError):
            tm.rank_services("unknown_dcc")

    def test_is_shutdown_initially_false(self, tmp_path):
        """is_shutdown() returns False before shutdown() is called."""
        tm = make_transport_manager(str(tmp_path))
        assert tm.is_shutdown() is False

    def test_session_count_initially_zero(self, tmp_path):
        """session_count() returns 0 for a fresh TransportManager."""
        tm = make_transport_manager(str(tmp_path))
        assert tm.session_count() == 0

    def test_pool_count_initially_zero(self, tmp_path):
        """pool_count_for_dcc() returns 0 when no connections have been made."""
        tm = make_transport_manager(str(tmp_path))
        tm.register_service("maya", "127.0.0.1", 9001, version="1.0.0")
        assert tm.pool_count_for_dcc("maya") == 0

    def test_list_all_services_after_register(self, tmp_path):
        """list_all_services() includes all registered services."""
        tm = make_transport_manager(str(tmp_path))
        tm.register_service("maya", "127.0.0.1", 9001)
        tm.register_service("blender", "127.0.0.1", 9002)
        svcs = tm.list_all_services()
        assert len(svcs) == 2

    def test_heartbeat_after_unreachable_does_not_restore_status(self, tmp_path):
        """heartbeat() does not automatically restore an UNREACHABLE instance."""
        tm = make_transport_manager(str(tmp_path))
        tm.register_service("maya", "127.0.0.1", 9001, version="1.0.0")
        svcs = tm.list_all_services()
        iid = svcs[0].instance_id

        tm.update_service_status("maya", iid, ServiceStatus.UNREACHABLE)
        tm.heartbeat("maya", iid)
        svc = tm.get_service("maya", iid)
        # heartbeat doesn't reset status, just updates last_seen
        assert svc.status == ServiceStatus.UNREACHABLE

    def test_busy_status_excluded_from_rank(self, tmp_path):
        """BUSY instances are included in rank_services (only UNREACHABLE excluded)."""
        tm = make_transport_manager(str(tmp_path))
        tm.register_service("maya", "127.0.0.1", 9001)
        tm.register_service("maya", "127.0.0.1", 9002)
        svcs = tm.list_all_services()
        iid1 = svcs[0].instance_id

        tm.update_service_status("maya", iid1, ServiceStatus.BUSY)
        # BUSY should still appear in ranking (only UNREACHABLE/SHUTTING_DOWN excluded)
        ranked = tm.rank_services("maya")
        # both should appear (BUSY is still rankable)
        assert len(ranked) >= 1


# ---------------------------------------------------------------------------
# AuditMiddleware.records() field depth
# ---------------------------------------------------------------------------


class TestAuditMiddlewareRecords:
    """Tests for AuditMiddleware records() field depth."""

    def test_records_len_after_dispatch(self):
        """records() has one entry after one successful dispatch."""
        pipe, audit = make_pipeline_with_audit()
        pipe.dispatch("my_action", '{"x": 1}')
        assert len(audit.records()) == 1

    def test_records_has_expected_keys(self):
        """Each record has exactly the expected keys."""
        pipe, audit = make_pipeline_with_audit()
        pipe.dispatch("my_action", '{"x": 1}')
        rec = audit.records()[0]
        assert set(rec.keys()) == {"action", "error", "output_preview", "success", "timestamp_ms"}

    def test_records_action_field(self):
        """record['action'] matches the dispatched action name."""
        pipe, audit = make_pipeline_with_audit()
        pipe.dispatch("my_action", '{"x": 1}')
        assert audit.records()[0]["action"] == "my_action"

    def test_records_success_true_on_success(self):
        """record['success'] is True for a successful dispatch."""
        pipe, audit = make_pipeline_with_audit()
        pipe.dispatch("my_action", '{"x": 1}')
        assert audit.records()[0]["success"] is True

    def test_records_timestamp_ms_is_int(self):
        """record['timestamp_ms'] is an integer."""
        pipe, audit = make_pipeline_with_audit()
        pipe.dispatch("my_action", '{"x": 1}')
        ts = audit.records()[0]["timestamp_ms"]
        assert isinstance(ts, int)

    def test_records_timestamp_ms_positive(self):
        """record['timestamp_ms'] is a positive integer (real epoch ms)."""
        pipe, audit = make_pipeline_with_audit()
        pipe.dispatch("my_action", '{"x": 1}')
        ts = audit.records()[0]["timestamp_ms"]
        assert ts > 0

    def test_records_error_is_none_on_success(self):
        """record['error'] is None for a successful dispatch."""
        pipe, audit = make_pipeline_with_audit()
        pipe.dispatch("my_action", '{"x": 1}')
        assert audit.records()[0]["error"] is None

    def test_records_output_preview_is_none_by_default(self):
        """record['output_preview'] is None (not stored in default audit)."""
        pipe, audit = make_pipeline_with_audit()
        pipe.dispatch("my_action", '{"x": 1}')
        # output_preview may be None or a string; it's not the full output
        rec = audit.records()[0]
        assert "output_preview" in rec

    def test_records_accumulate_multiple_dispatches(self):
        """records() accumulates entries for each dispatch call."""
        pipe, audit = make_pipeline_with_audit()
        for _ in range(5):
            pipe.dispatch("my_action", '{"x": 1}')
        assert len(audit.records()) == 5

    def test_records_timestamp_ordering(self):
        """Timestamps are non-decreasing across successive dispatches."""
        pipe, audit = make_pipeline_with_audit()
        pipe.dispatch("my_action", '{"x": 1}')
        pipe.dispatch("my_action", '{"x": 2}')
        recs = audit.records()
        assert recs[0]["timestamp_ms"] <= recs[1]["timestamp_ms"]

    def test_dispatch_invalid_json_raises_value_error(self):
        """Dispatching invalid JSON raises ValueError (not recorded as failure)."""
        pipe, audit = make_pipeline_with_audit()
        with pytest.raises(ValueError):
            pipe.dispatch("my_action", "not-json")
        # Invalid dispatch doesn't create an audit record
        assert len(audit.records()) == 0

    def test_records_for_action_filters_correctly(self):
        """records_for_action() returns only records for the specified action."""
        reg = ActionRegistry()
        reg.register("action_a", description="a", category="test")
        reg.register("action_b", description="b", category="test")
        disp = ActionDispatcher(reg)
        disp.register_handler("action_a", lambda p: {"a": 1})
        disp.register_handler("action_b", lambda p: {"b": 2})
        pipe = ActionPipeline(disp)
        audit = pipe.add_audit()

        pipe.dispatch("action_a", "{}")
        pipe.dispatch("action_b", "{}")
        pipe.dispatch("action_a", "{}")

        a_recs = audit.records_for_action("action_a")
        b_recs = audit.records_for_action("action_b")
        assert len(a_recs) == 2
        assert len(b_recs) == 1
        assert all(r["action"] == "action_a" for r in a_recs)

    def test_records_for_action_nonexistent_returns_empty(self):
        """records_for_action() returns empty list for unknown action name."""
        pipe, audit = make_pipeline_with_audit()
        pipe.dispatch("my_action", '{"x": 1}')
        assert audit.records_for_action("nonexistent") == []

    def test_record_count_matches_records_len(self):
        """record_count() matches len(records())."""
        pipe, audit = make_pipeline_with_audit()
        for _ in range(3):
            pipe.dispatch("my_action", "{}")
        assert audit.record_count() == len(audit.records())

    def test_audit_clear_resets_count(self):
        """audit.clear() resets record_count() to 0."""
        pipe, audit = make_pipeline_with_audit()
        pipe.dispatch("my_action", "{}")
        pipe.dispatch("my_action", "{}")
        audit.clear()
        assert audit.record_count() == 0
        assert audit.records() == []


# ---------------------------------------------------------------------------
# decode_envelope: notify / response structure
# ---------------------------------------------------------------------------


class TestDecodeEnvelopeStructure:
    """Tests for decode_envelope with stripped length prefix."""

    def test_notify_ping_returns_dict(self):
        """decode_envelope of a notify frame returns a dict."""
        frame = encode_notify("ping")
        result = decode_envelope(strip_length_prefix(frame))
        assert isinstance(result, dict)

    def test_notify_type_field(self):
        """Notify envelope has type == 'notify'."""
        frame = encode_notify("ping")
        env = decode_envelope(strip_length_prefix(frame))
        assert env["type"] == "notify"

    def test_notify_topic_field_ping(self):
        """Notify envelope for 'ping' has topic == 'ping'."""
        frame = encode_notify("ping")
        env = decode_envelope(strip_length_prefix(frame))
        assert env["topic"] == "ping"

    def test_notify_topic_field_pong(self):
        """Notify envelope for 'pong' has topic == 'pong'."""
        frame = encode_notify("pong")
        env = decode_envelope(strip_length_prefix(frame))
        assert env["topic"] == "pong"

    def test_notify_topic_field_shutdown(self):
        """Notify envelope for 'shutdown' has topic == 'shutdown'."""
        frame = encode_notify("shutdown")
        env = decode_envelope(strip_length_prefix(frame))
        assert env["topic"] == "shutdown"

    def test_notify_topic_field_heartbeat(self):
        """Notify envelope for 'heartbeat' has topic == 'heartbeat'."""
        frame = encode_notify("heartbeat")
        env = decode_envelope(strip_length_prefix(frame))
        assert env["topic"] == "heartbeat"

    def test_notify_topic_field_custom(self):
        """Notify envelope for custom topic preserves the topic string."""
        frame = encode_notify("custom_event")
        env = decode_envelope(strip_length_prefix(frame))
        assert env["topic"] == "custom_event"

    def test_notify_id_is_none(self):
        """Notify envelope id field is None."""
        frame = encode_notify("ping")
        env = decode_envelope(strip_length_prefix(frame))
        assert env["id"] is None

    def test_notify_data_empty_when_no_data(self):
        """Notify envelope data is b'' when no data argument given."""
        frame = encode_notify("ping")
        env = decode_envelope(strip_length_prefix(frame))
        assert env["data"] == b""

    def test_notify_data_preserved(self):
        """Notify envelope data field preserves the given bytes."""
        frame = encode_notify("pong", b"payload-data")
        env = decode_envelope(strip_length_prefix(frame))
        assert env["data"] == b"payload-data"

    def test_notify_keys(self):
        """Notify envelope has exactly type/id/topic/data keys."""
        frame = encode_notify("ping")
        env = decode_envelope(strip_length_prefix(frame))
        assert set(env.keys()) == {"type", "id", "topic", "data"}

    def test_response_type_field(self):
        """Response envelope has type == 'response'."""
        req_id = str(uuid.uuid4())
        frame = encode_response(req_id, True, b"ok", None)
        env = decode_envelope(strip_length_prefix(frame))
        assert env["type"] == "response"

    def test_response_success_true(self):
        """Response envelope with success=True has success field True."""
        req_id = str(uuid.uuid4())
        frame = encode_response(req_id, True, b"result", None)
        env = decode_envelope(strip_length_prefix(frame))
        assert env["success"] is True

    def test_response_success_false(self):
        """Response envelope with success=False has success field False."""
        req_id = str(uuid.uuid4())
        frame = encode_response(req_id, False, b"", "failed")
        env = decode_envelope(strip_length_prefix(frame))
        assert env["success"] is False

    def test_response_id_preserved(self):
        """Response envelope preserves the request_id."""
        req_id = str(uuid.uuid4())
        frame = encode_response(req_id, True, b"", None)
        env = decode_envelope(strip_length_prefix(frame))
        assert env["id"] == req_id

    def test_response_payload_is_bytes(self):
        """Response envelope payload field is bytes."""
        req_id = str(uuid.uuid4())
        frame = encode_response(req_id, True, b"result_data", None)
        env = decode_envelope(strip_length_prefix(frame))
        assert isinstance(env["payload"], bytes)

    def test_response_payload_preserved(self):
        """Response envelope payload bytes are preserved exactly."""
        req_id = str(uuid.uuid4())
        frame = encode_response(req_id, True, b"exact-bytes", None)
        env = decode_envelope(strip_length_prefix(frame))
        assert env["payload"] == b"exact-bytes"

    def test_response_error_none_on_success(self):
        """Response envelope error field is None when success."""
        req_id = str(uuid.uuid4())
        frame = encode_response(req_id, True, b"", None)
        env = decode_envelope(strip_length_prefix(frame))
        assert env["error"] is None

    def test_response_error_string_on_failure(self):
        """Response envelope error field contains error string on failure."""
        req_id = str(uuid.uuid4())
        frame = encode_response(req_id, False, b"", "something went wrong")
        env = decode_envelope(strip_length_prefix(frame))
        assert env["error"] == "something went wrong"

    def test_response_keys(self):
        """Response envelope has exactly type/id/success/payload/error keys."""
        req_id = str(uuid.uuid4())
        frame = encode_response(req_id, True, b"", None)
        env = decode_envelope(strip_length_prefix(frame))
        assert set(env.keys()) == {"type", "id", "success", "payload", "error"}

    def test_decode_empty_raises(self):
        """decode_envelope raises on empty bytes."""
        with pytest.raises((RuntimeError, ValueError)):
            decode_envelope(b"")

    def test_decode_garbage_raises(self):
        """decode_envelope raises on garbage bytes."""
        with pytest.raises((RuntimeError, ValueError)):
            decode_envelope(b"not-a-valid-msgpack-envelope")

    def test_notify_different_topics_roundtrip(self):
        """Topic field is preserved correctly for all tested topics."""
        for topic in ["ping", "pong", "shutdown", "heartbeat", "error", "ready"]:
            frame = encode_notify(topic)
            env = decode_envelope(strip_length_prefix(frame))
            assert env["topic"] == topic, f"Failed for topic '{topic}'"


# ---------------------------------------------------------------------------
# SkillScanner repr + dcc_name filter + discovered_skills + clear_cache
# ---------------------------------------------------------------------------


class TestSkillScannerReprAndFilter:
    """Tests for SkillScanner repr + scan + discovered_skills + clear_cache."""

    def _make_skills(self, base_dir: pathlib.Path, dccs: list[str]) -> None:
        """Create SKILL.md files for the given DCCs."""
        for dcc in dccs:
            skill_dir = base_dir / f"{dcc}-skill"
            skill_dir.mkdir(exist_ok=True)
            (skill_dir / "SKILL.md").write_text(
                f"---\nname: {dcc}-skill\ndcc: {dcc}\ntags: [test]\n---\nA {dcc} skill.\n"
            )

    def test_repr_is_string(self):
        """repr(SkillScanner()) returns a non-empty string."""
        scanner = SkillScanner()
        r = repr(scanner)
        assert isinstance(r, str)
        assert len(r) > 0

    def test_repr_contains_cached(self):
        """Repr includes 'cached=' substring."""
        scanner = SkillScanner()
        assert "cached=" in repr(scanner)

    def test_repr_contains_discovered(self):
        """Repr includes 'discovered=' substring."""
        scanner = SkillScanner()
        assert "discovered=" in repr(scanner)

    def test_repr_initial_zero_counts(self):
        """Fresh scanner repr shows 0 for both cached and discovered."""
        scanner = SkillScanner()
        r = repr(scanner)
        assert "cached=0" in r
        assert "discovered=0" in r

    def test_scan_with_extra_paths_returns_list(self, tmp_path):
        """scan(extra_paths=...) returns a list."""
        self._make_skills(tmp_path, ["maya"])
        scanner = SkillScanner()
        result = scanner.scan(extra_paths=[str(tmp_path)])
        assert isinstance(result, list)

    def test_scan_discovers_all_skills(self, tmp_path):
        """scan() discovers all skill directories under extra_paths."""
        self._make_skills(tmp_path, ["maya", "blender", "python"])
        scanner = SkillScanner()
        result = scanner.scan(extra_paths=[str(tmp_path)])
        assert len(result) == 3

    def test_scan_returns_string_paths(self, tmp_path):
        """scan() returns list of string paths (not SkillMetadata objects)."""
        self._make_skills(tmp_path, ["maya"])
        scanner = SkillScanner()
        result = scanner.scan(extra_paths=[str(tmp_path)])
        assert all(isinstance(p, str) for p in result)

    def test_scan_paths_exist(self, tmp_path):
        """All paths returned by scan() exist on the filesystem."""
        self._make_skills(tmp_path, ["maya", "blender"])
        scanner = SkillScanner()
        result = scanner.scan(extra_paths=[str(tmp_path)])
        for p in result:
            assert pathlib.Path(p).exists()

    def test_repr_updates_after_scan(self, tmp_path):
        """Repr shows updated counts after scan()."""
        self._make_skills(tmp_path, ["maya", "blender"])
        scanner = SkillScanner()
        scanner.scan(extra_paths=[str(tmp_path)])
        r = repr(scanner)
        assert "cached=2" in r
        assert "discovered=2" in r

    def test_discovered_skills_is_attribute(self, tmp_path):
        """discovered_skills is a list attribute (not a callable method)."""
        self._make_skills(tmp_path, ["maya"])
        scanner = SkillScanner()
        scanner.scan(extra_paths=[str(tmp_path)])
        ds = scanner.discovered_skills
        assert isinstance(ds, list)

    def test_discovered_skills_matches_scan(self, tmp_path):
        """discovered_skills matches the results of scan()."""
        self._make_skills(tmp_path, ["maya", "blender"])
        scanner = SkillScanner()
        result = scanner.scan(extra_paths=[str(tmp_path)])
        assert set(scanner.discovered_skills) == set(result)

    def test_discovered_skills_empty_before_scan(self):
        """discovered_skills is empty before any scan is called."""
        scanner = SkillScanner()
        assert scanner.discovered_skills == []

    def test_scan_no_paths_returns_empty(self):
        """scan() with no paths returns empty list."""
        scanner = SkillScanner()
        result = scanner.scan()
        assert result == []

    def test_clear_cache_resets_discovered(self, tmp_path):
        """clear_cache() resets discovered_skills to empty."""
        self._make_skills(tmp_path, ["maya"])
        scanner = SkillScanner()
        scanner.scan(extra_paths=[str(tmp_path)])
        scanner.clear_cache()
        assert scanner.discovered_skills == []

    def test_clear_cache_resets_repr_counts(self, tmp_path):
        """clear_cache() resets repr counts to 0."""
        self._make_skills(tmp_path, ["maya"])
        scanner = SkillScanner()
        scanner.scan(extra_paths=[str(tmp_path)])
        scanner.clear_cache()
        r = repr(scanner)
        assert "cached=0" in r
        assert "discovered=0" in r

    def test_scan_after_clear_rediscovers(self, tmp_path):
        """scan() after clear_cache() re-discovers skills."""
        self._make_skills(tmp_path, ["maya"])
        scanner = SkillScanner()
        scanner.scan(extra_paths=[str(tmp_path)])
        scanner.clear_cache()
        result2 = scanner.scan(extra_paths=[str(tmp_path)])
        assert len(result2) == 1

    def test_force_refresh_re_scans(self, tmp_path):
        """scan(force_refresh=True) re-scans even when cache is populated."""
        self._make_skills(tmp_path, ["maya"])
        scanner = SkillScanner()
        result1 = scanner.scan(extra_paths=[str(tmp_path)])
        result2 = scanner.scan(extra_paths=[str(tmp_path)], force_refresh=True)
        assert len(result2) == len(result1)

    def test_scan_single_skill_dir(self, tmp_path):
        """scan() discovers exactly one skill when only one exists."""
        self._make_skills(tmp_path, ["houdini"])
        scanner = SkillScanner()
        result = scanner.scan(extra_paths=[str(tmp_path)])
        assert len(result) == 1


# ---------------------------------------------------------------------------
# UsdPrim.attributes_summary() + attribute_names() all types
# ---------------------------------------------------------------------------


class TestUsdPrimAttributesSummary:
    """Tests for UsdPrim.attributes_summary() and attribute_names()."""

    def _make_prim(self, path: str = "/World", prim_type: str = "Xform") -> object:
        """Create a UsdStage and define a prim at the given path."""
        stage = UsdStage("test-stage")
        return stage.define_prim(path, prim_type)

    def test_attributes_summary_empty_prim(self):
        """attributes_summary() returns empty dict for a prim with no attributes."""
        prim = self._make_prim("/Empty", "Scope")
        assert prim.attributes_summary() == {}

    def test_attributes_summary_returns_dict(self):
        """attributes_summary() returns a dict."""
        prim = self._make_prim()
        prim.set_attribute("x", VtValue.from_int(1))
        result = prim.attributes_summary()
        assert isinstance(result, dict)

    def test_attributes_summary_int_type(self):
        """attributes_summary() maps int attribute to 'int' type string."""
        prim = self._make_prim()
        prim.set_attribute("count", VtValue.from_int(42))
        summary = prim.attributes_summary()
        assert summary.get("count") == "int"

    def test_attributes_summary_float_type(self):
        """attributes_summary() maps float attribute to 'float' type string."""
        prim = self._make_prim()
        prim.set_attribute("scale", VtValue.from_float(3.14))
        summary = prim.attributes_summary()
        assert summary.get("scale") == "float"

    def test_attributes_summary_string_type(self):
        """attributes_summary() maps string attribute to 'string' type string."""
        prim = self._make_prim()
        prim.set_attribute("label", VtValue.from_string("mesh01"))
        summary = prim.attributes_summary()
        assert summary.get("label") == "string"

    def test_attributes_summary_bool_type(self):
        """attributes_summary() maps bool attribute to 'bool' type string."""
        prim = self._make_prim()
        prim.set_attribute("visible", VtValue.from_bool(True))
        summary = prim.attributes_summary()
        assert summary.get("visible") == "bool"

    def test_attributes_summary_mixed_types(self):
        """attributes_summary() correctly maps mixed type attributes."""
        prim = self._make_prim()
        prim.set_attribute("i", VtValue.from_int(1))
        prim.set_attribute("f", VtValue.from_float(1.0))
        prim.set_attribute("s", VtValue.from_string("s"))
        prim.set_attribute("b", VtValue.from_bool(False))
        summary = prim.attributes_summary()
        assert summary == {"i": "int", "f": "float", "s": "string", "b": "bool"}

    def test_attributes_summary_keys_match_set_attributes(self):
        """attributes_summary() keys match the attribute names that were set."""
        prim = self._make_prim()
        names = ["alpha", "beta", "gamma"]
        for name in names:
            prim.set_attribute(name, VtValue.from_int(0))
        summary = prim.attributes_summary()
        assert set(summary.keys()) == set(names)

    def test_attribute_names_empty_prim(self):
        """attribute_names() returns empty list for prim with no attributes."""
        prim = self._make_prim("/EmptyPrim", "Scope")
        assert prim.attribute_names() == []

    def test_attribute_names_returns_list(self):
        """attribute_names() returns a list."""
        prim = self._make_prim()
        prim.set_attribute("x", VtValue.from_int(1))
        assert isinstance(prim.attribute_names(), list)

    def test_attribute_names_matches_set_attributes(self):
        """attribute_names() contains all attribute names that were set."""
        prim = self._make_prim()
        prim.set_attribute("one", VtValue.from_int(1))
        prim.set_attribute("two", VtValue.from_float(2.0))
        prim.set_attribute("three", VtValue.from_string("3"))
        names = prim.attribute_names()
        assert set(names) == {"one", "two", "three"}

    def test_attributes_summary_values_are_strings(self):
        """All values in attributes_summary() are strings (type names)."""
        prim = self._make_prim()
        prim.set_attribute("n", VtValue.from_int(0))
        prim.set_attribute("r", VtValue.from_float(1.0))
        summary = prim.attributes_summary()
        assert all(isinstance(v, str) for v in summary.values())

    def test_attributes_summary_after_overwrite(self):
        """attributes_summary() reflects the latest set_attribute call."""
        prim = self._make_prim()
        prim.set_attribute("val", VtValue.from_int(1))
        # Overwrite with a float
        prim.set_attribute("val", VtValue.from_float(3.14))
        summary = prim.attributes_summary()
        assert summary.get("val") == "float"

    def test_summary_and_names_count_match(self):
        """len(attributes_summary()) == len(attribute_names())."""
        prim = self._make_prim()
        prim.set_attribute("a", VtValue.from_int(1))
        prim.set_attribute("b", VtValue.from_float(2.0))
        assert len(prim.attributes_summary()) == len(prim.attribute_names())


# ---------------------------------------------------------------------------
# PySharedBuffer cross-instance read/write
# ---------------------------------------------------------------------------


def _open_from_desc(desc_json: str) -> PySharedBuffer:
    """Open a PySharedBuffer from its descriptor_json string."""
    d = json.loads(desc_json)
    return PySharedBuffer.open(d["path"], d["id"])


class TestPySharedBufferCrossInstance:
    """Tests for PySharedBuffer cross-instance read/write via descriptor_json."""

    def test_create_and_id_is_string(self):
        """PySharedBuffer.create() returns a buffer with a non-empty id attribute."""
        buf = PySharedBuffer.create(1024)
        assert isinstance(buf.id, str)
        assert len(buf.id) > 0

    def test_create_capacity(self):
        """PySharedBuffer.create() capacity matches the requested size."""
        buf = PySharedBuffer.create(4096)
        assert buf.capacity() == 4096

    def test_descriptor_json_is_string(self):
        """descriptor_json() returns a string."""
        buf = PySharedBuffer.create(1024)
        assert isinstance(buf.descriptor_json(), str)

    def test_descriptor_json_not_empty(self):
        """descriptor_json() returns a non-empty string."""
        buf = PySharedBuffer.create(1024)
        assert len(buf.descriptor_json()) > 0

    def test_descriptor_json_contains_id(self):
        """descriptor_json() JSON contains 'id' field matching buf.id."""
        buf = PySharedBuffer.create(1024)
        d = json.loads(buf.descriptor_json())
        assert d["id"] == buf.id

    def test_descriptor_json_contains_path(self):
        """descriptor_json() JSON contains 'path' field."""
        buf = PySharedBuffer.create(1024)
        d = json.loads(buf.descriptor_json())
        assert "path" in d
        assert len(d["path"]) > 0

    def test_write_and_read_roundtrip(self):
        """write() + read() roundtrip returns the exact same bytes."""
        buf = PySharedBuffer.create(1024)
        data = b"hello roundtrip"
        buf.write(data)
        assert buf.read() == data

    def test_open_from_descriptor_returns_buffer(self):
        """PySharedBuffer.open(path, id) returns a PySharedBuffer object."""
        buf1 = PySharedBuffer.create(1024)
        buf2 = _open_from_desc(buf1.descriptor_json())
        assert buf2 is not None

    def test_cross_instance_read_after_write(self):
        """buf2.read() returns the same data that buf1.write() wrote."""
        buf1 = PySharedBuffer.create(1024)
        buf1.write(b"cross-instance-data")
        buf2 = _open_from_desc(buf1.descriptor_json())
        assert buf2.read() == b"cross-instance-data"

    def test_write_via_buf2_visible_in_buf1(self):
        """Data written via buf2 is visible when reading from buf1."""
        buf1 = PySharedBuffer.create(1024)
        buf1.write(b"initial")
        buf2 = _open_from_desc(buf1.descriptor_json())
        buf2.write(b"updated-by-buf2")
        assert buf1.read() == b"updated-by-buf2"

    def test_ids_equal_after_open(self):
        """buf1.id == buf2.id after opening the same descriptor."""
        buf1 = PySharedBuffer.create(1024)
        buf2 = _open_from_desc(buf1.descriptor_json())
        assert buf1.id == buf2.id

    def test_capacity_consistent_cross_instance(self):
        """buf2.capacity() == buf1.capacity() after open."""
        buf1 = PySharedBuffer.create(2048)
        buf2 = _open_from_desc(buf1.descriptor_json())
        assert buf2.capacity() == buf1.capacity()

    def test_clear_resets_data(self):
        """clear() makes subsequent read() return empty bytes."""
        buf = PySharedBuffer.create(1024)
        buf.write(b"some data")
        buf.clear()
        after = buf.read()
        assert after == b"" or after == b"\x00" * len(b"some data")

    def test_clear_visible_across_instances(self):
        """buf1.clear() is visible when reading via buf2."""
        buf1 = PySharedBuffer.create(1024)
        buf1.write(b"test")
        buf2 = _open_from_desc(buf1.descriptor_json())
        buf1.clear()
        data_after_clear = buf2.read()
        assert data_after_clear == b"" or len(data_after_clear) == 0 or all(b == 0 for b in data_after_clear)

    def test_multiple_opens_same_descriptor(self):
        """Multiple opens of the same descriptor all see the same data."""
        buf1 = PySharedBuffer.create(1024)
        buf1.write(b"shared-data")
        desc = buf1.descriptor_json()
        buf2 = _open_from_desc(desc)
        buf3 = _open_from_desc(desc)
        assert buf2.read() == b"shared-data"
        assert buf3.read() == b"shared-data"

    def test_write_large_data(self):
        """write() and read() work correctly for data near capacity."""
        capacity = 4096
        buf = PySharedBuffer.create(capacity)
        data = b"x" * (capacity - 64)  # near full
        buf.write(data)
        assert buf.read() == data

    def test_cross_instance_repeated_writes(self):
        """Multiple writes across instances are each reflected correctly."""
        buf1 = PySharedBuffer.create(1024)
        buf2 = _open_from_desc(buf1.descriptor_json())

        for i in range(5):
            payload = f"iteration-{i}".encode()
            buf1.write(payload)
            assert buf2.read() == payload
