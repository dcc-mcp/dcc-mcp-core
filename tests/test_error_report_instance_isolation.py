from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import sqlite3


def _load_error_report_module():
    script = Path(__file__).parents[1] / "python/dcc_mcp_core/skills/dcc-diagnostics/scripts/error_report.py"
    spec = importlib.util.spec_from_file_location("error_report_instance_isolation", script)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


def _write_failed_job(path: Path, tool: str, error: str) -> None:
    connection = sqlite3.connect(str(path))
    connection.execute("CREATE TABLE jobs (status TEXT, tool TEXT, error TEXT, updated_at TEXT)")
    connection.execute("INSERT INTO jobs VALUES ('failed', ?, ?, '2026-09-01')", (tool, error))
    connection.commit()
    connection.close()


def test_error_report_does_not_read_a_sibling_instance_database(tmp_path, monkeypatch, capsys):
    _write_failed_job(
        tmp_path / "dcc-mcp-maya-other-instance-jobs.db",
        "foreign.tool",
        r"foreign-error C:\Users\foreign\scene.ma",
    )
    monkeypatch.setenv("DCC_MCP_JOB_INSTANCE_KEY", "current-instance")

    module = _load_error_report_module()
    module.main(dcc_name="maya", log_dir=str(tmp_path), tail=10, job_limit=10)
    report = json.loads(capsys.readouterr().out)

    assert not report["context"]["jobs"]["available"]
    serialized = json.dumps(report)
    assert "foreign.tool" not in serialized
    assert "foreign-error" not in serialized
    assert "foreign\\scene.ma" not in serialized


def test_error_report_reads_only_the_exact_current_instance_database(tmp_path, monkeypatch, capsys):
    _write_failed_job(
        tmp_path / "dcc-mcp-maya-current-instance-jobs.db",
        "current.tool",
        r"current-error C:\Users\current\scene.ma",
    )
    _write_failed_job(
        tmp_path / "dcc-mcp-maya-other-instance-jobs.db",
        "foreign.tool",
        "foreign-error",
    )
    monkeypatch.setenv("DCC_MCP_JOB_INSTANCE_KEY", "current-instance")

    module = _load_error_report_module()
    module.main(dcc_name="maya", log_dir=str(tmp_path), tail=10, job_limit=10)
    report = json.loads(capsys.readouterr().out)

    serialized = json.dumps(report)
    assert report["context"]["jobs"]["available"]
    assert "current.tool" in serialized
    assert "foreign.tool" not in serialized
    assert "foreign-error" not in serialized
    assert report["context"]["jobs"]["db_path"].endswith("dcc-mcp-maya-current-instance-jobs.db")


def test_error_report_does_not_read_a_sibling_instance_log(tmp_path, monkeypatch, capsys):
    sibling = tmp_path / "dcc-mcp-maya.999999.20260901.log"
    sibling.write_text(r"ERROR sibling-instance-secret C:\Users\foreign\scene.ma", encoding="utf-8")
    monkeypatch.setenv("DCC_MCP_JOB_INSTANCE_KEY", "current-instance")

    module = _load_error_report_module()
    module.main(dcc_name="maya", log_dir=str(tmp_path), tail=10, job_limit=10)
    report = json.loads(capsys.readouterr().out)

    assert not report["context"]["log"]["available"]
    serialized = json.dumps(report)
    assert "sibling-instance-secret" not in serialized
    assert "999999" not in serialized
    assert "foreign\\scene.ma" not in serialized


def test_error_report_reads_only_the_exact_current_process_log(tmp_path, monkeypatch, capsys):
    pid = 424242
    current = tmp_path / f"dcc-mcp-maya.{pid}.20260901.log"
    sibling = tmp_path / "dcc-mcp-maya.999999.20260901.log"
    current.write_text("ERROR current-instance-error", encoding="utf-8")
    sibling.write_text("ERROR sibling-instance-secret", encoding="utf-8")
    monkeypatch.setattr("os.getpid", lambda: pid)

    module = _load_error_report_module()
    module.main(dcc_name="maya", log_dir=str(tmp_path), tail=10, job_limit=10)
    report = json.loads(capsys.readouterr().out)

    serialized = json.dumps(report)
    assert report["context"]["log"]["available"]
    assert "current-instance-error" in serialized
    assert "sibling-instance-secret" not in serialized
    assert report["context"]["log"]["log_file"].endswith(current.name)
