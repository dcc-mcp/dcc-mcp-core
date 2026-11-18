"""Host capability manifest contract (issue #2867).

These tests are pure Python: they must pass in an embedded DCC interpreter that
has no native ``_core`` extension, so nothing here imports the Rust runtime.
"""

from __future__ import annotations

import json
from pathlib import Path

import jsonschema
import pytest

from dcc_mcp_core.capabilities import HOST_CAPABILITIES_SCHEMA_ID
from dcc_mcp_core.capabilities import HOST_CAPABILITIES_SCHEMA_VERSION
from dcc_mcp_core.capabilities import PROBE_FAIL
from dcc_mcp_core.capabilities import PROBE_NOT_RUN
from dcc_mcp_core.capabilities import PROBE_PASS
from dcc_mcp_core.capabilities import PROBE_UNSUPPORTED
from dcc_mcp_core.capabilities import HostCapabilityCache
from dcc_mcp_core.capabilities import HostCapabilityValidationError
from dcc_mcp_core.capabilities import build_host_capabilities
from dcc_mcp_core.capabilities import make_host_capability
from dcc_mcp_core.capabilities import register_host_capability_tool
from dcc_mcp_core.capabilities import summarize_host_capabilities
from dcc_mcp_core.capabilities import validate_host_capabilities
import dcc_mcp_core.host_capabilities as capabilities

SCHEMA_PATH = (
    Path(__file__).resolve().parent.parent / "python" / "dcc_mcp_core" / "schemas" / "host-capabilities-v1.schema.json"
)


def _manifest(capabilities):
    return build_host_capabilities(
        host_version="0.32.0",
        adapter_version="0.1.6",
        dcc_type="comfyui",
        instance_id="inst-1",
        capabilities=capabilities,
    )


def _capability(**overrides):
    base = {
        "key": "workflow.api_format",
        "supported": True,
        "probe_result": PROBE_PASS,
        "probed_at": "2026-11-18T00:00:00Z",
        "evidence_ref": "artifact://probe-1",
    }
    base.update(overrides)
    return make_host_capability(
        base["key"],
        base["supported"],
        base["probe_result"],
        reason=base.get("reason"),
        evidence_ref=base.get("evidence_ref"),
        probed_at=base.get("probed_at"),
        declaration_only=base.get("declaration_only", False),
    )


def _raw_capability(**overrides):
    record = {
        "key": "render.lpe",
        "supported": True,
        "probe_result": PROBE_PASS,
        "probed_at": "2026-11-18T00:00:00Z",
    }
    record.update(overrides)
    return record


def _raw_manifest(records):
    return {
        "schema_name": "dcc-mcp/host-capabilities",
        "schema_version": 1,
        "host_version": "0.32.0",
        "adapter_version": "0.1.6",
        "dcc_type": "comfyui",
        "capabilities": records,
    }


def _schema_validator():
    document = json.loads(SCHEMA_PATH.read_text(encoding="utf-8"))
    checker = jsonschema.FormatChecker()
    assert "date-time" not in checker.checkers
    return jsonschema.Draft202012Validator(document, format_checker=checker)


def _schema_is_valid(payload):
    return _schema_validator().is_valid(payload)


def _python_is_valid(payload):
    try:
        validate_host_capabilities(payload)
    except HostCapabilityValidationError:
        return False
    return True


def test_schema_document_is_packaged_and_pinned_to_this_contract():
    document = json.loads(SCHEMA_PATH.read_text(encoding="utf-8"))
    assert document["$id"] == HOST_CAPABILITIES_SCHEMA_ID
    assert document["properties"]["schema_version"]["const"] == (HOST_CAPABILITIES_SCHEMA_VERSION)
    assert document["properties"]["schema_name"]["const"] == "dcc-mcp/host-capabilities"
    for required in ("host_version", "adapter_version", "capabilities"):
        assert required in document["required"]
    capability = document["$defs"]["capability"]
    assert capability["properties"]["probe_result"]["enum"] == [
        PROBE_PASS,
        PROBE_FAIL,
        PROBE_UNSUPPORTED,
        PROBE_NOT_RUN,
    ]
    # The schema must stay closed so an adapter cannot smuggle free-form state.
    assert document["additionalProperties"] is False
    assert capability["additionalProperties"] is False
    assert capability["properties"]["supported"] == capabilities._hc_supported_schema()
    assert capability["properties"]["probed_at"] == capabilities._hc_timestamp_schema()
    assert capability["allOf"] == capabilities._hc_consistency_rules()


@pytest.mark.parametrize(
    "record",
    [
        _raw_capability(),
        _raw_capability(
            supported=False,
            probe_result=PROBE_UNSUPPORTED,
            reason="host has no editable VOP network",
        ),
        _raw_capability(
            supported=True,
            probe_result=PROBE_NOT_RUN,
            declaration_only=True,
            reason="adapter declares support; no probe implemented",
        ),
        _raw_capability(supported="6.8+"),
        _raw_capability(probed_at="2026-11-18T08:00:00+08:00"),
        _raw_capability(declaration_only=False),
        _raw_capability(probed_at="2024-02-29T00:00:00Z"),
        _raw_capability(probed_at="2000-02-29T00:00:00Z"),
        _raw_capability(probed_at="2026-12-31T00:00:00+23:59"),
    ],
)
def test_schema_and_python_accept_the_same_conformance_vectors(record):
    payload = _raw_manifest([record])
    assert _schema_is_valid(payload) is True
    assert _python_is_valid(payload) is True


@pytest.mark.parametrize(
    "record",
    [
        _raw_capability(probe_result=PROBE_FAIL),
        _raw_capability(supported=False, probe_result=PROBE_UNSUPPORTED),
        _raw_capability(
            supported=True,
            probe_result=PROBE_PASS,
            declaration_only=True,
            reason="unproven declaration",
        ),
        _raw_capability(supported=True, probe_result=PROBE_NOT_RUN),
        _raw_capability(supported="false"),
        _raw_capability(supported="0"),
        _raw_capability(probed_at="yesterday"),
        _raw_capability(probed_at="2026-11-18T00:00:00"),
        _raw_capability(probed_at="2026-02-30T00:00:00Z"),
        _raw_capability(probed_at="2026-02-29T00:00:00Z"),
        _raw_capability(probed_at="1900-02-29T00:00:00Z"),
        _raw_capability(probed_at="2026-04-31T00:00:00Z"),
        _raw_capability(probed_at="2026-12-31T24:00:00Z"),
        _raw_capability(probed_at="2026-12-31T00:60:00Z"),
        _raw_capability(probed_at="2026-12-31T23:59:60Z"),
        _raw_capability(probed_at="2026-12-31T00:00:00+24:00"),
        _raw_capability(probed_at="0000-02-29T00:00:00Z"),
    ],
)
def test_schema_and_python_reject_the_same_conformance_vectors(record):
    payload = _raw_manifest([record])
    assert _schema_is_valid(payload) is False
    assert _python_is_valid(payload) is False


def test_passing_probe_is_available():
    manifest = _manifest([_capability()])
    summary = summarize_host_capabilities(manifest)
    assert [entry["key"] for entry in summary["available"]] == ["workflow.api_format"]
    assert summary["totals"] == {
        "available": 1,
        "declared": 0,
        "unavailable": 0,
        "not_run": 0,
    }


def test_missing_probe_result_is_rejected():
    record = {
        "key": "render.lpe",
        "supported": True,
        "probed_at": "2026-11-18T00:00:00Z",
    }
    payload = _manifest([])
    payload["capabilities"] = [record]
    with pytest.raises(HostCapabilityValidationError, match="probe_result"):
        validate_host_capabilities(payload)


def test_claim_without_probe_requires_declaration_and_reason():
    with pytest.raises(HostCapabilityValidationError, match="declaration_only"):
        make_host_capability(
            "render.lpe",
            True,
            PROBE_NOT_RUN,
            probed_at="2026-11-18T00:00:00Z",
        )

    declared = make_host_capability(
        "render.lpe",
        True,
        PROBE_NOT_RUN,
        reason="adapter declares LPE support; no probe implemented yet",
        probed_at="2026-11-18T00:00:00Z",
        declaration_only=True,
    )
    summary = summarize_host_capabilities(_manifest([declared]))
    assert [entry["key"] for entry in summary["declared"]] == ["render.lpe"]
    assert summary["declared"][0]["declaration_only"] is True


def test_declaration_only_cannot_claim_a_passing_probe():
    with pytest.raises(HostCapabilityValidationError, match="PASS probe"):
        make_host_capability(
            "render.lpe",
            True,
            PROBE_PASS,
            reason="unproven declaration",
            probed_at="2026-11-18T00:00:00Z",
            declaration_only=True,
        )


@pytest.mark.parametrize("timestamp", ["yesterday", "2026-11-18T00:00:00"])
def test_invalid_probe_timestamp_is_rejected(timestamp):
    with pytest.raises(HostCapabilityValidationError, match="RFC 3339"):
        make_host_capability("render.lpe", True, PROBE_PASS, probed_at=timestamp)

    payload = _manifest([])
    payload["probed_at"] = timestamp
    with pytest.raises(HostCapabilityValidationError, match="RFC 3339"):
        validate_host_capabilities(payload)


def test_impossible_calendar_timestamps_are_rejected_at_both_levels():
    timestamps = (
        "2026-02-30T00:00:00Z",
        "2026-02-29T00:00:00Z",
        "1900-02-29T00:00:00Z",
        "2026-04-31T00:00:00Z",
        "2026-12-31T24:00:00Z",
        "2026-12-31T00:60:00Z",
        "2026-12-31T23:59:60Z",
        "2026-12-31T00:00:00+24:00",
        "0000-02-29T00:00:00Z",
    )
    for timestamp in timestamps:
        with pytest.raises(HostCapabilityValidationError, match="RFC 3339"):
            make_host_capability("render.lpe", True, PROBE_PASS, probed_at=timestamp)
        payload = _manifest([])
        payload["probed_at"] = timestamp
        with pytest.raises(HostCapabilityValidationError, match="RFC 3339"):
            validate_host_capabilities(payload)


@pytest.mark.parametrize("timestamp", ["2026-11-18T00:00:00Z", "2026-11-18T08:00:00+08:00"])
def test_valid_probe_timestamps_are_preserved(timestamp):
    record = make_host_capability("render.lpe", True, PROBE_PASS, probed_at=timestamp)
    assert record["probed_at"] == timestamp


def test_unsupported_capability_requires_a_concrete_reason():
    with pytest.raises(HostCapabilityValidationError, match="reason"):
        make_host_capability(
            "render.cryptomatte",
            False,
            PROBE_UNSUPPORTED,
            probed_at="2026-11-18T00:00:00Z",
        )

    record = make_host_capability(
        "render.cryptomatte",
        False,
        PROBE_UNSUPPORTED,
        reason="host build has no cryptomatte node",
        probed_at="2026-11-18T00:00:00Z",
    )
    summary = summarize_host_capabilities(_manifest([record]))
    assert summary["unavailable"][0]["reason"] == "host build has no cryptomatte node"


def test_failed_probe_cannot_be_declared_supported():
    with pytest.raises(HostCapabilityValidationError, match="claims support"):
        make_host_capability(
            "render.lpe",
            True,
            PROBE_FAIL,
            probed_at="2026-11-18T00:00:00Z",
        )


def test_passing_probe_cannot_be_declared_unsupported():
    with pytest.raises(HostCapabilityValidationError, match="passing probe"):
        make_host_capability(
            "render.lpe",
            False,
            PROBE_PASS,
            reason="contradiction",
            probed_at="2026-11-18T00:00:00Z",
        )


def test_not_run_is_never_counted_as_available():
    record = make_host_capability(
        "sim.rbd_field_writes",
        False,
        PROBE_NOT_RUN,
        reason="probe not implemented for this host yet",
        probed_at="2026-11-18T00:00:00Z",
    )
    summary = summarize_host_capabilities(_manifest([record]))
    assert summary["not_run"][0]["key"] == "sim.rbd_field_writes"
    assert summary["available"] == []


def test_version_string_support_is_preserved():
    record = make_host_capability(
        "capture.in_process_qt",
        "6.8+",
        PROBE_PASS,
        probed_at="2026-11-18T00:00:00Z",
    )
    summary = summarize_host_capabilities(_manifest([record]))
    assert summary["available"][0]["supported"] == "6.8+"


@pytest.mark.parametrize("supported", ["false", "False", "0", "no", "off"])
def test_false_like_support_strings_are_rejected(supported):
    with pytest.raises(HostCapabilityValidationError):
        make_host_capability("render.lpe", supported, PROBE_PASS, probed_at="2026-11-18T00:00:00Z")


def test_duplicate_keys_are_rejected():
    payload = {
        "schema_name": "dcc-mcp/host-capabilities",
        "schema_version": 1,
        "host_version": "0.32.0",
        "adapter_version": "0.1.6",
        "capabilities": [_capability(), _capability()],
    }
    with pytest.raises(HostCapabilityValidationError, match="duplicate"):
        validate_host_capabilities(payload)


@pytest.mark.parametrize(
    "field,value",
    [
        ("unexpected", "x"),
        ("declaration_only", "yes"),
        ("probed_at", ""),
        ("key", "NotNamespaced"),
    ],
)
def test_unknown_or_malformed_fields_fail_closed(field, value):
    record = _capability()
    record[field] = value
    payload = _manifest([])
    payload["capabilities"] = [record]
    with pytest.raises(HostCapabilityValidationError):
        validate_host_capabilities(payload)


def test_manifest_identity_and_versions_are_enforced():
    payload = _manifest([_capability()])
    payload["schema_version"] = 2
    with pytest.raises(HostCapabilityValidationError, match="schema_version"):
        validate_host_capabilities(payload)

    payload = _manifest([_capability()])
    payload["host_version"] = ""
    with pytest.raises(HostCapabilityValidationError, match="host_version"):
        validate_host_capabilities(payload)


def test_summary_can_be_narrowed_to_requested_keys():
    manifest = _manifest(
        [
            _capability(),
            make_host_capability(
                "queue.cancel_running",
                False,
                PROBE_UNSUPPORTED,
                reason="legacy interrupt is global, not per-prompt",
                probed_at="2026-11-18T00:00:00Z",
            ),
        ]
    )
    summary = summarize_host_capabilities(manifest, keys=["queue.cancel_running"])
    assert summary["totals"]["available"] == 0
    assert summary["unavailable"][0]["key"] == "queue.cancel_running"


def test_cache_expires_by_ttl_and_can_be_invalidated():
    now = [100.0]
    cache = HostCapabilityCache(ttl_secs=10, clock=lambda: now[0])
    key = HostCapabilityCache.cache_key("comfyui", "0.32.0", "0.1.6")
    payload = _manifest([_capability()])

    assert cache.is_stale(key)
    cache.put(key, payload)
    assert cache.get(key) is payload

    now[0] = 109.0
    assert cache.get(key) is payload
    now[0] = 111.0
    assert cache.get(key) is None
    assert cache.is_stale(key)

    cache.put(key, payload)
    cache.invalidate(key)
    assert cache.get(key) is None


def test_cache_key_includes_host_and_adapter_version():
    assert HostCapabilityCache.cache_key("comfyui", "0.32.0", "0.1.6") != (
        HostCapabilityCache.cache_key("comfyui", "0.33.0", "0.1.6")
    )
    assert HostCapabilityCache.cache_key("comfyui", "0.32.0", "0.1.6") != (
        HostCapabilityCache.cache_key("maya", "0.32.0", "0.1.6")
    )


def test_cache_key_cannot_collide_on_delimiter_strings():
    assert HostCapabilityCache.cache_key("comfyui", "1|2", "3") != (
        HostCapabilityCache.cache_key("comfyui", "1", "2|3")
    )


def test_cache_rejects_invalid_ttl():
    with pytest.raises(HostCapabilityValidationError):
        HostCapabilityCache(ttl_secs=-1)


class _Registry:
    def __init__(self):
        self.registered = []
        self.handlers = {}

    def register(self, name, **kwargs):
        self.registered.append((name, kwargs))

    def register_handler(self, name, handler):
        self.handlers[name] = handler


class _Inner:
    def __init__(self):
        self.registry = _Registry()

    def register_handler(self, name, handler):
        self.registry.register_handler(name, handler)


class _Server:
    def __init__(self):
        self._server = _Inner()


def test_tool_registration_serves_the_summary_from_a_provider():
    server = _Server()
    calls = {"count": 0}

    def provider():
        calls["count"] += 1
        return _manifest([_capability()])

    assert register_host_capability_tool(server, dcc_name="comfyui", provider=provider) is True
    name = server._server.registry.registered[0][0]
    assert name == "dcc_host_capabilities"

    handler = server._server.registry.handlers[name]
    result = handler({})
    assert result["success"] is True
    assert result["context"]["totals"]["available"] == 1
    assert calls["count"] == 1


def test_tool_cache_uses_current_identity_and_does_not_stale_on_version_change():
    server = _Server()
    versions = iter(["20.5", "21.0", "21.0"])
    calls = []

    def current_identity():
        return ("comfyui", "20.5" if len(calls) == 0 else "21.0", "0.1.6")

    def provider():
        version = next(versions)
        calls.append(version)
        manifest = _manifest([_capability()])
        manifest["host_version"] = version
        manifest["adapter_version"] = "0.1.6"
        return manifest

    cache = HostCapabilityCache(ttl_secs=300)
    assert register_host_capability_tool(
        server,
        dcc_name="comfyui",
        provider=provider,
        cache=cache,
        provider_cache_key=current_identity,
    )
    handler = server._server.registry.handlers["dcc_host_capabilities"]

    first = handler({})
    second = handler({})
    third = handler({})
    assert [item["context"]["host_version"] for item in (first, second, third)] == [
        "20.5",
        "21.0",
        "21.0",
    ]
    assert calls == ["20.5", "21.0"]


def test_tool_returns_structured_error_when_provider_fails():
    server = _Server()

    def provider():
        raise RuntimeError("host bridge unavailable")

    assert register_host_capability_tool(server, dcc_name="comfyui", provider=provider)
    handler = server._server.registry.handlers["dcc_host_capabilities"]
    result = handler({})
    assert result["success"] is False
    assert result["error"] == "host_capability_provider_failed"
    assert result["context"]["exception_type"] == "RuntimeError"


def test_tool_registration_is_skipped_without_a_server_registry():
    class _Bare:
        pass

    assert register_host_capability_tool(_Bare(), dcc_name="comfyui", provider=lambda: _manifest([])) is False
