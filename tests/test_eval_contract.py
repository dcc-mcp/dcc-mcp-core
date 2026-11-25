"""Tests for the persisted offline evaluation contract and CLI."""

from __future__ import annotations

import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys

import pytest

from dcc_mcp_core import EvalCase
from dcc_mcp_core import eval_report_json_schema
from dcc_mcp_core import eval_set_json_schema
from dcc_mcp_core import run_eval_set

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "run_eval.py"


def _cli(*arguments: str) -> subprocess.CompletedProcess:
    environment = os.environ.copy()
    python_path = str(ROOT / "python")
    if environment.get("PYTHONPATH"):
        python_path = os.pathsep.join((python_path, environment["PYTHONPATH"]))
    environment["PYTHONPATH"] = python_path
    return subprocess.run(
        [sys.executable, str(SCRIPT), *arguments],
        capture_output=True,
        text=True,
        check=False,
        env=environment,
    )


def _load_script_module():
    spec = importlib.util.spec_from_file_location("run_eval_script", SCRIPT)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_versioned_schemas_are_loadable() -> None:
    assert eval_set_json_schema()["$id"].endswith("eval-set-v1.schema.json")
    assert eval_report_json_schema()["$id"].endswith("eval-report-v1.schema.json")


def test_cli_runs_python_cases_and_validates_report(tmp_path: Path) -> None:
    case_module = tmp_path / "cases.py"
    case_module.write_text(
        "from dcc_mcp_core.verification import EvalCase\n"
        "CASES = [EvalCase('smoke', 'Smoke case', lambda: {'checks': [{'name': 'ready', 'passed': True}]})]\n",
        encoding="utf-8",
    )
    first_report = tmp_path / "first.json"
    second_report = tmp_path / "second.json"

    run = _cli(
        "run",
        "--module",
        str(case_module),
        "--output",
        str(first_report),
        "--generated-at",
        "2026-01-01T00:00:00Z",
    )
    assert run.returncode == 0, run.stderr
    assert _cli("validate", str(first_report), "--kind", "report").returncode == 0

    repeat = _cli(
        "run",
        "--module",
        str(case_module),
        "--output",
        str(second_report),
        "--generated-at",
        "2026-01-01T00:00:00Z",
    )
    assert repeat.returncode == 0, repeat.stderr
    assert _cli("diff", str(first_report), str(second_report)).returncode == 0

    report = json.loads(first_report.read_text(encoding="utf-8"))
    assert report["metrics"]["pass_count"] == 1

    second_report.write_text(
        second_report.read_text(encoding="utf-8").replace('"pass_count": 1', '"pass_count": 0'), encoding="utf-8"
    )
    changed = _cli("diff", str(first_report), str(second_report))
    assert changed.returncode == 1
    assert '"pass_count": 1' in changed.stdout

    second_report.write_bytes(first_report.read_bytes())
    first_report.write_bytes(first_report.read_bytes().replace(b"\n", b"\r\n"))
    line_ending_change = _cli("diff", str(first_report), str(second_report))
    assert line_ending_change.returncode == 1


def test_cli_validates_evaluation_set(tmp_path: Path) -> None:
    document = tmp_path / "set.json"
    document.write_text(
        json.dumps(
            {
                "schema_version": 1,
                "set_version": "eval-set-v1",
                "cases": [
                    {
                        "case_id": "smoke",
                        "title": "Smoke case",
                        "invocation": {},
                        "tags": ["smoke"],
                    }
                ],
            }
        ),
        encoding="utf-8",
    )
    result = _cli("validate", str(document), "--kind", "set")
    assert result.returncode == 0, result.stderr


def test_basic_validation_is_available_without_jsonschema(monkeypatch) -> None:
    module = _load_script_module()
    monkeypatch.setitem(sys.modules, "jsonschema", None)
    module._validate(
        {
            "schema_version": 1,
            "set_version": "eval-set-v1",
            "cases": [{"case_id": "smoke", "title": "Smoke", "invocation": {}, "tags": []}],
        },
        "set",
    )
    with pytest.raises(ValueError):
        module._validate(
            {"schema_version": 1, "set_version": "eval-set-v1", "runs": [{"verdict": "pass"}]},
            "report",
        )


def test_incomplete_checks_fail_closed_into_a_valid_report() -> None:
    report = run_eval_set(
        [EvalCase("incomplete", "Incomplete check", lambda: {"checks": [{"passed": False}]})],
        clock=lambda: 0.0,
    )
    assert report.runs[0].verdict == "inconclusive"
    assert report.runs[0].checks[0]["name"] == "grader_error"
