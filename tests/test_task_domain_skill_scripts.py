from __future__ import annotations

import importlib.util
import json
from pathlib import Path
from types import SimpleNamespace

ROOT = Path(__file__).parents[1]


def _load_script(relative_path: str):
    path = ROOT / relative_path
    spec = importlib.util.spec_from_file_location(path.stem, path)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_import_asset_missing_file_returns_error_without_crashing() -> None:
    module = _load_script("skills/asset/scripts/import_asset.py")

    result = module.import_asset({"variants": [{"path": "missing.fbx", "format": "fbx"}]})

    assert result["success"] is False
    assert result["error"] == "Asset file not found: missing.fbx"


def test_asset_instance_resolution_never_falls_back_to_another_dcc(monkeypatch) -> None:
    module = _load_script("skills/asset/scripts/import_asset.py")
    payload = {
        "instances": [
            {
                "dcc_type": "blender",
                "instance_short": "blender-abcd",
                "direct_control": {"ready": True},
            }
        ]
    }
    monkeypatch.setattr(
        module.subprocess,
        "run",
        lambda *args, **kwargs: SimpleNamespace(returncode=0, stdout=json.dumps(payload), stderr=""),
    )

    assert module._get_first_instance("maya") is None


def test_interact_widget_uses_snapshot_act_snapshot_stop(monkeypatch) -> None:
    module = _load_script("skills/ui/scripts/interact_widget.py")
    calls = []
    snapshots = iter(["snapshot-before", "snapshot-after"])

    def fake_run(argv, **kwargs):
        tool = argv[2]
        arguments = json.loads(argv[4])
        calls.append((tool, arguments))
        if tool.endswith("ui_control__snapshot"):
            payload = {"success": True, "context": {"snapshot_id": next(snapshots)}}
        else:
            payload = {"success": True}
        return SimpleNamespace(returncode=0, stdout=json.dumps(payload), stderr="")

    monkeypatch.setattr(module.subprocess, "run", fake_run)

    result = module.interact_widget(
        {"control_id": "saveButton", "window_title": "Maya"},
        instance="maya-abcd",
        session_id="save-scene",
    )

    assert result["success"] is True
    assert [tool.rsplit(".", 1)[-1] for tool, _ in calls] == [
        "ui_control__snapshot",
        "ui_control__act",
        "ui_control__snapshot",
        "ui_control__stop_computer_use",
    ]
    assert calls[1][1] == {
        "session_id": "save-scene",
        "snapshot_id": "snapshot-before",
        "control_id": "saveButton",
        "action": "click",
        "window_title": "Maya",
    }


def test_interact_widget_stops_after_action_failure_without_retry(monkeypatch) -> None:
    module = _load_script("skills/ui/scripts/interact_widget.py")
    calls = []

    def fake_run(argv, **kwargs):
        tool = argv[2]
        calls.append(tool)
        if tool.endswith("ui_control__snapshot"):
            payload = {"success": True, "context": {"snapshot_id": "snapshot-before"}}
        elif tool.endswith("ui_control__act"):
            payload = {
                "success": True,
                "result": {"structuredContent": {"success": False, "error": "user_interrupted"}},
            }
        else:
            payload = {"success": True}
        return SimpleNamespace(returncode=0, stdout=json.dumps(payload), stderr="")

    monkeypatch.setattr(module.subprocess, "run", fake_run)

    result = module.interact_widget(
        {"control_id": "saveButton"},
        instance="maya-abcd",
        session_id="save-scene",
    )

    assert result["success"] is False
    assert [tool.rsplit(".", 1)[-1] for tool in calls] == [
        "ui_control__snapshot",
        "ui_control__act",
        "ui_control__stop_computer_use",
    ]


def test_capture_ui_state_uses_canonical_snapshot_and_stops(monkeypatch, tmp_path) -> None:
    module = _load_script("skills/ui/scripts/capture_ui_state.py")
    calls = []

    def fake_run(argv, **kwargs):
        tool = argv[2]
        calls.append(tool)
        if tool.endswith("ui_control__snapshot"):
            payload = {
                "success": True,
                "result": {
                    "structuredContent": {
                        "success": True,
                        "context": {
                            "capture_provenance": {"backend": "windows_graphics_capture"},
                            "__rich__": {"kind": "image"},
                        },
                    }
                },
            }
        else:
            payload = {"success": True}
        return SimpleNamespace(returncode=0, stdout=json.dumps(payload), stderr="")

    monkeypatch.setattr(module.subprocess, "run", fake_run)

    result = module.capture_ui_state(
        instance="maya-abcd",
        session_id="capture-state",
        window_title="Maya",
        output_dir=str(tmp_path),
    )

    assert result["success"] is True
    assert result["capture_provenance"] == {"backend": "windows_graphics_capture"}
    assert result["has_screenshot"] is True
    assert [tool.rsplit(".", 1)[-1] for tool in calls] == [
        "ui_control__snapshot",
        "qt_ui_inspector__snapshot_tree",
        "qt_ui_inspector__list_windows",
        "ui_control__stop_computer_use",
    ]
