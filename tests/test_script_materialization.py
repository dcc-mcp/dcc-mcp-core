"""Tests for per-instance script materialization (#1220)."""

from __future__ import annotations

from datetime import datetime
from datetime import timedelta
from datetime import timezone
from pathlib import Path

import pytest

import dcc_mcp_core
from dcc_mcp_core.script_execution import cleanup_temp_scripts
from dcc_mcp_core.script_execution import write_temp_script
from dcc_mcp_core.script_materialization import MaterializedScript
from dcc_mcp_core.script_materialization import cleanup_materialized_scripts
from dcc_mcp_core.script_materialization import materialize_script
from dcc_mcp_core.script_materialization import sanitize_materialization_segment


def test_materialization_helpers_are_exported() -> None:
    assert dcc_mcp_core.MaterializedScript is MaterializedScript
    assert dcc_mcp_core.materialize_script is materialize_script
    assert "MaterializedScript" in dcc_mcp_core.__all__
    assert "materialize_script" in dcc_mcp_core.__all__


def test_sanitize_materialization_segment_blocks_traversal_and_windows_paths() -> None:
    assert sanitize_materialization_segment("maya/../../prod") == "maya_.._.._prod"
    assert sanitize_materialization_segment(r"C:\show\maya:2026") == "C__show_maya_2026"
    assert sanitize_materialization_segment("...") == "unknown"


def test_materialize_script_returns_file_ref_descriptor(tmp_path: Path) -> None:
    descriptor = materialize_script(
        "print('hello')",
        dcc_type="maya",
        instance_id="inst-1",
        session_id="sess-1",
        root=tmp_path,
        ttl_secs=60,
        tool_call_id="tool-1",
        correlation_id="corr-1",
    )

    path = Path(descriptor.file_path)
    assert path.is_file()
    assert path.read_text(encoding="utf-8") == "print('hello')"
    assert descriptor.bytes == len(b"print('hello')")
    assert descriptor.file_ref["uri"].startswith("file:")
    assert descriptor.file_ref["digest"] == f"sha256:{descriptor.sha256}"
    assert descriptor.file_ref["session_id"] == "sess-1"
    assert descriptor.file_ref["tool_call_id"] == "tool-1"
    assert descriptor.file_ref["metadata"]["dcc_type"] == "maya"
    assert str(path).startswith(str(tmp_path.resolve()))


def test_materialize_script_reuses_identical_content_when_requested(tmp_path: Path) -> None:
    first = materialize_script(
        "x = 1",
        dcc_type="photoshop",
        instance_id="ps-1",
        session_id="sess-1",
        root=tmp_path,
        reuse=True,
        reuse_key="bootstrap",
    )
    second = materialize_script(
        "x = 1",
        dcc_type="photoshop",
        instance_id="ps-1",
        session_id="sess-1",
        root=tmp_path,
        reuse=True,
        reuse_key="bootstrap",
    )

    assert first.file_path == second.file_path
    assert first.reused is False
    assert second.reused is True


def test_cleanup_materialized_scripts_removes_expired_only(tmp_path: Path) -> None:
    expired = materialize_script(
        "x = 1",
        dcc_type="zbrush",
        instance_id="z-1",
        session_id="sess-1",
        root=tmp_path,
        ttl_secs=1,
    )
    live = materialize_script(
        "x = 2",
        dcc_type="zbrush",
        instance_id="z-1",
        session_id="sess-1",
        root=tmp_path,
    )

    removed = cleanup_materialized_scripts(
        root=tmp_path,
        now=datetime.now(timezone.utc).replace(tzinfo=None) + timedelta(seconds=2),
    )

    assert removed >= 2
    assert not Path(expired.file_path).exists()
    assert Path(live.file_path).exists()


def test_materialize_script_validates_suffix_and_ttl(tmp_path: Path) -> None:
    descriptor = materialize_script(
        "echo ok",
        dcc_type="custom",
        instance_id="inst",
        session_id="sess",
        root=tmp_path,
        language="powershell",
        suffix="../ps1",
    )

    assert descriptor.suffix == "._ps1"
    assert descriptor.file_path.endswith("._ps1")
    with pytest.raises(ValueError, match="ttl_secs"):
        materialize_script(
            "pass",
            dcc_type="custom",
            instance_id="inst",
            session_id="sess",
            root=tmp_path,
            ttl_secs=0,
        )


def test_write_temp_script_is_thin_wrapper_over_materialization_store() -> None:
    path = Path(write_temp_script("print('compat')", suffix=".py", prefix="compat"))
    try:
        assert path.is_file()
        assert path.read_text(encoding="utf-8") == "print('compat')"
        assert "generic" in path.parts
        assert path.name.startswith("compat_")
    finally:
        cleanup_temp_scripts()
