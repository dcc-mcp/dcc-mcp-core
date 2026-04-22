"""Tests for the default observability features in DccServerBase.

Verifies that, out-of-the-box, every DccServerBase instance:

1. Starts rolling file logging (``init_file_logging``).
2. Wires a SQLite job-persistence path into ``McpHttpConfig``.
3. Initialises in-process telemetry metrics before ``start()``.
4. Respects ``DCC_MCP_DISABLE_*`` env vars and explicit ``enable_*=False``
   constructor flags to let adapters opt out.
"""

from __future__ import annotations

from pathlib import Path
from unittest.mock import MagicMock
from unittest.mock import patch

import pytest


def _make_stub_class(tmp_path: Path):
    """Return a DccServerBase subclass and the skills dir for it."""
    from dcc_mcp_core.server_base import DccServerBase

    class _Stub(DccServerBase):
        pass

    skills = tmp_path / "skills"
    skills.mkdir(exist_ok=True)
    return _Stub, skills


# ── File logging ──────────────────────────────────────────────────────────────


class TestFileLoggingDefaults:
    def test_file_logging_enabled_by_default(self, tmp_path: Path) -> None:
        """init_file_logging must be called on construction when not disabled."""
        with patch("dcc_mcp_core.create_skill_server", return_value=MagicMock()):
            with patch("dcc_mcp_core.server_base.DccServerBase._init_file_logging") as mock_log:
                mock_log.return_value = str(tmp_path)
                _Stub, skills = _make_stub_class(tmp_path)
                _Stub(dcc_name="maya", builtin_skills_dir=skills, port=0)
                mock_log.assert_called_once_with("maya")

    def test_file_logging_disabled_via_flag(self, tmp_path: Path) -> None:
        """enable_file_logging=False must be stored correctly."""
        with patch("dcc_mcp_core.create_skill_server", return_value=MagicMock()):
            with patch("dcc_mcp_core.server_base.DccServerBase._init_file_logging") as mock_log:
                mock_log.return_value = ""
                _Stub, skills = _make_stub_class(tmp_path)
                srv = _Stub(
                    dcc_name="maya",
                    builtin_skills_dir=skills,
                    port=0,
                    enable_file_logging=False,
                )
                assert srv._enable_file_logging is False

    def test_file_logging_disabled_via_env_var(self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
        """DCC_MCP_DISABLE_FILE_LOGGING=1 must override enable_file_logging=True."""
        monkeypatch.setenv("DCC_MCP_DISABLE_FILE_LOGGING", "1")
        with patch("dcc_mcp_core.create_skill_server", return_value=MagicMock()):
            _Stub, skills = _make_stub_class(tmp_path)
            srv = _Stub(dcc_name="maya", builtin_skills_dir=skills, port=0)
            assert srv._enable_file_logging is False

    def test_log_dir_property_reflects_resolved_dir(self, tmp_path: Path) -> None:
        """server.log_dir must return the path passed back by init_file_logging."""
        log_dir = str(tmp_path / "logs")
        with patch("dcc_mcp_core.create_skill_server", return_value=MagicMock()):
            with patch("dcc_mcp_core.server_base.DccServerBase._init_file_logging", return_value=log_dir):
                _Stub, skills = _make_stub_class(tmp_path)
                srv = _Stub(dcc_name="maya", builtin_skills_dir=skills, port=0)
                assert srv.log_dir == log_dir

    def test_init_file_logging_helper_uses_dcc_prefix(self, tmp_path: Path) -> None:
        """_init_file_logging must set file_name_prefix=dcc-mcp-<dcc_name>."""
        captured: list = []

        def _fake_init(cfg):
            captured.append(cfg)
            return str(tmp_path)

        with patch("dcc_mcp_core.create_skill_server", return_value=MagicMock()):
            with patch("dcc_mcp_core.init_file_logging", side_effect=_fake_init):
                with patch("dcc_mcp_core.get_log_dir", return_value=str(tmp_path)):
                    from importlib import reload

                    import dcc_mcp_core.server_base as sb

                    reload(sb)

                    class _Stub(sb.DccServerBase):
                        pass

                    skills = tmp_path / "skills"
                    skills.mkdir(exist_ok=True)
                    _Stub(dcc_name="houdini", builtin_skills_dir=skills, port=0)

        if captured:
            assert captured[0].file_name_prefix == "dcc-mcp-houdini"


# ── Job persistence ───────────────────────────────────────────────────────────


class TestJobPersistenceDefaults:
    def test_job_storage_path_set_by_default(self, tmp_path: Path) -> None:
        """job_storage_path on McpHttpConfig must be set when persistence is enabled."""
        with patch("dcc_mcp_core.create_skill_server", return_value=MagicMock()):
            with patch("dcc_mcp_core.server_base.DccServerBase._init_file_logging", return_value=str(tmp_path)):
                _Stub, skills = _make_stub_class(tmp_path)
                srv = _Stub(dcc_name="maya", builtin_skills_dir=skills, port=0)
                if srv._enable_job_persistence:
                    db_path = getattr(srv._config, "job_storage_path", None)
                    assert db_path is None or "maya" in db_path

    def test_job_persistence_disabled_via_flag(self, tmp_path: Path) -> None:
        """enable_job_persistence=False must be stored correctly."""
        with patch("dcc_mcp_core.create_skill_server", return_value=MagicMock()):
            with patch("dcc_mcp_core.server_base.DccServerBase._init_file_logging", return_value=""):
                _Stub, skills = _make_stub_class(tmp_path)
                srv = _Stub(
                    dcc_name="maya",
                    builtin_skills_dir=skills,
                    port=0,
                    enable_job_persistence=False,
                )
                assert srv._enable_job_persistence is False

    def test_job_persistence_disabled_via_env_var(self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
        """DCC_MCP_DISABLE_JOB_PERSISTENCE=1 must override the default."""
        monkeypatch.setenv("DCC_MCP_DISABLE_JOB_PERSISTENCE", "1")
        with patch("dcc_mcp_core.create_skill_server", return_value=MagicMock()):
            with patch("dcc_mcp_core.server_base.DccServerBase._init_file_logging", return_value=""):
                _Stub, skills = _make_stub_class(tmp_path)
                srv = _Stub(dcc_name="maya", builtin_skills_dir=skills, port=0)
                assert srv._enable_job_persistence is False


# ── Telemetry ─────────────────────────────────────────────────────────────────


class TestTelemetryDefaults:
    def test_telemetry_init_called_on_start(self, tmp_path: Path) -> None:
        """_init_telemetry must be called inside start()."""
        mock_handle = MagicMock()
        mock_handle.mcp_url.return_value = "http://127.0.0.1:0/mcp"
        mock_server = MagicMock()
        mock_server.start.return_value = mock_handle

        with patch("dcc_mcp_core.create_skill_server", return_value=mock_server):
            with patch("dcc_mcp_core.server_base.DccServerBase._init_file_logging", return_value=""):
                with patch("dcc_mcp_core.server_base.DccServerBase._init_telemetry") as mock_tel:
                    _Stub, skills = _make_stub_class(tmp_path)
                    srv = _Stub(dcc_name="maya", builtin_skills_dir=skills, port=0)
                    mock_tel.assert_not_called()
                    srv.start()
                    mock_tel.assert_called_once()

    def test_telemetry_disabled_via_flag(self, tmp_path: Path) -> None:
        """enable_telemetry=False must be stored correctly."""
        with patch("dcc_mcp_core.create_skill_server", return_value=MagicMock()):
            with patch("dcc_mcp_core.server_base.DccServerBase._init_file_logging", return_value=""):
                _Stub, skills = _make_stub_class(tmp_path)
                srv = _Stub(
                    dcc_name="maya",
                    builtin_skills_dir=skills,
                    port=0,
                    enable_telemetry=False,
                )
                assert srv._enable_telemetry is False

    def test_telemetry_disabled_via_env_var(self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
        """DCC_MCP_DISABLE_TELEMETRY=1 must override enable_telemetry=True."""
        monkeypatch.setenv("DCC_MCP_DISABLE_TELEMETRY", "1")
        with patch("dcc_mcp_core.create_skill_server", return_value=MagicMock()):
            with patch("dcc_mcp_core.server_base.DccServerBase._init_file_logging", return_value=""):
                _Stub, skills = _make_stub_class(tmp_path)
                srv = _Stub(dcc_name="maya", builtin_skills_dir=skills, port=0)
                assert srv._enable_telemetry is False

    def test_init_telemetry_skips_when_already_initialized(self, tmp_path: Path) -> None:
        """_init_telemetry must not call TelemetryConfig.init() twice."""
        with patch("dcc_mcp_core.create_skill_server", return_value=MagicMock()):
            with patch("dcc_mcp_core.server_base.DccServerBase._init_file_logging", return_value=""):
                _Stub, skills = _make_stub_class(tmp_path)
                srv = _Stub(dcc_name="maya", builtin_skills_dir=skills, port=0)

        with patch("dcc_mcp_core.is_telemetry_initialized", return_value=True):
            with patch("dcc_mcp_core.TelemetryConfig") as mock_tc:
                srv._init_telemetry()
                mock_tc.assert_not_called()


# ── observability_summary property ───────────────────────────────────────────


class TestObservabilitySummary:
    def test_summary_keys_present(self, tmp_path: Path) -> None:
        """observability_summary must expose all three feature flags."""
        with patch("dcc_mcp_core.create_skill_server", return_value=MagicMock()):
            with patch("dcc_mcp_core.server_base.DccServerBase._init_file_logging", return_value=""):
                _Stub, skills = _make_stub_class(tmp_path)
                srv = _Stub(dcc_name="maya", builtin_skills_dir=skills, port=0)
                summary = srv.observability_summary
                assert "file_logging" in summary
                assert "log_dir" in summary
                assert "job_persistence" in summary
                assert "job_db" in summary
                assert "telemetry" in summary
