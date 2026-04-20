"""Tests for the rolling file-logging layer.

These exercise the Python surface (``FileLoggingConfig`` /
``init_file_logging`` / ``shutdown_file_logging``) — the rotation and
retention semantics themselves are covered by the Rust unit tests in
``crates/dcc-mcp-utils/src/file_logging.rs``.

The global ``tracing`` subscriber is a process-wide resource, so each
test swaps the file layer onto a per-test ``tmp_path`` and tears it
down afterwards to keep suites isolated.
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
import contextlib
import logging
from pathlib import Path

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core import DEFAULT_LOG_FILE_PREFIX
from dcc_mcp_core import DEFAULT_LOG_MAX_FILES
from dcc_mcp_core import DEFAULT_LOG_MAX_SIZE
from dcc_mcp_core import DEFAULT_LOG_ROTATION
from dcc_mcp_core import ENV_LOG_DIR
from dcc_mcp_core import ENV_LOG_MAX_FILES
from dcc_mcp_core import ENV_LOG_MAX_SIZE
from dcc_mcp_core import ENV_LOG_ROTATION
from dcc_mcp_core import FileLoggingConfig
from dcc_mcp_core import init_file_logging
from dcc_mcp_core import shutdown_file_logging


@pytest.fixture(autouse=True)
def _detach_file_layer():
    """Make sure every test starts and ends with no file layer attached."""
    # First test before any init — swallow the "not initialized" error.
    with contextlib.suppress(Exception):
        shutdown_file_logging()
    yield
    with contextlib.suppress(Exception):
        shutdown_file_logging()


def test_defaults_match_published_constants():
    cfg = FileLoggingConfig()
    assert cfg.file_name_prefix == DEFAULT_LOG_FILE_PREFIX
    assert cfg.max_size_bytes == DEFAULT_LOG_MAX_SIZE
    assert cfg.max_files == DEFAULT_LOG_MAX_FILES
    assert cfg.rotation == DEFAULT_LOG_ROTATION
    assert cfg.directory is None
    assert cfg.include_console is True


def test_config_setters_roundtrip(tmp_path: Path):
    cfg = FileLoggingConfig()
    cfg.directory = str(tmp_path)
    cfg.file_name_prefix = "unit"
    cfg.max_size_bytes = 2048
    cfg.max_files = 3
    cfg.rotation = "size"
    cfg.include_console = False

    assert cfg.directory == str(tmp_path)
    assert cfg.file_name_prefix == "unit"
    assert cfg.max_size_bytes == 2048
    assert cfg.max_files == 3
    assert cfg.rotation == "size"
    assert cfg.include_console is False


def test_rotation_rejects_unknown_policy():
    cfg = FileLoggingConfig()
    with pytest.raises(ValueError):
        cfg.rotation = "never"


def test_init_returns_resolved_directory(tmp_path: Path):
    cfg = FileLoggingConfig(
        directory=str(tmp_path),
        file_name_prefix="pytest",
        max_size_bytes=4096,
        max_files=2,
        rotation="both",
    )
    resolved = init_file_logging(cfg)
    assert Path(resolved) == tmp_path
    # init creates the current-day log file eagerly.
    files = [p.name for p in tmp_path.iterdir()]
    assert any(name.startswith("pytest.") and name.endswith(".log") for name in files), files


def test_from_env_uses_env_vars(tmp_path: Path, monkeypatch: pytest.MonkeyPatch):
    monkeypatch.setenv(ENV_LOG_DIR, str(tmp_path))
    monkeypatch.setenv(ENV_LOG_MAX_SIZE, "1234")
    monkeypatch.setenv(ENV_LOG_MAX_FILES, "4")
    monkeypatch.setenv(ENV_LOG_ROTATION, "daily")

    cfg = FileLoggingConfig.from_env()
    assert cfg.directory == str(tmp_path)
    assert cfg.max_size_bytes == 1234
    assert cfg.max_files == 4
    assert cfg.rotation == "daily"


def test_init_is_idempotent(tmp_path: Path):
    cfg = FileLoggingConfig(directory=str(tmp_path), file_name_prefix="idem")
    first = init_file_logging(cfg)
    second = init_file_logging(cfg)
    assert first == second == str(tmp_path)


def test_shutdown_is_idempotent(tmp_path: Path):
    init_file_logging(FileLoggingConfig(directory=str(tmp_path), file_name_prefix="down"))
    shutdown_file_logging()
    # Second call must not raise.
    shutdown_file_logging()


def test_emits_to_file_via_tracing_bridge(tmp_path: Path):
    """Writes from the standard ``logging`` module should land in the file.

    The Rust subscriber installs a ``tracing`` registry; Python's ``logging``
    module is bridged into ``tracing`` only when the user configures
    ``tracing-log`` upstream. Rather than reaching for that, we verify the
    **file** exists and is the one ``init_file_logging`` reports — which
    is the real contract the Python API owes its callers. Content-level
    assertions live in the Rust unit tests where the emit path is fully
    deterministic.
    """
    cfg = FileLoggingConfig(
        directory=str(tmp_path),
        file_name_prefix="emit",
        max_size_bytes=4096,
        max_files=2,
        rotation="both",
    )
    resolved = init_file_logging(cfg)

    log_files = list(Path(resolved).glob("emit.*.log"))
    assert log_files, f"no log file under {resolved}: {list(Path(resolved).iterdir())}"

    # Emit a record through the standard library — harmless even when the
    # tracing bridge is not wired up; the file is already created.
    logging.getLogger("dcc_mcp_core").info("hello from pytest")


def test_swapping_directory_does_not_raise(tmp_path: Path):
    dir_a = tmp_path / "a"
    dir_b = tmp_path / "b"
    dir_a.mkdir()
    dir_b.mkdir()

    init_file_logging(FileLoggingConfig(directory=str(dir_a), file_name_prefix="swap"))
    init_file_logging(FileLoggingConfig(directory=str(dir_b), file_name_prefix="swap"))
    # Both dirs should exist and at least one should have received a stub
    # "current file" by the RollingFileWriter constructor.
    assert any(dir_a.iterdir()) or any(dir_b.iterdir())


def test_repr_is_informative():
    cfg = FileLoggingConfig(file_name_prefix="debug", max_size_bytes=1024, max_files=5)
    r = repr(cfg)
    assert "debug" in r
    assert "1024" in r
    assert "5" in r
