"""Unit tests for the VRS HTTP replayer (scripts/vrs_replay.py)."""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import sys


def _load_replay_module():
    root = Path(__file__).resolve().parents[1]
    path = root / "scripts" / "vrs_replay.py"
    spec = importlib.util.spec_from_file_location("vrs_replay", path)
    assert spec and spec.loader
    mod = importlib.util.module_from_spec(spec)
    sys.modules["vrs_replay"] = mod
    spec.loader.exec_module(mod)
    return mod


def test_json_subset_match_nested():
    vr = _load_replay_module()
    big = {"output": {"success": True, "message": "ok"}, "slug": "x"}
    assert vr._json_subset_match(big, {"output": {"success": True}})
    assert not vr._json_subset_match(big, {"output": {"success": False}})


def test_get_by_pointer():
    vr = _load_replay_module()
    data = {"hits": [{"tool_slug": "maya.abcdef01.maya_scripting__execute_python"}], "total": 1}
    assert vr._get_by_pointer(data, "/hits/0/tool_slug") == "maya.abcdef01.maya_scripting__execute_python"


def test_substitute_captures():
    vr = _load_replay_module()
    body = {"tool_slug": "{{capture:slug}}", "arguments": {"code": "1"}}
    out = vr._substitute_captures(body, {"slug": "maya.abc.maya_scripting__execute_python"})
    assert out["tool_slug"] == "maya.abc.maya_scripting__execute_python"


def test_substitute_captures_in_headers():
    vr = _load_replay_module()
    headers = {"X-Request-Id": "{{capture:request_id}}"}
    out = vr._substitute_captures(headers, {"request_id": "req-123"})
    assert out["X-Request-Id"] == "req-123"


def test_check_expect_any_one_matches():
    vr = _load_replay_module()
    raw = json.dumps({"output": {"success": False}})
    parsed = json.loads(raw)
    err = vr._check_expect_any(
        200,
        raw,
        parsed,
        {},
        [
            {"status": 404},
            {"status": 200, "json_subset": {"output": {"success": False}}},
        ],
    )
    assert err is None


def test_check_expect_body_contains_all():
    vr = _load_replay_module()
    raw = '{"instances":[{"port":0,"status":"booting"}]}'
    err = vr._check_expect(
        200,
        raw,
        json.loads(raw),
        {},
        {"status": 200, "body_contains_all": ['"port":0', '"status":"booting"']},
    )
    assert err is None


def test_skip_preflight_body_not_contains(monkeypatch):
    vr = _load_replay_module()

    def fake_request(*_args, **_kwargs):
        return 200, '{"instances":[]}', {"instances": []}, {}

    monkeypatch.setattr(vr, "_do_request", fake_request)
    assert vr._run_skip_preflight(
        "http://127.0.0.1:1",
        {
            "http": {"method": "GET", "path": "/v1/debug/instances?view=all"},
            "skip_when": {"body_not_contains": '"port":0'},
        },
        1.0,
    )
