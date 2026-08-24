from __future__ import annotations

import re

import pytest


def test_shared_finding_v1_schema_has_the_required_contract() -> None:
    from dcc_mcp_core.schemas.finding import FINDING_V1_SCHEMA_VERSION
    from dcc_mcp_core.schemas.finding import finding_v1_json_schema

    schema = finding_v1_json_schema()
    assert FINDING_V1_SCHEMA_VERSION == 1
    assert schema["properties"]["schema_version"] == {"type": "integer", "const": 1}
    assert schema["properties"]["severity"]["enum"] == [
        "blocker",
        "degraded",
        "workaround_found",
        "suggestion",
    ]
    assert {
        "schema_version",
        "fingerprint",
        "dcc_type",
        "adapter",
        "adapter_version",
        "core_version",
        "host_version",
        "os",
        "phase",
        "severity",
        "intent",
        "observed",
        "expected",
        "repro",
        "evidence",
        "redaction_status",
    }.issubset(schema["required"])


def test_finding_fingerprint_normalizes_owner_tool_and_host_major() -> None:
    from dcc_mcp_core.schemas.finding import finding_fingerprint

    first = finding_fingerprint(
        owning_repo="https://github.com/dcc-mcp/dcc-mcp-photoshop.git",
        phase="skill",
        tool_slug="photoshop_layers__merge",
        error_kind=None,
        host_version="26.4.1",
    )
    equivalent = finding_fingerprint(
        owning_repo="dcc-mcp/dcc-mcp-photoshop/",
        phase="skill",
        tool_slug="PHOTOSHOP_LAYERS__MERGE",
        error_kind=None,
        host_version="26.4",
    )

    assert first == equivalent
    assert first == "sha256:0f7545056e6e3ee9387804a6276669a21c723f8a422b59477e3587be6a8997ac"
    assert re.fullmatch(r"sha256:[0-9a-f]{64}", first)


def test_build_finding_v1_autofills_runtime_identity_and_keeps_custom_dcc() -> None:
    from dcc_mcp_core.schemas.finding import FindingRuntimeContext
    from dcc_mcp_core.schemas.finding import build_finding_v1

    finding = build_finding_v1(
        {
            "phase": "dispatch",
            "severity": "blocker",
            "tool_slug": "studio_host_scene__save",
            "intent": "Save the scene",
            "observed": "The dispatcher rejected the request",
            "expected": "The scene is saved",
            "repro": {"steps": ["Call the save tool"]},
            "evidence": {"error_kind": "dispatcher_rejected", "request_id": "request-42"},
        },
        FindingRuntimeContext(
            dcc_type="Studio Host Custom",
            adapter="studio-dcc-adapter",
            adapter_version="3.4.5",
            core_version="0.20.11",
            host_version="2026.2",
            os="windows",
            owning_repo="studio/studio-dcc-adapter",
        ),
    )

    assert finding["schema_version"] == 1
    assert finding["dcc_type"] == "Studio Host Custom"
    assert finding["adapter"] == "studio-dcc-adapter"
    assert finding["adapter_version"] == "3.4.5"
    assert finding["core_version"] == "0.20.11"
    assert finding["host_version"] == "2026.2"
    assert finding["os"] == "windows"
    assert finding["redaction_status"]["mode"] == "needs-review"
    assert re.fullmatch(r"sha256:[0-9a-f]{64}", finding["fingerprint"])


@pytest.mark.parametrize(
    "update",
    [
        {"repro": {"argv": ["dcc-mcp-cli"], "steps": ["Call the tool"]}},
        {"repro": {"steps": []}},
        {"tool_slug": None, "evidence": {}},
        {"severity": "critical"},
        {"runtime_claim": "agent-supplied"},
    ],
)
def test_build_finding_v1_rejects_unbounded_or_ambiguous_input(update) -> None:
    from dcc_mcp_core.schemas.finding import FindingRuntimeContext
    from dcc_mcp_core.schemas.finding import FindingValidationError
    from dcc_mcp_core.schemas.finding import build_finding_v1

    args = {
        "phase": "dispatch",
        "severity": "blocker",
        "tool_slug": "custom_scene__save",
        "intent": "Save the scene",
        "observed": "The call failed",
        "expected": "The scene is saved",
        "repro": {"steps": ["Call save"]},
        "evidence": {"error_kind": "save_failed"},
    }
    args.update(update)
    context = FindingRuntimeContext(
        dcc_type="custom",
        adapter="custom-adapter",
        adapter_version="1.2.3",
        core_version="0.20.11",
        host_version="7.0",
        os="linux",
        owning_repo="studio/custom-adapter",
    )

    with pytest.raises(FindingValidationError):
        build_finding_v1(args, context)
