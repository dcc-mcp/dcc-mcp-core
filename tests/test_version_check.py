"""Tests for the ``dcc-mcp-core`` version provenance self-check (PIP-3536).

Covers the three behaviours the issue asks for:

1. the two version answers (compiled extension vs installed distribution) are
   compared, and a disagreement is reported rather than silently printed;
2. a drift report names **which side is stale** and **where the distribution
   lives**, so an operator can delete the right artifact;
3. the check is pure stdlib and works on Python 3.7 (Maya 2022), where
   :mod:`importlib.metadata` may not exist.
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
import logging
import sys
from typing import Any

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core import _version_check
from dcc_mcp_core.constants import ENV_VERSION_CHECK

FAKE_DIST = "/opt/site-packages/dcc_mcp_core-0.19.3.dist-info"
FAKE_EXTENSION = "/opt/site-packages/dcc_mcp_core/_core.cp311-win_amd64.pyd"

#: Sentinel meaning "importlib.metadata is importable" (the common case).
#: ``None`` simulates a Python 3.7 host with no backport installed — the
#: condition the pure-stdlib ``sys.path`` scan exists for.
_METADATA_PRESENT = object()


@pytest.fixture()
def isolated(monkeypatch: pytest.MonkeyPatch):
    """Force both answers to be injected, never read from the live host."""

    def _install(
        *,
        runtime: str | None = None,
        installed: str | None = None,
        installed_path: str | None = None,
        module_path: str | None = None,
        metadata_module: Any = _METADATA_PRESENT,
    ) -> dict[str, Any]:
        monkeypatch.setattr(_version_check, "native_version", lambda load=False: runtime)
        monkeypatch.setattr(_version_check, "native_origin", lambda: module_path)
        monkeypatch.setattr(_version_check, "distribution_version", lambda package=_version_check.PACKAGE: installed)
        monkeypatch.setattr(
            _version_check, "distribution_location", lambda package=_version_check.PACKAGE: installed_path
        )
        monkeypatch.setattr(_version_check, "_metadata_module", lambda: metadata_module)
        return {
            "runtime_version": runtime,
            "installed_version": installed,
            "installed_path": installed_path,
            "module_path": module_path,
        }

    return _install


# ── status classification ───────────────────────────────────────────────────


def test_consistent_when_both_answers_agree(isolated):
    isolated(runtime="0.20.39", installed="0.20.39", installed_path=FAKE_DIST, module_path=FAKE_EXTENSION)
    report = _version_check.build_report()

    assert report.status == _version_check.STATUS_CONSISTENT
    assert report.consistent is True
    assert report.drift is False
    assert report.stale_side == ""
    assert report.detail == ""
    assert report.authoritative_version == "0.20.39"


def test_punctuation_differences_are_not_drift(isolated):
    """``0.19.2`` vs ``0.19.02`` is one version written two ways."""
    isolated(runtime="0.19.2", installed="0.19.02")
    assert _version_check.build_report().status == _version_check.STATUS_CONSISTENT


def test_mismatch_when_extension_is_older(isolated):
    """The field report: ``0.19.3`` metadata next to a ``0.19.2`` extension."""
    isolated(runtime="0.19.2", installed="0.19.3", installed_path=FAKE_DIST, module_path=FAKE_EXTENSION)
    report = _version_check.build_report()

    assert report.status == _version_check.STATUS_MISMATCH
    assert report.drift is True
    assert report.consistent is False
    assert report.stale_side == _version_check.STALE_RUNTIME
    assert report.runtime_version == "0.19.2"
    assert report.distribution_version == "0.19.3"


def test_mismatch_when_distribution_is_older(isolated):
    isolated(runtime="0.20.39", installed="0.19.3", installed_path=FAKE_DIST)
    report = _version_check.build_report()

    assert report.drift is True
    assert report.stale_side == _version_check.STALE_DISTRIBUTION


def test_minor_versions_order_numerically_not_lexicographically(isolated):
    """``0.19.30`` is newer than ``0.19.2``; ``str`` comparison gets it wrong."""
    isolated(runtime="0.19.30", installed="0.19.2")
    assert _version_check.build_report().stale_side == _version_check.STALE_DISTRIBUTION


def test_unorderable_versions_report_unknown_stale_side(isolated):
    isolated(runtime="0.0.0-dev", installed="unknown")
    report = _version_check.build_report()

    assert report.drift is True
    assert report.stale_side == _version_check.STALE_UNKNOWN


def test_not_installed_when_metadata_is_missing(isolated):
    isolated(runtime="0.20.39", installed=None, metadata_module=object())
    report = _version_check.build_report()

    assert report.status == _version_check.STATUS_NOT_INSTALLED
    assert report.drift is False
    assert report.authoritative_version == "0.20.39"
    assert "no installed distribution metadata" in report.detail


def test_unknown_when_no_metadata_source_exists(isolated):
    """Python 3.7 with no backport and nothing on ``sys.path``."""
    isolated(runtime="0.20.39", installed=None, metadata_module=None)
    report = _version_check.build_report()

    assert report.status == _version_check.STATUS_UNKNOWN
    assert "neither importlib.metadata nor the importlib_metadata backport" in report.detail


def test_unknown_when_extension_is_not_loaded(isolated):
    isolated(runtime=None, installed="0.20.39", installed_path=FAKE_DIST)
    report = _version_check.build_report()

    assert report.status == _version_check.STATUS_UNKNOWN
    assert report.runtime_version is None
    assert report.authoritative_version == "0.20.39"
    assert "native extension not loaded" in report.detail


# ── drift report content (acceptance: name the stale side + the path) ───────


def test_drift_detail_names_both_versions_and_the_remedy(isolated):
    isolated(runtime="0.19.2", installed="0.19.3", installed_path=FAKE_DIST, module_path=FAKE_EXTENSION)
    detail = _version_check.build_report().detail

    assert "0.19.3" in detail
    assert "0.19.2" in detail
    assert "older than the installed distribution" in detail
    assert "reinstall dcc-mcp-core" in detail


def test_drift_detail_for_stale_distribution(isolated):
    isolated(runtime="0.20.39", installed="0.19.3", installed_path=FAKE_DIST)
    detail = _version_check.build_report().detail

    assert "older than the loaded native extension" in detail
    assert "upgrade dcc-mcp-core" in detail


def test_report_payload_carries_the_distribution_path(isolated):
    isolated(runtime="0.19.2", installed="0.19.3", installed_path=FAKE_DIST, module_path=FAKE_EXTENSION)
    payload = _version_check.build_report().to_dict()

    assert payload["distribution_path"] == FAKE_DIST
    assert payload["module_path"] == FAKE_EXTENSION
    assert payload["drift"] is True
    assert payload["stale_side"] == _version_check.STALE_RUNTIME


def test_drift_warning_logs_both_paths(isolated, caplog: pytest.LogCaptureFixture):
    isolated(runtime="0.19.2", installed="0.19.3", installed_path=FAKE_DIST, module_path=FAKE_EXTENSION)
    report = _version_check.build_report()

    with caplog.at_level(logging.WARNING, logger="dcc_mcp_core._version_check"):
        _version_check.log_report(report)

    assert "version drift" in caplog.text
    assert FAKE_DIST in caplog.text
    assert FAKE_EXTENSION in caplog.text
    assert "0.19.2" in caplog.text
    assert "0.19.3" in caplog.text


def test_consistent_report_logs_at_info(isolated, caplog: pytest.LogCaptureFixture):
    isolated(runtime="0.20.39", installed="0.20.39", installed_path=FAKE_DIST, module_path=FAKE_EXTENSION)
    report = _version_check.build_report()

    with caplog.at_level(logging.DEBUG, logger="dcc_mcp_core._version_check"):
        _version_check.log_report(report)

    assert "version drift" not in caplog.text
    assert "dcc-mcp-core 0.20.39" in caplog.text
    assert [r.levelno for r in caplog.records] == [logging.INFO]


def test_undecidable_report_stays_below_warning(isolated, caplog: pytest.LogCaptureFixture):
    """A source checkout must not spray warnings into the operator's log."""
    isolated(runtime="0.20.39", installed=None, metadata_module=object())
    report = _version_check.build_report()

    with caplog.at_level(logging.DEBUG, logger="dcc_mcp_core._version_check"):
        _version_check.log_report(report)

    assert [r.levelno for r in caplog.records] == [logging.DEBUG]


# ── never raises / opt-out ──────────────────────────────────────────────────


def test_self_check_survives_a_booming_resolver(monkeypatch: pytest.MonkeyPatch):
    def _boom(*_args: Any, **_kwargs: Any) -> Any:
        raise RuntimeError("no dist-info for you")

    monkeypatch.setattr(_version_check, "native_version", _boom)
    assert _version_check.run_version_self_check() == {}


def test_resolve_startup_version_falls_back_on_failure(monkeypatch: pytest.MonkeyPatch):
    def _boom(*_args: Any, **_kwargs: Any) -> Any:
        raise RuntimeError("boom")

    monkeypatch.setattr(_version_check, "native_version", _boom)
    assert _version_check.resolve_startup_version("0.0.0-dev") == "0.0.0-dev"


def test_resolve_startup_version_prefers_the_executing_artifact(isolated):
    isolated(runtime="0.19.2", installed="0.19.3", installed_path=FAKE_DIST)
    assert _version_check.resolve_startup_version("0.0.0-dev") == "0.19.2"


def test_resolve_startup_version_uses_metadata_without_the_extension(isolated):
    isolated(runtime=None, installed="0.20.39", installed_path=FAKE_DIST)
    assert _version_check.resolve_startup_version("0.0.0-dev") == "0.20.39"


def test_resolve_startup_version_never_returns_empty(isolated):
    isolated(runtime=None, installed=None, metadata_module=None)
    assert _version_check.resolve_startup_version("0.0.0-dev") == "0.0.0-dev"


def test_check_is_enabled_by_default(monkeypatch: pytest.MonkeyPatch):
    monkeypatch.delenv(ENV_VERSION_CHECK, raising=False)
    assert _version_check.version_check_enabled() is True


def test_check_can_be_disabled(monkeypatch: pytest.MonkeyPatch):
    monkeypatch.setenv(ENV_VERSION_CHECK, "0")
    assert _version_check.version_check_enabled() is False


# ── Python 3.7 stdlib fallback scan ─────────────────────────────────────────


def test_scan_finds_a_dist_info_directory(tmp_path, monkeypatch: pytest.MonkeyPatch):
    dist = tmp_path / "dcc_mcp_core-0.20.39.dist-info"
    dist.mkdir()
    (dist / "METADATA").write_text("Metadata-Version: 2.1\nName: dcc-mcp-core\nVersion: 0.20.39\n", encoding="utf-8")

    monkeypatch.setattr(sys, "path", [str(tmp_path)])
    monkeypatch.setattr(_version_check, "_metadata_module", lambda: None)

    assert _version_check.distribution_version() == "0.20.39"
    assert _version_check.distribution_location() == str(dist)


def test_scan_finds_an_egg_info_directory(tmp_path, monkeypatch: pytest.MonkeyPatch):
    dist = tmp_path / "dcc_mcp_core-0.20.39.egg-info"
    dist.mkdir()
    (dist / "PKG-INFO").write_text("Metadata-Version: 2.1\nName: dcc-mcp-core\nVersion: 0.20.39\n", encoding="utf-8")

    monkeypatch.setattr(sys, "path", [str(tmp_path)])
    monkeypatch.setattr(_version_check, "_metadata_module", lambda: None)

    assert _version_check.distribution_version() == "0.20.39"


def test_scan_finds_the_unversioned_editable_egg_info(tmp_path, monkeypatch: pytest.MonkeyPatch):
    """Setuptools writes ``<package>.egg-info`` with no version for editable installs."""
    dist = tmp_path / "dcc_mcp_core.egg-info"
    dist.mkdir()
    (dist / "PKG-INFO").write_text("Metadata-Version: 2.1\nName: dcc-mcp-core\nVersion: 0.20.39\n", encoding="utf-8")

    monkeypatch.setattr(sys, "path", [str(tmp_path)])
    monkeypatch.setattr(_version_check, "_metadata_module", lambda: None)

    assert _version_check.distribution_version() == "0.20.39"


def test_scan_finds_a_single_file_egg_info(tmp_path, monkeypatch: pytest.MonkeyPatch):
    egg_info = tmp_path / "dcc_mcp_core-0.20.39.egg-info"
    egg_info.write_text("Metadata-Version: 2.1\nName: dcc-mcp-core\nVersion: 0.20.39\n", encoding="utf-8")

    monkeypatch.setattr(sys, "path", [str(tmp_path)])
    monkeypatch.setattr(_version_check, "_metadata_module", lambda: None)

    assert _version_check.distribution_version() == "0.20.39"


def test_scan_ranks_newest_first_within_one_directory(tmp_path, monkeypatch: pytest.MonkeyPatch):
    """``0.20.30`` must win over ``0.20.2``; lexicographic order would not."""
    for version in ("0.20.2", "0.20.30"):
        dist = tmp_path / f"dcc_mcp_core-{version}.dist-info"
        dist.mkdir()
        (dist / "METADATA").write_text(
            f"Metadata-Version: 2.1\nName: dcc-mcp-core\nVersion: {version}\n", encoding="utf-8"
        )

    monkeypatch.setattr(sys, "path", [str(tmp_path)])
    monkeypatch.setattr(_version_check, "_metadata_module", lambda: None)

    assert _version_check.distribution_version() == "0.20.30"


def test_scan_skips_a_directory_without_a_version(tmp_path, monkeypatch: pytest.MonkeyPatch):
    """A malformed ``.dist-info`` must not contribute a path on its own."""
    dist = tmp_path / "dcc_mcp_core-0.20.39.dist-info"
    dist.mkdir()
    (dist / "METADATA").write_text("Metadata-Version: 2.1\nName: dcc-mcp-core\n", encoding="utf-8")

    monkeypatch.setattr(sys, "path", [str(tmp_path)])
    monkeypatch.setattr(_version_check, "_metadata_module", lambda: None)

    assert _version_check.distribution_version() is None
    assert _version_check.distribution_location() is None


def test_scan_ignores_other_packages(tmp_path, monkeypatch: pytest.MonkeyPatch):
    dist = tmp_path / "some_other_package-9.9.9.dist-info"
    dist.mkdir()
    (dist / "METADATA").write_text(
        "Metadata-Version: 2.1\nName: some-other-package\nVersion: 9.9.9\n", encoding="utf-8"
    )

    monkeypatch.setattr(sys, "path", [str(tmp_path)])
    monkeypatch.setattr(_version_check, "_metadata_module", lambda: None)

    assert _version_check.distribution_version() is None


def test_scan_tolerates_missing_sys_path_entries(monkeypatch: pytest.MonkeyPatch):
    monkeypatch.setattr(sys, "path", ["", "/definitely/not/a/real/path"])
    monkeypatch.setattr(_version_check, "_metadata_module", lambda: None)

    assert _version_check.distribution_version() is None


def test_version_and_path_come_from_the_same_artifact(tmp_path, monkeypatch: pytest.MonkeyPatch):
    """A report must never quote one version and another distribution's directory."""
    for version in ("0.20.2", "0.20.30"):
        dist = tmp_path / f"dcc_mcp_core-{version}.dist-info"
        dist.mkdir()
        (dist / "METADATA").write_text(
            f"Metadata-Version: 2.1\nName: dcc-mcp-core\nVersion: {version}\n", encoding="utf-8"
        )

    monkeypatch.setattr(sys, "path", [str(tmp_path)])
    monkeypatch.setattr(_version_check, "_metadata_module", lambda: None)

    assert _version_check.distribution_location().endswith("dcc_mcp_core-0.20.30.dist-info")


# ── native extension probing ────────────────────────────────────────────────


def test_native_version_reads_sys_modules_without_importing(monkeypatch: pytest.MonkeyPatch):
    monkeypatch.setitem(sys.modules, _version_check.NATIVE_MODULE, None)
    assert _version_check.native_version() is None


def test_native_version_ignores_an_empty_version(monkeypatch: pytest.MonkeyPatch):
    class _Core:
        __version__ = ""

    monkeypatch.setitem(sys.modules, _version_check.NATIVE_MODULE, _Core())
    assert _version_check.native_version() is None


def test_native_version_reads_a_loaded_extension(monkeypatch: pytest.MonkeyPatch):
    class _Core:
        __version__ = "0.20.39"
        __file__ = FAKE_EXTENSION

    monkeypatch.setitem(sys.modules, _version_check.NATIVE_MODULE, _Core())

    assert _version_check.native_version() == "0.20.39"
    assert _version_check.native_origin() == FAKE_EXTENSION


def test_native_origin_is_none_without_the_extension(monkeypatch: pytest.MonkeyPatch):
    monkeypatch.delitem(sys.modules, _version_check.NATIVE_MODULE, raising=False)
    assert _version_check.native_origin() is None


# ── metadata-API path resolution ────────────────────────────────────────────


def test_distribution_path_prefers_the_dist_info_directory():
    class _Distribution:
        _path = FAKE_DIST

        def locate_file(self, _name: str) -> str:
            raise AssertionError("locate_file must not be used when _path is present")

    class _Metadata:
        @staticmethod
        def distribution(_package: str) -> Any:
            return _Distribution()

    assert _version_check._distribution_path_from_api(_Metadata(), "dcc-mcp-core") == FAKE_DIST


def test_distribution_path_falls_back_to_locate_file():
    class _Distribution:
        _path = None

        def locate_file(self, _name: str) -> str:
            return "/opt/site-packages"

    class _Metadata:
        @staticmethod
        def distribution(_package: str) -> Any:
            return _Distribution()

    assert _version_check._distribution_path_from_api(_Metadata(), "dcc-mcp-core") == "/opt/site-packages"


def test_distribution_path_is_none_when_the_package_is_absent():
    class _Metadata:
        @staticmethod
        def distribution(_package: str) -> Any:
            raise RuntimeError("PackageNotFoundError")

    assert _version_check._distribution_path_from_api(_Metadata(), "dcc-mcp-core") is None


def test_metadata_api_falls_through_to_the_scan_on_an_empty_version(tmp_path, monkeypatch: pytest.MonkeyPatch):
    r"""A truthy check keeps the literal ``"None"`` out of the report."""
    dist = tmp_path / "dcc_mcp_core-0.20.39.dist-info"
    dist.mkdir()
    (dist / "METADATA").write_text("Metadata-Version: 2.1\nName: dcc-mcp-core\nVersion: 0.20.39\n", encoding="utf-8")

    class _Metadata:
        @staticmethod
        def version(_package: str) -> str:
            return ""

    monkeypatch.setattr(sys, "path", [str(tmp_path)])
    monkeypatch.setattr(_version_check, "_metadata_module", lambda: _Metadata())

    assert _version_check.distribution_version() == "0.20.39"


# ── version normalisation ───────────────────────────────────────────────────


@pytest.mark.parametrize(
    ("raw", "expected"),
    [
        ("0.19.2", "0.19.2"),
        ("  0.19.02  ", "0.19.2"),
        (None, ""),
        ("", ""),
    ],
)
def test_normalize_version(raw, expected):
    assert _version_check._normalize_version(raw) == expected


def test_normalize_version_lower_cases_non_pep440_values():
    assert _version_check._normalize_version("UNKNOWN") == "unknown"


def test_metadata_field_matches_case_insensitively():
    text = "Metadata-Version: 2.1\nname: dcc-mcp-core\nVERSION: 0.20.39\n"
    assert _version_check._metadata_field(text, "version") == "0.20.39"


def test_metadata_field_returns_none_for_a_missing_field():
    assert _version_check._metadata_field("Name: dcc-mcp-core\n", "Version") is None


def test_metadata_field_skips_an_empty_value():
    assert _version_check._metadata_field("Version:\nVersion: 0.20.39\n", "Version") == "0.20.39"


def test_version_from_distribution_name():
    assert _version_check._version_from_distribution_name("/x/dcc_mcp_core-0.20.39.dist-info") == "0.20.39"
    assert _version_check._version_from_distribution_name("/x/dcc_mcp_core.egg-info") == ""
    assert _version_check._version_from_distribution_name("/x/other-1.2.3.dist-info") == "1.2.3"
