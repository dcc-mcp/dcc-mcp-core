"""Contracts for standalone skill-script parameter transport."""

from __future__ import annotations

import io
import json

import pytest

from dcc_mcp_core.skill import run_main
from dcc_mcp_core.skill import skill_success


def test_run_main_forwards_stdin_json_to_skill(monkeypatch, capsys):
    """The subprocess executor's stdin payload must reach the skill entry point."""
    monkeypatch.setattr(
        "sys.stdin",
        io.StringIO('{"name":"SIGNAL FORGE TITLE","width":1920,"layers":["title","subtitle"]}'),
    )

    def create_document(name="Untitled", width=640, layers=None):
        return skill_success("created", name=name, width=width, layers=layers)

    with pytest.raises(SystemExit) as exit_info:
        run_main(create_document)

    assert exit_info.value.code == 0
    payload = json.loads(capsys.readouterr().out)
    assert payload["context"] == {
        "name": "SIGNAL FORGE TITLE",
        "width": 1920,
        "layers": ["title", "subtitle"],
    }


@pytest.mark.parametrize(
    "result",
    [
        {"success": "yes", "message": "malformed"},
        {"message": "missing success"},
        ["not", "a", "mapping"],
    ],
)
def test_run_main_exits_from_normalized_failure(result, monkeypatch, capsys):
    monkeypatch.setattr("sys.stdin", io.StringIO(""))

    with pytest.raises(SystemExit) as exit_info:
        run_main(lambda: result)

    assert exit_info.value.code == 1
    payload = json.loads(capsys.readouterr().out)
    assert payload["success"] is False
    assert payload["error"] == "invalid_result_envelope"


def test_run_main_serialization_failure_exits_nonzero(monkeypatch, capsys):
    monkeypatch.setattr("sys.stdin", io.StringIO(""))

    with pytest.raises(SystemExit) as exit_info:
        run_main(lambda: skill_success("done", value=object()))

    assert exit_info.value.code == 1
    payload = json.loads(capsys.readouterr().out)
    assert payload["success"] is False
    assert payload["error"] == "non_serializable_result"
