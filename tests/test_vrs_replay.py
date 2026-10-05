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


def _write_trace(tmp_path, steps):
    path = tmp_path / "trace.jsonl"
    path.write_text("\n".join(json.dumps(step) for step in steps), encoding="utf-8")
    return str(path)


def _capture_then_assert_trace(assert_id):
    """Build a trace that captures an instance id, then asserts on that row."""
    return [
        {
            "id": "capture_first_instance",
            "http": {"method": "GET", "path": "/v1/instances"},
            "expect": {"status": 200},
            "capture": {"json_pointer": "/instances/0/instance_id", "as": "iid"},
        },
        {
            "id": "readyz_explains_every_row",
            "http": {"method": "GET", "path": "/v1/readyz"},
            "expect_any": [
                {"status": 200, "json_subset": {"instances": [{"instance_id": assert_id}]}},
            ],
        },
    ]


def test_expect_any_substitutes_captures_end_to_end(tmp_path, monkeypatch):
    """A capture used inside `expect_any` must match the live value (#4209).

    Regression: `_substitute_captures` only ran over request body/headers, so
    the literal `{{capture:iid}}` token was compared against the real UUID and
    the step failed even for a perfectly healthy instance. `--dry-run` cannot
    catch this because it never evaluates assertions.
    """
    vr = _load_replay_module()
    instance_id = "0f7a2b31-1111-4222-8333-444455556666"

    def fake_request(base, method, path, body, headers, timeout):
        if path == "/v1/instances":
            payload = {"total": 1, "instances": [{"instance_id": instance_id}]}
        else:
            payload = {
                "instances": [
                    {
                        "instance_id": instance_id,
                        "readiness": {"process": True, "dcc": True},
                    }
                ]
            }
        raw = json.dumps(payload)
        return 200, raw, json.loads(raw), {}

    monkeypatch.setattr(vr, "_do_request", fake_request)

    trace = _write_trace(tmp_path, _capture_then_assert_trace("{{capture:iid}}"))
    assert vr.run_trace("http://127.0.0.1:1", trace, dry_run=False, verbose=False) == 0

    # Negative control: an id that is not the captured one must still fail,
    # otherwise the assertion above is vacuous.
    other = _write_trace(tmp_path, _capture_then_assert_trace("not-the-captured-id"))
    assert vr.run_trace("http://127.0.0.1:1", other, dry_run=False, verbose=False) == 1


def test_expect_substitutes_captures_end_to_end(tmp_path, monkeypatch):
    """The single-`expect` branch is substituted too, not just `expect_any`."""
    vr = _load_replay_module()
    slug = "blender.abcdef01.blender_scene__create_cube"

    def fake_request(base, method, path, body, headers, timeout):
        if path == "/v1/search":
            payload = {"total": 1, "hits": [{"tool_slug": slug}]}
        else:
            # The captured slug must reach the request path as well.
            assert path == f"/v1/describe?tool_slug={slug}", path
            payload = {"tool": {"tool_slug": slug}, "record": {"has_schema": True}}
        raw = json.dumps(payload)
        return 200, raw, json.loads(raw), {}

    monkeypatch.setattr(vr, "_do_request", fake_request)

    trace = _write_trace(
        tmp_path,
        [
            {
                "id": "search",
                "http": {"method": "POST", "path": "/v1/search", "json": {"query": "cube"}},
                "expect": {"status": 200},
                "capture": {"json_pointer": "/hits/0/tool_slug", "as": "slug"},
            },
            {
                "id": "describe",
                "http": {"method": "GET", "path": "/v1/describe?tool_slug={{capture:slug}}"},
                "expect": {
                    "status": 200,
                    "json_subset": {"record": {"has_schema": True}},
                    "body_contains": "{{capture:slug}}",
                },
            },
        ],
    )
    assert vr.run_trace("http://127.0.0.1:1", trace, dry_run=False, verbose=False) == 0


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
