"""Validate the cross-DCC modeling adoption matrix contract."""

import json
from pathlib import Path

MATRIX_PATH = Path(__file__).resolve().parents[1] / "docs" / "guide" / "cross-dcc-modeling-adoption.json"


def test_adoption_matrix_tracks_required_adapters_and_evidence() -> None:
    matrix = json.loads(MATRIX_PATH.read_text(encoding="utf-8"))

    assert matrix["schema_version"] == 1
    assert matrix["contract"] == "dcc-mcp/model-state@1"
    assert matrix["status_values"] == ["pass", "fail", "unknown"]
    assert matrix["canonical_checks"] == [
        "parts",
        "hierarchy",
        "materials",
        "uv_coverage",
        "non_manifold",
        "euler",
    ]
    assert matrix["workflow_stages"] == [
        "resolve_entry_skill",
        "inspect_scene",
        "apply_bounded_operations",
        "read_back_state",
        "verify_reference_evidence",
    ]

    adapters = {row["dcc"]: row for row in matrix["adapters"]}
    assert set(adapters) == {"blender", "houdini", "maya", "3dsmax", "zbrush"}
    for row in adapters.values():
        assert row["repository"].startswith("https://github.com/dcc-mcp/")
        for field in (
            "typed_coverage",
            "readback_coverage",
            "reference_fit",
            "real_dcc_smoke",
        ):
            assert row[field] in matrix["status_values"]
        if any(
            row[field] == "pass"
            for field in (
                "typed_coverage",
                "readback_coverage",
                "reference_fit",
                "real_dcc_smoke",
            )
        ):
            assert row["evidence"], f"pass status without evidence for {row['dcc']}"
