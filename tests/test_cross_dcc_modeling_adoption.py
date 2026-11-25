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

    rows = matrix["adapters"]
    identifiers = [row["dcc"] for row in rows]
    assert len(identifiers) == len(set(identifiers))
    adapters = {row["dcc"]: row for row in rows}
    assert set(adapters) == {"blender", "houdini", "maya", "3dsmax", "zbrush"}
    expected_issues = {
        "blender": "https://github.com/dcc-mcp/dcc-mcp-blender/issues/371",
        "houdini": "https://github.com/dcc-mcp/dcc-mcp-houdini/issues/384",
        "maya": "https://github.com/dcc-mcp/dcc-mcp-maya/issues/612",
        "3dsmax": "https://github.com/dcc-mcp/dcc-mcp-3dsmax/issues/242",
        "zbrush": "https://github.com/dcc-mcp/dcc-mcp-zbrush/issues/115",
    }
    for row in adapters.values():
        assert row["repository"].startswith("https://github.com/dcc-mcp/")
        assert row["implementation_issue"] == expected_issues[row["dcc"]]
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
            assert isinstance(row["evidence"], list)
            assert all(isinstance(link, str) and link.strip() for link in row["evidence"])
            assert row["evidence"], f"pass status without evidence for {row['dcc']}"
