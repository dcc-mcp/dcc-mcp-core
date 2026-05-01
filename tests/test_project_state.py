"""Tests for project-level state persistence (issue #576)."""

from __future__ import annotations

from pathlib import Path

from dcc_mcp_core import json_dumps
from dcc_mcp_core.project import PROJECT_DIR_NAME
from dcc_mcp_core.project import PROJECT_STATE_FILE
from dcc_mcp_core.project import DccProject
from dcc_mcp_core.project import ProjectState


def test_project_state_round_trips_dict() -> None:
    state = ProjectState(
        scene_path="/show/shot/main.ma",
        loaded_assets=["/assets/char.ma"],
        active_skills=["maya-geometry"],
        active_tool_groups=["maya-lookdev-tools"],
        checkpoint_ids=["job-1"],
        metadata={"units": "cm"},
        session_id="session",
        created_at=100.0,
        updated_at=123.0,
    )

    restored = ProjectState.from_dict(state.to_dict())

    assert restored.scene_path == "/show/shot/main.ma"
    assert restored.loaded_assets == ["/assets/char.ma"]
    assert restored.active_skills == ["maya-geometry"]
    assert restored.active_tool_groups == ["maya-lookdev-tools"]
    assert restored.checkpoint_ids == ["job-1"]
    assert restored.metadata == {"units": "cm"}
    assert restored.session_id == "session"
    assert restored.created_at == 100.0
    assert restored.updated_at == 123.0


def test_project_state_from_legacy_payload_without_new_fields() -> None:
    """Loading a project.json written before #576 follow-up must still work."""
    # This mirrors what older releases persisted: no `created_at`, no
    # `active_tool_groups`.  `from_dict` must fill in defaults without raising.
    legacy_payload = {
        "scene_path": "/show/shot/legacy.ma",
        "loaded_assets": ["/assets/char.ma"],
        "active_skills": ["maya-geometry"],
        "checkpoint_ids": ["job-legacy"],
        "metadata": {"units": "cm"},
        "session_id": "legacy-session",
        "updated_at": 999.0,
    }

    restored = ProjectState.from_dict(legacy_payload)

    assert restored.active_tool_groups == []
    # `created_at` should fall back to the legacy `updated_at` when absent,
    # not to "now", so the timestamp remains meaningful.
    assert restored.created_at == 999.0
    assert restored.updated_at == 999.0


def test_dcc_project_open_creates_state_next_to_scene(tmp_path: Path) -> None:
    scene = tmp_path / "shot_010.ma"

    project = DccProject.open(scene)

    assert project.project_dir == tmp_path / PROJECT_DIR_NAME
    assert project.state_path == tmp_path / PROJECT_DIR_NAME / PROJECT_STATE_FILE
    assert project.state_path.is_file()
    assert project.state.scene_path == str(scene)


def test_dcc_project_mutations_auto_save_and_reload(tmp_path: Path) -> None:
    scene = tmp_path / "shot_020.ma"
    project = DccProject.open(scene)

    project.add_asset(tmp_path / "char.ma")
    project.add_asset(tmp_path / "char.ma")
    project.activate_skill("maya-lookdev")
    project.activate_tool_group("maya-lookdev-tools")
    project.add_checkpoint_id("job-abc")
    project.update_metadata(units="cm", up_axis="y")

    restored = DccProject.load(scene)

    assert restored.state.loaded_assets == [str(tmp_path / "char.ma")]
    assert restored.state.active_skills == ["maya-lookdev"]
    assert restored.state.active_tool_groups == ["maya-lookdev-tools"]
    assert restored.state.checkpoint_ids == ["job-abc"]
    assert restored.state.metadata == {"units": "cm", "up_axis": "y"}


def test_dcc_project_tool_group_mutators(tmp_path: Path) -> None:
    project = DccProject.open(tmp_path / "shot_tg.ma")

    project.activate_tool_group("group-a")
    project.activate_tool_group("group-a")  # idempotent
    project.activate_tool_group("group-b")

    restored = DccProject.load(project.project_dir)
    assert restored.state.active_tool_groups == ["group-a", "group-b"]

    assert project.deactivate_tool_group("group-a") is True
    assert project.deactivate_tool_group("group-a") is False  # already gone

    restored2 = DccProject.load(project.project_dir)
    assert restored2.state.active_tool_groups == ["group-b"]


def test_dcc_project_preserves_created_at_across_saves(tmp_path: Path) -> None:
    scene = tmp_path / "shot_ts.ma"
    project = DccProject.open(scene)
    original_created_at = project.state.created_at

    project.add_asset("asset.ma")  # triggers save → touches updated_at only
    project.activate_skill("skill-a")

    restored = DccProject.load(scene)
    assert restored.state.created_at == original_created_at
    assert restored.state.updated_at >= original_created_at


def test_dcc_project_loads_legacy_project_json_without_new_fields(tmp_path: Path) -> None:
    project_dir = tmp_path / PROJECT_DIR_NAME
    project_dir.mkdir()
    legacy_payload = {
        "scene_path": str(tmp_path / "legacy.ma"),
        "loaded_assets": [],
        "active_skills": [],
        "checkpoint_ids": [],
        "metadata": {},
        "session_id": "legacy",
        "updated_at": 555.0,
    }
    (project_dir / PROJECT_STATE_FILE).write_text(json_dumps(legacy_payload, indent=2), encoding="utf-8")

    restored = DccProject.load(project_dir)
    assert restored.state.active_tool_groups == []
    assert restored.state.created_at == 555.0


def test_dcc_project_remove_helpers(tmp_path: Path) -> None:
    project = DccProject.open(tmp_path / "shot_030.ma")
    project.add_asset("asset.ma")
    project.activate_skill("skill-a")
    project.add_checkpoint_id("job-a")

    assert project.remove_asset("asset.ma") is True
    assert project.remove_asset("missing.ma") is False
    assert project.deactivate_skill("skill-a") is True
    assert project.deactivate_skill("missing") is False
    assert project.remove_checkpoint_id("job-a") is True
    assert project.remove_checkpoint_id("missing") is False

    restored = DccProject.load(project.project_dir)
    assert restored.state.loaded_assets == []
    assert restored.state.active_skills == []
    assert restored.state.checkpoint_ids == []


def test_dcc_project_resume_session_payload(tmp_path: Path) -> None:
    project = DccProject.open(tmp_path / "shot_040.ma")
    project.add_asset("asset.ma")
    project.activate_skill("skill-a")
    project.activate_tool_group("group-a")
    project.add_checkpoint_id("job-a")

    payload = project.resume_session()

    assert payload["scene_path"] == str(tmp_path / "shot_040.ma")
    assert payload["loaded_assets"] == ["asset.ma"]
    assert payload["active_skills"] == ["skill-a"]
    assert payload["active_tool_groups"] == ["group-a"]
    assert payload["checkpoint_ids"] == ["job-a"]
    assert payload["project_dir"] == str(project.project_dir)
    assert payload["state_path"] == str(project.state_path)
    assert payload["session_id"] == project.state.session_id
    assert "created_at" in payload
    assert "updated_at" in payload
