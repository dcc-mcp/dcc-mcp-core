"""Tests for the dual-layer assertion library (core#2269)."""

from __future__ import annotations

import pytest

from dcc_mcp_core.verification import SCHEMA_ANIM_CURVES
from dcc_mcp_core.verification import AssertionFailure
from dcc_mcp_core.verification import BehaviorVerifier
from dcc_mcp_core.verification import assert_exact
from dcc_mcp_core.verification import assert_exists
from dcc_mcp_core.verification import assert_in_band
from dcc_mcp_core.verification import assert_resolution
from dcc_mcp_core.verification import assert_state_schema
from dcc_mcp_core.verification import assert_within


def _anim_curves_payload(key_count=3):
    return {
        "schema_name": "dcc-mcp/anim-curves",
        "schema_version": 1,
        "curves": [
            {
                "target": "rotor_main.rotateY",
                "key_count": key_count,
                "times": [0.0, 12.0, 24.0],
                "values": [0.0, 180.0, 360.0],
            }
        ],
    }


class TestFailFastAssertions:
    def test_assert_exact_passes_and_fails(self):
        assert_exact(12, 12, name="joint_count")
        with pytest.raises(AssertionFailure) as exc:
            assert_exact(13, 12, name="joint_count")
        assert exc.value.expected == 12
        assert exc.value.actual == 13
        assert exc.value.kind == "exact"

    def test_assert_within_tolerance(self):
        assert_within(360.0005, 360.0, 1e-3, name="rotateY_at_24")
        with pytest.raises(AssertionFailure):
            assert_within(361.0, 360.0, 1e-3, name="rotateY_at_24")

    def test_assert_within_relative(self):
        # +/-5% of 100 -> +/-5
        assert_within(104, 100, 0.05, relative=True)
        with pytest.raises(AssertionFailure):
            assert_within(106, 100, 0.05, relative=True)

    def test_assert_in_band(self):
        assert_in_band(0.3, 0.18, 0.55, name="mean_luma")
        with pytest.raises(AssertionFailure):
            assert_in_band(0.05, 0.18, 0.55, name="mean_luma")

    def test_assert_exists(self, tmp_path):
        target = tmp_path / "frame.exr"
        target.write_bytes(b"stub")
        assert_exists(str(target), name="render_output")
        with pytest.raises(AssertionFailure):
            assert_exists(str(tmp_path / "missing.exr"), name="render_output")

    def test_assert_resolution(self):
        assert_resolution(1920, 1080, name="frame", min_width=1280, min_height=720)
        with pytest.raises(AssertionFailure):
            assert_resolution(640, 1080, name="frame", min_width=1280, min_height=720)

    def test_assert_state_schema(self):
        assert_state_schema(_anim_curves_payload(), SCHEMA_ANIM_CURVES, name="curves")
        bad = _anim_curves_payload(key_count=99)
        with pytest.raises(AssertionFailure) as exc:
            assert_state_schema(bad, SCHEMA_ANIM_CURVES, name="curves")
        assert exc.value.kind == "schema"


class TestBehaviorVerifier:
    def test_report_aggregates_pass_rate(self):
        verify = BehaviorVerifier("rotor")
        verify.exact("joint_count", 12, 12)
        verify.within("rotateY_at_24", 360.0005, 360.0, 1e-3)
        verify.exact("key_count", 99, 3)  # fails
        report = verify.report()

        assert report.total == 3
        assert report.passed == 2
        assert report.failed == 1
        assert report.pass_rate == pytest.approx(2 / 3.0)
        assert report.to_dict()["checks"][0]["name"] == "joint_count"

    def test_empty_report_is_vacuously_passing(self):
        report = BehaviorVerifier().report()
        assert report.pass_rate == 1.0
        assert report.total == 0

    def test_verifier_passed_flag(self):
        verify = BehaviorVerifier()
        verify.exact("a", 1, 1)
        assert verify.passed() is True
        verify.exact("b", 1, 2)
        assert verify.passed() is False

    def test_within_band_and_resolution_recorded(self):
        verify = BehaviorVerifier()
        verify.within_band("mean_luma", 0.3, 0.18, 0.55)
        verify.resolution("frame", 1920, 1080, min_width=1280)
        report = verify.report()
        assert report.passed == 2
        assert {check.kind for check in report.checks} == {"tolerance", "resolution"}

    def test_negative_tolerance_rejected(self):
        with pytest.raises(ValueError):
            BehaviorVerifier().within("x", 1.0, 1.0, -0.1)
        with pytest.raises(ValueError):
            assert_within(1.0, 1.0, -0.1)

    def test_inverted_band_rejected(self):
        with pytest.raises(ValueError):
            BehaviorVerifier().within_band("x", 1.0, 2.0, 1.0)


class TestSchemaCheckFailureIsRecordedNotRaised:
    def test_verifier_schema_records_failure(self):
        verify = BehaviorVerifier()
        verify.schema("curves", _anim_curves_payload(key_count=99), SCHEMA_ANIM_CURVES)
        report = verify.report()
        assert report.failed == 1
        assert report.checks[0].kind == "schema"
        assert "key_count" in report.checks[0].message
