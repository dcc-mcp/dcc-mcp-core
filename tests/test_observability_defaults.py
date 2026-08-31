"""Tests for the default observability features in DccServerBase.

Verifies that, out-of-the-box, every DccServerBase instance:

1. Starts rolling file logging (``init_file_logging``).
2. Wires a SQLite job-persistence path into ``McpHttpConfig``.
3. Initialises in-process telemetry metrics before ``start()``.
4. Respects ``DCC_MCP_DISABLE_*`` env vars and explicit ``enable_*=False``
   options to let adapters opt out.
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


def _options(dcc_name: str, skills: Path, **kwargs: object):
    from dcc_mcp_core._server.options import DccServerOptions

    return DccServerOptions.from_env(dcc_name, skills, **kwargs)


# ── File logging ──────────────────────────────────────────────────────────────


class TestFileLoggingDefaults:
    def test_file_logging_enabled_by_default(self, tmp_path: Path) -> None:
        """init_file_logging must be called on construction when not disabled."""
        with patch("dcc_mcp_core.server_base.create_adapter_server", return_value=MagicMock()):
            with patch("dcc_mcp_core.server_base.DccServerBase._init_file_logging") as mock_log:
                mock_log.return_value = str(tmp_path)
                _Stub, skills = _make_stub_class(tmp_path)
                _Stub(_options("maya", skills, port=0))
                mock_log.assert_called_once_with("maya")

    def test_file_logging_disabled_via_flag(self, tmp_path: Path) -> None:
        """enable_file_logging=False must be stored correctly."""
        with patch("dcc_mcp_core.server_base.create_adapter_server", return_value=MagicMock()):
            with patch("dcc_mcp_core.server_base.DccServerBase._init_file_logging") as mock_log:
                mock_log.return_value = ""
                _Stub, skills = _make_stub_class(tmp_path)
                srv = _Stub(
                    _options(
                        "maya",
                        skills,
                        port=0,
                        enable_file_logging=False,
                    )
                )
                assert srv._enable_file_logging is False

    def test_file_logging_disabled_via_env_var(self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
        """DCC_MCP_DISABLE_FILE_LOGGING=1 must override enable_file_logging=True."""
        monkeypatch.setenv("DCC_MCP_DISABLE_FILE_LOGGING", "1")
        with patch("dcc_mcp_core.server_base.create_adapter_server", return_value=MagicMock()):
            _Stub, skills = _make_stub_class(tmp_path)
            srv = _Stub(_options("maya", skills, port=0))
            assert srv._enable_file_logging is False

    def test_log_dir_property_reflects_resolved_dir(self, tmp_path: Path) -> None:
        """server.log_dir must return the path passed back by init_file_logging."""
        log_dir = str(tmp_path / "logs")
        Path(log_dir).mkdir()
        with patch("dcc_mcp_core.server_base.create_adapter_server", return_value=MagicMock()):
            with patch("dcc_mcp_core.server_base.DccServerBase._init_file_logging", return_value=log_dir):
                _Stub, skills = _make_stub_class(tmp_path)
                srv = _Stub(_options("maya", skills, port=0))
                assert srv.log_dir == log_dir

    def test_init_file_logging_helper_uses_dcc_prefix(self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
        """_init_file_logging must set file_name_prefix=dcc-mcp-<dcc_name>."""
        captured: list = []

        def _fake_init(cfg):
            captured.append(cfg)
            return str(tmp_path)

        from types import SimpleNamespace

        import dcc_mcp_core

        monkeypatch.setitem(dcc_mcp_core.__dict__, "init_file_logging", _fake_init)
        monkeypatch.setitem(dcc_mcp_core.__dict__, "get_log_dir", lambda: str(tmp_path))
        monkeypatch.setitem(
            dcc_mcp_core.__dict__,
            "FileLoggingConfig",
            lambda **kwargs: SimpleNamespace(**kwargs),
        )

        with patch("dcc_mcp_core.server_base.create_adapter_server", return_value=MagicMock()):
            _Stub, skills = _make_stub_class(tmp_path)
            _Stub(_options("houdini", skills, port=0))

        if captured:
            # Prefix includes PID suffix for multi-instance isolation (issue #402).
            assert captured[0].file_name_prefix.startswith("dcc-mcp-houdini")


# ── Job persistence ───────────────────────────────────────────────────────────


class TestJobPersistenceDefaults:
    def test_default_storage_path_isolated_by_instance_key(self, tmp_path: Path) -> None:
        """Concurrent same-DCC processes must not contend on one default DB."""
        from dcc_mcp_core._server.observability import JobPersistenceManager

        first = JobPersistenceManager("blender", log_dir=str(tmp_path), instance_key="gui-a")
        second = JobPersistenceManager("blender", log_dir=str(tmp_path), instance_key="gui-b")
        first_path = first._resolve_db_path()
        second_path = second._resolve_db_path()
        assert first_path and second_path and first_path != second_path
        assert first_path.endswith("dcc-mcp-blender-gui-a-jobs.db")
        assert second_path.endswith("dcc-mcp-blender-gui-b-jobs.db")

    def test_environment_instance_key_is_honored_for_restart_recovery(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """A stable operator key must survive process restarts for recovery."""
        from dcc_mcp_core._server.observability import JobPersistenceManager

        monkeypatch.setenv("DCC_MCP_JOB_INSTANCE_KEY", "stable-gui")
        manager = JobPersistenceManager("maya", log_dir=str(tmp_path))
        assert manager._resolve_db_path().endswith("dcc-mcp-maya-stable-gui-jobs.db")

    def test_explicit_storage_path_remains_shared_contract(self, tmp_path: Path) -> None:
        """An explicit path is never rewritten with the per-instance suffix."""
        from dcc_mcp_core._server.observability import JobPersistenceManager

        manager = JobPersistenceManager("maya", log_dir=str(tmp_path), instance_key="gui-a")
        config = MagicMock(job_storage_path=str(tmp_path / "shared.sqlite3"))
        config.job_storage_path = str(tmp_path / "shared.sqlite3")
        with patch("dcc_mcp_core._server.observability.importlib.import_module") as import_module:
            import_module.return_value.job_persistence_feature_enabled.return_value = False
            manager.init(config)
        assert config.job_storage_path == str(tmp_path / "shared.sqlite3")

    def test_storage_probe_rolls_back_without_recovering_pending_rows(self, tmp_path: Path) -> None:
        """The startup probe must test writes without mutating persisted jobs."""
        import sqlite3

        from dcc_mcp_core._server.observability import JobPersistenceManager

        db_path = tmp_path / "jobs.sqlite3"
        with sqlite3.connect(db_path) as connection:
            connection.execute("CREATE TABLE jobs (job_id TEXT PRIMARY KEY, status TEXT)")
            connection.execute("INSERT INTO jobs(job_id, status) VALUES ('pending-1', 'pending')")
            connection.commit()

        JobPersistenceManager._probe_storage(str(db_path))

        with sqlite3.connect(db_path) as connection:
            assert connection.execute("SELECT status FROM jobs WHERE job_id = 'pending-1'").fetchone() == ("pending",)

    def test_storage_probe_classifies_readonly_and_wal_failures(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """Probe failures expose stable categories without raw SQLite text."""
        from types import SimpleNamespace

        from dcc_mcp_core._server.observability import JobPersistenceManager

        manager = JobPersistenceManager("maya", log_dir=str(tmp_path))
        config = MagicMock()
        monkeypatch.setattr(
            "dcc_mcp_core._server.observability.importlib.import_module",
            lambda _name: SimpleNamespace(job_persistence_feature_enabled=lambda: True),
        )
        monkeypatch.setattr(
            JobPersistenceManager,
            "_probe_storage",
            staticmethod(lambda _path: (_ for _ in ()).throw(OSError("attempt to write a readonly database"))),
        )
        manager.init(config)
        assert manager.state == "unavailable"
        assert manager.last_error_kind == "readonly"

        monkeypatch.setattr(
            JobPersistenceManager,
            "_probe_storage",
            staticmethod(lambda _path: (_ for _ in ()).throw(OSError("stale WAL/SHM sidecar"))),
        )
        manager.init(config)
        assert manager.last_error_kind == "wal"

    def test_storage_probe_detects_rust_sidecar_owner(self, tmp_path: Path) -> None:
        """The Python probe must fail closed when the Rust ownership lease is held."""
        from dcc_mcp_core._server.observability import JobPersistenceManager
        from dcc_mcp_core._server.observability import _storage_ownership_lock

        db_path = tmp_path / "jobs.sqlite3"
        import sqlite3

        with sqlite3.connect(db_path):
            pass
        with _storage_ownership_lock(str(db_path)):
            with pytest.raises(OSError, match="ownership unavailable"):
                JobPersistenceManager._probe_storage(str(db_path))

    def test_storage_probe_rejects_symlink_database_path(self, tmp_path: Path) -> None:
        """Ownership probing must not follow aliases to another database."""
        from dcc_mcp_core._server.observability import JobPersistenceManager

        target = tmp_path / "real.sqlite3"
        target.touch()
        alias = tmp_path / "alias.sqlite3"
        try:
            alias.symlink_to(target)
        except (OSError, NotImplementedError):
            pytest.skip("symlink creation is unavailable on this host")
        with pytest.raises(OSError, match="symlink or reparse"):
            JobPersistenceManager._probe_storage(str(alias))

    def test_featureless_native_build_disables_persistence(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """A wheel without the optional Rust feature must stay explicitly disabled."""
        import sys
        from types import SimpleNamespace

        import dcc_mcp_core
        from dcc_mcp_core._server.observability import JobPersistenceManager

        fake_core = SimpleNamespace(job_persistence_feature_enabled=lambda: False)
        monkeypatch.setitem(
            sys.modules,
            "dcc_mcp_core._core",
            fake_core,
        )
        monkeypatch.setattr(dcc_mcp_core, "_core", fake_core, raising=False)
        manager = JobPersistenceManager("maya", log_dir=str(tmp_path))
        config = MagicMock()
        manager.init(config)
        assert manager.state == "disabled"
        assert manager.last_error_kind == "feature_disabled"
        config.job_storage_path = None

    def test_explicit_path_on_featureless_build_is_unavailable(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """An explicit persistence request must not silently become in-memory."""
        import sys
        from types import SimpleNamespace

        from dcc_mcp_core._server.observability import JobPersistenceManager

        fake_core = SimpleNamespace(job_persistence_feature_enabled=lambda: False)
        monkeypatch.setitem(sys.modules, "dcc_mcp_core._core", fake_core)
        manager = JobPersistenceManager("maya", log_dir=str(tmp_path))
        config = MagicMock(job_storage_path=str(tmp_path / "jobs.sqlite3"))
        manager.init(config)
        assert manager.state == "unavailable"
        assert manager.last_error_kind == "feature_disabled"
        assert config.job_storage_path.endswith("jobs.sqlite3")

    def test_missing_native_build_without_explicit_path_is_disabled(self, monkeypatch: pytest.MonkeyPatch) -> None:
        """Lite wheels without a log-dir helper may disable persistence safely."""
        from dcc_mcp_core._server.observability import JobPersistenceManager

        monkeypatch.setattr(
            "dcc_mcp_core._server.observability.importlib.import_module",
            lambda _name: (_ for _ in ()).throw(ImportError("native extension unavailable")),
        )
        manager = JobPersistenceManager("maya")
        config = MagicMock()
        manager.init(config)
        assert manager.state == "disabled"
        assert manager.last_error_kind == "feature_disabled"

    def test_missing_native_extension_disables_persistence(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """The py37-lite fallback must not claim native persistence is healthy."""
        from dcc_mcp_core._server.observability import JobPersistenceManager

        monkeypatch.setattr(
            "dcc_mcp_core._server.observability.importlib.import_module",
            lambda _name: (_ for _ in ()).throw(ImportError("native extension unavailable")),
        )
        manager = JobPersistenceManager("maya", log_dir=str(tmp_path))
        config = MagicMock()
        manager.init(config)
        assert manager.state == "disabled"
        assert manager.last_error_kind == "feature_disabled"

    def test_native_extension_without_capability_method_disables_persistence(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        from types import SimpleNamespace

        from dcc_mcp_core._server.observability import JobPersistenceManager

        monkeypatch.setattr(
            "dcc_mcp_core._server.observability.importlib.import_module",
            lambda _name: SimpleNamespace(),
        )
        manager = JobPersistenceManager("maya", log_dir=str(tmp_path))
        config = MagicMock()
        manager.init(config)
        assert manager.state == "disabled"
        assert manager.last_error_kind == "feature_disabled"

    def test_path_resolution_failure_fails_closed_at_server_start(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        from types import SimpleNamespace

        from dcc_mcp_core._server.observability import JobPersistenceManager

        monkeypatch.setattr(
            "dcc_mcp_core._server.observability.importlib.import_module",
            lambda _name: SimpleNamespace(job_persistence_feature_enabled=lambda: True),
        )

        monkeypatch.setattr(JobPersistenceManager, "_resolve_db_path", lambda _self: None)
        manager = JobPersistenceManager("maya", log_dir=str(tmp_path))
        config = MagicMock()
        manager.init(config)
        assert manager.state == "unavailable"
        assert manager.last_error_kind == "path"

    def test_server_refuses_memory_fallback_after_persistence_path_failure(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        import dcc_mcp_core._server.observability as observability
        import dcc_mcp_core.server_base as server_base

        def fail_closed(manager: object, _config: object) -> None:
            manager._state = "unavailable"  # type: ignore[attr-defined]
            manager._last_error_kind = "path"  # type: ignore[attr-defined]

        monkeypatch.setattr(observability.JobPersistenceManager, "init", fail_closed)
        with patch.object(server_base, "create_adapter_server", return_value=MagicMock()):
            with patch.object(server_base.DccServerBase, "_init_file_logging", return_value=str(tmp_path)):
                _Stub, skills = _make_stub_class(tmp_path)
                with pytest.raises(RuntimeError, match="refusing to start with in-memory history"):
                    _Stub(_options("maya", skills, port=0))

    def test_job_storage_path_set_by_default(self, tmp_path: Path) -> None:
        """job_storage_path on McpHttpConfig must be set when persistence is enabled."""
        with patch("dcc_mcp_core.server_base.create_adapter_server", return_value=MagicMock()):
            with patch("dcc_mcp_core.server_base.DccServerBase._init_file_logging", return_value=str(tmp_path)):
                _Stub, skills = _make_stub_class(tmp_path)
                srv = _Stub(_options("maya", skills, port=0))
                if srv._enable_job_persistence:
                    db_path = getattr(srv._config, "job_storage_path", None)
                    assert db_path is None or "maya" in db_path

    def test_job_persistence_disabled_via_flag(self, tmp_path: Path) -> None:
        """enable_job_persistence=False must be stored correctly."""
        with patch("dcc_mcp_core.server_base.create_adapter_server", return_value=MagicMock()):
            with patch("dcc_mcp_core.server_base.DccServerBase._init_file_logging", return_value=""):
                _Stub, skills = _make_stub_class(tmp_path)
                srv = _Stub(
                    _options(
                        "maya",
                        skills,
                        port=0,
                        enable_job_persistence=False,
                    )
                )
                assert srv._enable_job_persistence is False

    def test_job_persistence_disabled_via_env_var(self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
        """DCC_MCP_DISABLE_JOB_PERSISTENCE=1 must override the default."""
        monkeypatch.setenv("DCC_MCP_DISABLE_JOB_PERSISTENCE", "1")
        with patch("dcc_mcp_core.server_base.create_adapter_server", return_value=MagicMock()):
            with patch("dcc_mcp_core.server_base.DccServerBase._init_file_logging", return_value=""):
                _Stub, skills = _make_stub_class(tmp_path)
                srv = _Stub(_options("maya", skills, port=0))
                assert srv._enable_job_persistence is False


# ── Telemetry ─────────────────────────────────────────────────────────────────


class TestTelemetryDefaults:
    def test_telemetry_init_called_on_start(self, tmp_path: Path) -> None:
        """_init_telemetry must be called inside start()."""
        mock_handle = MagicMock()
        mock_handle.mcp_url.return_value = "http://127.0.0.1:0/mcp"
        mock_server = MagicMock()
        mock_server.start.return_value = mock_handle

        with patch("dcc_mcp_core.server_base.create_adapter_server", return_value=mock_server):
            with patch("dcc_mcp_core.server_base.DccServerBase._init_file_logging", return_value=""):
                with patch("dcc_mcp_core.server_base.DccServerBase._init_telemetry") as mock_tel:
                    _Stub, skills = _make_stub_class(tmp_path)
                    srv = _Stub(_options("maya", skills, port=0))
                    mock_tel.assert_not_called()
                    srv.start()
                    mock_tel.assert_called_once()

    def test_telemetry_disabled_via_flag(self, tmp_path: Path) -> None:
        """enable_telemetry=False must be stored correctly."""
        with patch("dcc_mcp_core.server_base.create_adapter_server", return_value=MagicMock()):
            with patch("dcc_mcp_core.server_base.DccServerBase._init_file_logging", return_value=""):
                _Stub, skills = _make_stub_class(tmp_path)
                srv = _Stub(
                    _options(
                        "maya",
                        skills,
                        port=0,
                        enable_telemetry=False,
                    )
                )
                assert srv._enable_telemetry is False

    def test_telemetry_disabled_via_env_var(self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
        """DCC_MCP_DISABLE_TELEMETRY=1 must override enable_telemetry=True."""
        monkeypatch.setenv("DCC_MCP_DISABLE_TELEMETRY", "1")
        with patch("dcc_mcp_core.server_base.create_adapter_server", return_value=MagicMock()):
            with patch("dcc_mcp_core.server_base.DccServerBase._init_file_logging", return_value=""):
                _Stub, skills = _make_stub_class(tmp_path)
                srv = _Stub(_options("maya", skills, port=0))
                assert srv._enable_telemetry is False

    def test_init_telemetry_skips_when_already_initialized(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        """_init_telemetry must not call TelemetryConfig.init() twice."""
        with patch("dcc_mcp_core.server_base.create_adapter_server", return_value=MagicMock()):
            with patch("dcc_mcp_core.server_base.DccServerBase._init_file_logging", return_value=""):
                _Stub, skills = _make_stub_class(tmp_path)
                srv = _Stub(_options("maya", skills, port=0))

        import dcc_mcp_core

        mock_tc = MagicMock()
        monkeypatch.setitem(dcc_mcp_core.__dict__, "is_telemetry_initialized", lambda: True)
        monkeypatch.setitem(dcc_mcp_core.__dict__, "TelemetryConfig", mock_tc)
        srv._init_telemetry()
        mock_tc.assert_not_called()


# ── observability_summary property ───────────────────────────────────────────


class TestObservabilitySummary:
    def test_summary_keys_present(self, tmp_path: Path) -> None:
        """observability_summary must expose all three feature flags."""
        with patch("dcc_mcp_core.server_base.create_adapter_server", return_value=MagicMock()):
            with patch("dcc_mcp_core.server_base.DccServerBase._init_file_logging", return_value=""):
                _Stub, skills = _make_stub_class(tmp_path)
                srv = _Stub(_options("maya", skills, port=0))
                summary = srv.observability_summary
                assert "file_logging" in summary
                assert "log_dir" in summary
                assert "job_persistence" in summary
                assert "job_db" in summary
                assert summary["job_persistence_state"] in {
                    "not_configured",
                    "disabled",
                    "healthy",
                    "unavailable",
                }
                assert "job_persistence_error_kind" in summary
                assert "telemetry" in summary
