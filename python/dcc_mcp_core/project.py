"""Project-level state persistence for DCC sessions (issue #576).

This module complements job-scoped checkpoints with a durable project file
stored at ``.dcc-mcp/project.json`` next to the scene/document.  The core keeps
the schema DCC-agnostic; adapters can store host-specific state in
``ProjectState.metadata``.
"""

from __future__ import annotations

from dataclasses import dataclass
from dataclasses import field
from pathlib import Path
import time
from typing import Any
import uuid

from dcc_mcp_core import json_dumps
from dcc_mcp_core import json_loads

PROJECT_DIR_NAME = ".dcc-mcp"
PROJECT_STATE_FILE = "project.json"


@dataclass
class ProjectState:
    """Serializable project/session state shared across jobs."""

    scene_path: str = ""
    loaded_assets: list[str] = field(default_factory=list)
    active_skills: list[str] = field(default_factory=list)
    active_tool_groups: list[str] = field(default_factory=list)
    checkpoint_ids: list[str] = field(default_factory=list)
    metadata: dict[str, Any] = field(default_factory=dict)
    session_id: str = field(default_factory=lambda: uuid.uuid4().hex)
    created_at: float = field(default_factory=time.time)
    updated_at: float = field(default_factory=time.time)

    def touch(self) -> None:
        """Refresh the last-updated timestamp."""
        self.updated_at = time.time()

    def to_dict(self) -> dict[str, Any]:
        """Return a JSON-serializable state payload."""
        return {
            "scene_path": self.scene_path,
            "loaded_assets": list(self.loaded_assets),
            "active_skills": list(self.active_skills),
            "active_tool_groups": list(self.active_tool_groups),
            "checkpoint_ids": list(self.checkpoint_ids),
            "metadata": dict(self.metadata),
            "session_id": self.session_id,
            "created_at": self.created_at,
            "updated_at": self.updated_at,
        }

    @classmethod
    def from_dict(cls, payload: dict[str, Any]) -> ProjectState:
        """Build state from a persisted JSON payload.

        Older payloads that predate ``created_at`` / ``active_tool_groups``
        (issue #576 follow-up) still load cleanly: missing fields fall back to
        sensible defaults, so on-disk state written by earlier releases
        remains forward-compatible.
        """
        now = time.time()
        return cls(
            scene_path=str(payload.get("scene_path") or ""),
            loaded_assets=[str(p) for p in payload.get("loaded_assets") or []],
            active_skills=[str(s) for s in payload.get("active_skills") or []],
            active_tool_groups=[str(g) for g in payload.get("active_tool_groups") or []],
            checkpoint_ids=[str(i) for i in payload.get("checkpoint_ids") or []],
            metadata=dict(payload.get("metadata") or {}),
            session_id=str(payload.get("session_id") or uuid.uuid4().hex),
            created_at=float(payload.get("created_at") or payload.get("updated_at") or now),
            updated_at=float(payload.get("updated_at") or now),
        )


class DccProject:
    """Persistent project state rooted at ``.dcc-mcp/project.json``."""

    def __init__(self, project_dir: str | Path, state: ProjectState | None = None) -> None:
        self.project_dir = Path(project_dir)
        self.state_path = self.project_dir / PROJECT_STATE_FILE
        self.state = state or ProjectState()
        self._checkpoints: Any = None  # lazy CheckpointStore (avoid import cycle)

    @classmethod
    def open(cls, scene_path: str | Path) -> DccProject:
        """Open or create project state for a scene/document path."""
        scene = Path(scene_path)
        project_dir = scene.parent / PROJECT_DIR_NAME
        project = cls(project_dir)
        if project.state_path.is_file():
            project = cls.load(project_dir)
        if not project.state.scene_path:
            project.state.scene_path = str(scene)
        project.save()
        return project

    @classmethod
    def load(cls, scene_path_or_project_dir: str | Path) -> DccProject:
        """Load project state from a scene path or an existing project dir."""
        raw = Path(scene_path_or_project_dir)
        project_dir = raw if raw.name == PROJECT_DIR_NAME else raw.parent / PROJECT_DIR_NAME
        state_path = project_dir / PROJECT_STATE_FILE
        if not state_path.is_file():
            return cls(project_dir, ProjectState(scene_path=str(raw) if raw.name != PROJECT_DIR_NAME else ""))
        payload = json_loads(state_path.read_text(encoding="utf-8"))
        return cls(project_dir, ProjectState.from_dict(payload))

    def save(self) -> None:
        """Persist the current project state to disk."""
        self.state.touch()
        self.project_dir.mkdir(parents=True, exist_ok=True)
        self.state_path.write_text(json_dumps(self.state.to_dict(), indent=2), encoding="utf-8")

    def update_scene_path(self, scene_path: str | Path) -> None:
        self.state.scene_path = str(scene_path)
        self.save()

    def add_asset(self, asset_path: str | Path) -> None:
        self._append_unique(self.state.loaded_assets, str(asset_path))
        self.save()

    def remove_asset(self, asset_path: str | Path) -> bool:
        removed = self._remove_value(self.state.loaded_assets, str(asset_path))
        if removed:
            self.save()
        return removed

    def activate_skill(self, skill_name: str) -> None:
        self._append_unique(self.state.active_skills, skill_name)
        self.save()

    def deactivate_skill(self, skill_name: str) -> bool:
        removed = self._remove_value(self.state.active_skills, skill_name)
        if removed:
            self.save()
        return removed

    def activate_tool_group(self, group_name: str) -> None:
        self._append_unique(self.state.active_tool_groups, group_name)
        self.save()

    def deactivate_tool_group(self, group_name: str) -> bool:
        removed = self._remove_value(self.state.active_tool_groups, group_name)
        if removed:
            self.save()
        return removed

    def add_checkpoint_id(self, checkpoint_id: str) -> None:
        self._append_unique(self.state.checkpoint_ids, checkpoint_id)
        self.save()

    def remove_checkpoint_id(self, checkpoint_id: str) -> bool:
        removed = self._remove_value(self.state.checkpoint_ids, checkpoint_id)
        if removed:
            self.save()
        return removed

    def update_metadata(self, **metadata: Any) -> None:
        self.state.metadata.update(metadata)
        self.save()

    @property
    def checkpoints(self) -> Any:
        """Return a :class:`CheckpointStore` rooted at ``<project_dir>/checkpoints.json``.

        The store is created lazily on first access so the module does not
        eagerly import :mod:`dcc_mcp_core.checkpoint` (which would be wasteful
        for callers that only touch ``ProjectState``).  Subsequent accesses
        return the same instance, so writes are visible within a session
        without explicit flushing — ``CheckpointStore`` already persists on
        every mutation.

        Part of issue #576's "integrate with existing CheckpointStore"
        acceptance criterion.
        """
        if self._checkpoints is None:
            # Local import to avoid circular module-load during package init.
            from dcc_mcp_core.checkpoint import CheckpointStore

            self.project_dir.mkdir(parents=True, exist_ok=True)
            self._checkpoints = CheckpointStore(path=self.project_dir / "checkpoints.json")
        return self._checkpoints

    def resume_session(self) -> dict[str, Any]:
        """Return the persisted context adapters need to restore a session."""
        return {
            "scene_path": self.state.scene_path,
            "loaded_assets": list(self.state.loaded_assets),
            "active_skills": list(self.state.active_skills),
            "active_tool_groups": list(self.state.active_tool_groups),
            "checkpoint_ids": list(self.state.checkpoint_ids),
            "metadata": dict(self.state.metadata),
            "session_id": self.state.session_id,
            "created_at": self.state.created_at,
            "updated_at": self.state.updated_at,
            "project_dir": str(self.project_dir),
            "state_path": str(self.state_path),
        }

    @staticmethod
    def _append_unique(items: list[str], value: str) -> None:
        if value not in items:
            items.append(value)

    @staticmethod
    def _remove_value(items: list[str], value: str) -> bool:
        if value not in items:
            return False
        items.remove(value)
        return True


__all__ = [
    "PROJECT_DIR_NAME",
    "PROJECT_STATE_FILE",
    "DccProject",
    "ProjectState",
]
