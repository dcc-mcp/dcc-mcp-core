"""Contract tests for the behavior-verification state-export schemas (core#2269)."""

from __future__ import annotations

import json

import pytest

from dcc_mcp_core.verification import SCHEMA_ANIM_CURVES
from dcc_mcp_core.verification import SCHEMA_GRAPH_STATE
from dcc_mcp_core.verification import SCHEMA_RIG_STATE
from dcc_mcp_core.verification import SCHEMA_SIM_STATUS
from dcc_mcp_core.verification import SchemaValidationError
from dcc_mcp_core.verification import sample_curve
from dcc_mcp_core.verification import schema_document
from dcc_mcp_core.verification import schema_names
from dcc_mcp_core.verification import validate_state_export
from dcc_mcp_core.verification import value_at


def _anim_curves_payload():
    return {
        "schema_name": "dcc-mcp/anim-curves",
        "schema_version": 1,
        "curves": [
            {
                "target": "rotor_main.rotateY",
                "key_count": 3,
                "times": [0.0, 12.0, 24.0],
                "values": [0.0, 180.0, 360.0],
                "tangents_in": [0.0, 0.0, 0.0],
                "tangents_out": [0.0, 0.0, 0.0],
                "infinity_pre": "constant",
                "infinity_post": "constant",
            }
        ],
    }


def _rig_state_payload():
    return {
        "schema_name": "dcc-mcp/rig-state",
        "schema_version": 1,
        "joints": {
            "count": 2,
            "hierarchy": [
                {"name": "root", "parent": None},
                {"name": "blade_pivot", "parent": "root"},
            ],
        },
        "constraints": [{"target": "aim_ctrl", "kind": "aim"}],
        "skins": [{"mesh": "body", "influences": 4, "unnormalized_vertices": 0}],
    }


def _sim_status_payload():
    return {
        "schema_name": "dcc-mcp/sim-status",
        "schema_version": 1,
        "cache_exists": True,
        "cache_path": "/tmp/sim.cache",
        "frame_count": 2,
        "per_frame_counts": [
            {"frame": 1, "particle_count": 100, "vertex_count": 50},
            {"frame": 2, "particle_count": 98, "vertex_count": 50},
        ],
    }


def _graph_state_payload():
    return {
        "schema_name": "dcc-mcp/graph-state",
        "schema_version": 1,
        "nodes": [{"name": "Read1", "type": "Read"}, {"name": "Blur1", "type": "Blur"}],
        "connections": [{"from": "Read1", "to": "Blur1"}],
        "params": [{"node": "Blur1", "name": "size", "value": 10}],
        "outputs": [{"node": "Blur1", "name": "rgba"}],
    }


class TestSchemaRegistry:
    def test_schema_names_are_the_four_vocabularies(self):
        assert set(schema_names()) == {
            SCHEMA_ANIM_CURVES,
            SCHEMA_RIG_STATE,
            SCHEMA_SIM_STATUS,
            SCHEMA_GRAPH_STATE,
        }

    def test_schema_documents_are_valid_json_objects(self):
        for name in schema_names():
            document = schema_document(name)
            assert isinstance(document, dict)
            assert document.get("schema_version") is None or document.get("$schema")
            # The JSON Schema must be parseable as JSON (round-trip stable).
            assert json.loads(json.dumps(document)) == document

    def test_schema_document_unknown_name_raises_keyerror(self):
        with pytest.raises(KeyError):
            schema_document("dcc-mcp/nope")


class TestAnimCurves:
    def test_valid_payload_passes(self):
        validate_state_export(_anim_curves_payload(), SCHEMA_ANIM_CURVES)

    def test_key_count_must_match_times_length(self):
        payload = _anim_curves_payload()
        payload["curves"][0]["key_count"] = 99
        with pytest.raises(SchemaValidationError, match="key_count"):
            validate_state_export(payload, SCHEMA_ANIM_CURVES)

    def test_values_length_must_match_times_length(self):
        payload = _anim_curves_payload()
        payload["curves"][0]["values"] = [0.0, 360.0]
        with pytest.raises(SchemaValidationError, match="values length"):
            validate_state_export(payload, SCHEMA_ANIM_CURVES)

    def test_schema_name_mismatch_rejected(self):
        payload = _anim_curves_payload()
        payload["schema_name"] = "dcc-mcp/rig-state"
        with pytest.raises(SchemaValidationError, match="schema_name mismatch"):
            validate_state_export(payload, SCHEMA_ANIM_CURVES)

    def test_unsupported_version_rejected(self):
        payload = _anim_curves_payload()
        payload["schema_version"] = 2
        with pytest.raises(SchemaValidationError, match="schema_version"):
            validate_state_export(payload, SCHEMA_ANIM_CURVES)

    def test_value_at_interpolates_and_clamps(self):
        payload = _anim_curves_payload()
        assert value_at(payload, t=24) == pytest.approx(360.0)
        assert value_at(payload, t=12) == pytest.approx(180.0)
        assert value_at(payload, t=6) == pytest.approx(90.0)
        assert value_at(payload, t=-5) == pytest.approx(0.0)  # clamp below first key
        assert value_at(payload, t=30) == pytest.approx(360.0)  # clamp above last key

    def test_value_at_selects_target_curve(self):
        payload = _anim_curves_payload()
        payload["curves"].append(
            {
                "target": "rotor_main.translateX",
                "key_count": 2,
                "times": [0.0, 24.0],
                "values": [10.0, 20.0],
            }
        )
        assert value_at(payload, t=24, target="rotor_main.translateX") == pytest.approx(20.0)
        assert value_at(payload, t=24, target="rotor_main.rotateY") == pytest.approx(360.0)

    def test_sample_curve_rejects_empty(self):
        with pytest.raises(SchemaValidationError, match="empty curve"):
            sample_curve({"times": [], "values": []}, 0.0)


class TestRigState:
    def test_valid_payload_passes(self):
        validate_state_export(_rig_state_payload(), SCHEMA_RIG_STATE)

    def test_requires_joint_count(self):
        payload = _rig_state_payload()
        del payload["joints"]["count"]
        with pytest.raises(SchemaValidationError, match="count"):
            validate_state_export(payload, SCHEMA_RIG_STATE)

    def test_skin_requires_unnormalized_vertices(self):
        payload = _rig_state_payload()
        del payload["skins"][0]["unnormalized_vertices"]
        with pytest.raises(SchemaValidationError, match="unnormalized_vertices"):
            validate_state_export(payload, SCHEMA_RIG_STATE)


class TestSimStatus:
    def test_valid_payload_passes(self):
        validate_state_export(_sim_status_payload(), SCHEMA_SIM_STATUS)

    def test_requires_cache_exists_and_frame_count(self):
        payload = _sim_status_payload()
        del payload["cache_exists"]
        with pytest.raises(SchemaValidationError, match="cache_exists"):
            validate_state_export(payload, SCHEMA_SIM_STATUS)


class TestGraphState:
    def test_valid_payload_passes(self):
        validate_state_export(_graph_state_payload(), SCHEMA_GRAPH_STATE)

    def test_requires_node_names(self):
        payload = _graph_state_payload()
        del payload["nodes"][0]["name"]
        with pytest.raises(SchemaValidationError, match="name"):
            validate_state_export(payload, SCHEMA_GRAPH_STATE)

    def test_connection_requires_from_and_to(self):
        payload = _graph_state_payload()
        del payload["connections"][0]["to"]
        with pytest.raises(SchemaValidationError, match="to"):
            validate_state_export(payload, SCHEMA_GRAPH_STATE)


class TestSchemaIdentityIsRequired:
    """``schema_name``/``schema_version`` are ``required`` in every packaged document.

    A payload that omits them is malformed, not merely untagged, and a boolean
    must never satisfy an integer version check (``True == 1`` in Python).
    """

    def test_missing_schema_name_rejected(self):
        payload = _rig_state_payload()
        del payload["schema_name"]
        with pytest.raises(SchemaValidationError, match="schema_name"):
            validate_state_export(payload, SCHEMA_RIG_STATE)

    def test_missing_schema_version_rejected(self):
        payload = _rig_state_payload()
        del payload["schema_version"]
        with pytest.raises(SchemaValidationError, match="schema_version"):
            validate_state_export(payload, SCHEMA_RIG_STATE)

    def test_boolean_schema_version_rejected(self):
        payload = _rig_state_payload()
        payload["schema_version"] = True
        with pytest.raises(SchemaValidationError, match="schema_version"):
            validate_state_export(payload, SCHEMA_RIG_STATE)

    def test_non_string_schema_name_rejected(self):
        payload = _rig_state_payload()
        payload["schema_name"] = 1
        with pytest.raises(SchemaValidationError, match="schema_name"):
            validate_state_export(payload, SCHEMA_RIG_STATE)


class TestCountFieldsAreNonNegativeIntegers:
    """Count fields mirror the packaged ``{"type": "integer", "minimum": 0}``.

    ``bool`` is a subclass of ``int``, so ``isinstance(value, int)`` alone would
    accept ``True`` as a count; the validator must reject it explicitly.
    """

    def test_negative_joint_count_rejected(self):
        payload = _rig_state_payload()
        payload["joints"]["count"] = -1
        with pytest.raises(SchemaValidationError, match="count"):
            validate_state_export(payload, SCHEMA_RIG_STATE)

    def test_negative_skin_influences_rejected(self):
        payload = _rig_state_payload()
        payload["skins"][0]["influences"] = -2
        with pytest.raises(SchemaValidationError, match="influences"):
            validate_state_export(payload, SCHEMA_RIG_STATE)

    def test_negative_unnormalized_vertices_rejected(self):
        payload = _rig_state_payload()
        payload["skins"][0]["unnormalized_vertices"] = -1
        with pytest.raises(SchemaValidationError, match="unnormalized_vertices"):
            validate_state_export(payload, SCHEMA_RIG_STATE)

    def test_boolean_counts_rejected(self):
        payload = _rig_state_payload()
        payload["joints"]["count"] = True
        with pytest.raises(SchemaValidationError, match="count"):
            validate_state_export(payload, SCHEMA_RIG_STATE)

        payload = _rig_state_payload()
        payload["skins"][0]["influences"] = False
        with pytest.raises(SchemaValidationError, match="influences"):
            validate_state_export(payload, SCHEMA_RIG_STATE)

    def test_negative_key_count_rejected(self):
        payload = _anim_curves_payload()
        payload["curves"][0]["key_count"] = -3
        with pytest.raises(SchemaValidationError, match="key_count"):
            validate_state_export(payload, SCHEMA_ANIM_CURVES)

    def test_negative_frame_count_rejected(self):
        payload = _sim_status_payload()
        payload["frame_count"] = -1
        with pytest.raises(SchemaValidationError, match="frame_count"):
            validate_state_export(payload, SCHEMA_SIM_STATUS)

    def test_zero_counts_are_valid(self):
        payload = _rig_state_payload()
        payload["joints"]["count"] = 0
        payload["skins"][0]["influences"] = 0
        payload["skins"][0]["unnormalized_vertices"] = 0
        validate_state_export(payload, SCHEMA_RIG_STATE)


class TestValidationGuards:
    def test_non_mapping_payload_raises_typeerror(self):
        with pytest.raises(TypeError):
            validate_state_export(["not", "a", "mapping"], SCHEMA_ANIM_CURVES)

    def test_unknown_schema_raises_keyerror(self):
        with pytest.raises(KeyError):
            validate_state_export({}, "dcc-mcp/nope")
