"""Tests for project-level state persistence (issue #576)."""

from __future__ import annotations

from pathlib import Path

from dcc_mcp_core.project import PROJECT_DIR_NAME
from dcc_mcp_core.project import PROJECT_STATE_FILE
from dcc_mcp_core.project import DccProject
from dcc_mcp_core.project import ProjectState


def test_project_state_round_trips_dict() -> None:
    state = ProjectState(
        scene_path="/show/shot/main.ma",
        loaded_assets=["/assets/char.ma"],
        active_skills=["maya-geometry"],
        checkpoint_ids=["job-1"],
        metadata={"units": "cm"},
        session_id="session",
        updated_at=123.0,
    )

    restored = ProjectState.from_dict(state.to_dict())

    assert restored.scene_path == "/show/shot/main.ma"
    assert restored.loaded_assets == ["/assets/char.ma"]
    assert restored.active_skills == ["maya-geometry"]
    assert restored.checkpoint_ids == ["job-1"]
    assert restored.metadata == {"units": "cm"}
    assert restored.session_id == "session"


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
    project.add_checkpoint_id("job-abc")
    project.update_metadata(units="cm", up_axis="y")

    restored = DccProject.load(scene)

    assert restored.state.loaded_assets == [str(tmp_path / "char.ma")]
    assert restored.state.active_skills == ["maya-lookdev"]
    assert restored.state.checkpoint_ids == ["job-abc"]
    assert restored.state.metadata == {"units": "cm", "up_axis": "y"}


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
    project.add_checkpoint_id("job-a")

    payload = project.resume_session()

    assert payload["scene_path"] == str(tmp_path / "shot_040.ma")
    assert payload["loaded_assets"] == ["asset.ma"]
    assert payload["active_skills"] == ["skill-a"]
    assert payload["checkpoint_ids"] == ["job-a"]
    assert payload["project_dir"] == str(project.project_dir)
    assert payload["state_path"] == str(project.state_path)
