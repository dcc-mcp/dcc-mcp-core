"""Tests for shared Python version helpers."""

from dcc_mcp_core._version_util import parse_semver


def test_parse_semver_normalizes_supported_versions():
    assert parse_semver("0.18.15") == (0, 18, 15)
    assert parse_semver("v2.3") == (2, 3, 0)
    assert parse_semver("5") == (5, 0, 0)
    assert parse_semver("V1.2.3-rc1") == (1, 2, 3)
    assert parse_semver("1.2.3+studio.4") == (1, 2, 3)


def test_parse_semver_rejects_invalid_numeric_core():
    assert parse_semver("") is None
    assert parse_semver("release") is None
    assert parse_semver("1.two.3") is None
    assert parse_semver("1..3") is None
