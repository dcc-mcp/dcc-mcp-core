"""Safe acceptance fixtures for released engine products.

These helpers write disposable, project-local editor files so the acceptance
matrix can exercise the *safe* discovery paths for Unity/Tuanjie, Unreal, and
Godot **without launching an editor or mutating a user's project**.  They are
data-only: no subprocess is ever spawned, and every fixture writes only into
the directory the caller passes (normally a ``pytest`` ``tmp_path``).

Version gates fail closed: an unsupported editor version raises
:class:`AcceptanceValidationError` instead of being recorded as accepted.
"""

from __future__ import annotations

import json
from pathlib import Path
import re
from typing import Any

from dcc_mcp_core.verification.acceptance import AcceptanceValidationError

#: Minimum supported Unity/Tuanjie editor major (Unity 2021 / Tuanjie 2022).
MIN_UNITY_EDITOR_MAJOR = 2021
#: Minimum supported Unreal Engine major.
MIN_UNREAL_ENGINE_MAJOR = 5
#: Bounded Godot ``config_version`` probe window (Godot 3.x through 4.x).
MIN_GODOT_CONFIG_VERSION = 3
MAX_GODOT_CONFIG_VERSION = 5

_UNITY_FLAVORS = ("unity", "tuanjie")
_MAJOR_RE = re.compile(r"^[0-9]+")


def _major(value: str) -> int | None:
    match = _MAJOR_RE.match(value.strip())
    return int(match.group(0)) if match else None


def _require_major(name: str, value: str, minimum: int) -> int:
    major = _major(value)
    if major is None:
        raise AcceptanceValidationError(f"{name} must begin with a numeric major version: {value!r}")
    if major < minimum:
        raise AcceptanceValidationError(f"{name} is unsupported: {value!r} (minimum major {minimum})")
    return major


def _write(path: Path, content: str) -> str:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")
    return str(path)


def unity_project_version(
    project_dir: Any,
    editor_version: str,
    *,
    flavor: str = "unity",
    custom_editor_path: str | None = None,
) -> dict[str, Any]:
    """Write a disposable Unity/Tuanjie ``ProjectVersion.txt``.

    ``flavor`` must be ``unity`` or ``tuanjie``.  ``editor_version`` must be a
    supported editor version (major >= :data:`MIN_UNITY_EDITOR_MAJOR`); anything
    older is rejected.  When ``custom_editor_path`` is supplied it becomes the
    recorded editor decision; otherwise the decision is the exact editor
    version.  Never launches the editor.
    """
    if flavor not in _UNITY_FLAVORS:
        raise AcceptanceValidationError(f"flavor must be one of: {', '.join(_UNITY_FLAVORS)}")
    _require_major("editor_version", editor_version, MIN_UNITY_EDITOR_MAJOR)
    root = Path(project_dir)
    content = f"m_EditorVersion: {editor_version}\nm_EditorVersionWithRevision: {editor_version} (0)\n"
    written = _write(root / "ProjectSettings" / "ProjectVersion.txt", content)
    decision = custom_editor_path if custom_editor_path else f"exact editor version {editor_version}"
    return {
        "kind": flavor,
        "project_dir": str(root),
        "files_written": [written],
        "editor_version": editor_version,
        "decision": decision,
        "launched": False,
    }


def unreal_project(
    project_dir: Any,
    engine_association: str,
    *,
    project_name: str = "DisposableProject",
    engine_root: str | None = None,
    released_package: str | None = None,
) -> dict[str, Any]:
    """Write a disposable ``.uproject`` with an ``EngineAssociation``.

    ``engine_association`` must be a supported engine (numeric major >=
    :data:`MIN_UNREAL_ENGINE_MAJOR`, or a custom engine identifier such as a
    source-build GUID).  ``engine_root`` records a custom engine-root decision;
    ``released_package`` records an explicit released-package capability
    check.  Never launches the editor.
    """
    association = engine_association.strip()
    if not association:
        raise AcceptanceValidationError("engine_association must be a non-empty string")
    major = _major(association)
    if major is not None and major < MIN_UNREAL_ENGINE_MAJOR:
        raise AcceptanceValidationError(
            f"engine_association is unsupported: {association!r} (minimum major {MIN_UNREAL_ENGINE_MAJOR})"
        )
    root = Path(project_dir)
    uproject: dict[str, Any] = {
        "FileVersion": 3,
        "EngineAssociation": association,
        "Category": "",
        "Description": "",
    }
    if released_package:
        uproject["Plugins"] = [{"Name": released_package, "Enabled": True}]
    written = _write(root / f"{project_name}.uproject", json.dumps(uproject, indent=2, sort_keys=True))
    decision = engine_root if engine_root else f"engine association {association}"
    return {
        "kind": "unreal",
        "project_dir": str(root),
        "files_written": [written],
        "engine_association": association,
        "engine_root": engine_root,
        "released_package": released_package,
        "decision": decision,
        "launched": False,
    }


def godot_project(
    project_dir: Any,
    *,
    config_version: int = 4,
    project_name: str = "DisposableProject",
    custom_editor_path: str | None = None,
) -> dict[str, Any]:
    """Write a disposable ``project.godot`` with a bounded ``config_version``.

    ``config_version`` must lie within :data:`MIN_GODOT_CONFIG_VERSION` ..
    :data:`MAX_GODOT_CONFIG_VERSION`; an out-of-range version is rejected.
    ``custom_editor_path`` records the exact custom editor path.  Never
    launches the editor.
    """
    if isinstance(config_version, bool) or not isinstance(config_version, int):
        raise AcceptanceValidationError("config_version must be an integer")
    if not MIN_GODOT_CONFIG_VERSION <= config_version <= MAX_GODOT_CONFIG_VERSION:
        raise AcceptanceValidationError(
            f"config_version is unsupported: {config_version} "
            f"(expected {MIN_GODOT_CONFIG_VERSION}..{MAX_GODOT_CONFIG_VERSION})"
        )
    root = Path(project_dir)
    content = (
        "; Engine configuration file.\n"
        f"config_version={config_version}\n"
        "\n"
        "[application]\n"
        f'config/name="{project_name}"\n'
    )
    written = _write(root / "project.godot", content)
    decision = custom_editor_path if custom_editor_path else f"config_version {config_version}"
    return {
        "kind": "godot",
        "project_dir": str(root),
        "files_written": [written],
        "config_version": config_version,
        "decision": decision,
        "launched": False,
    }


def godot_version_probe(project_dir: Any) -> int:
    """Read ``config_version`` back from a ``project.godot`` file.

    This is the bounded version probe: it reads the file (no editor launch)
    and returns the version only when it lies inside the supported window.
    """
    path = Path(project_dir) / "project.godot"
    if not path.is_file():
        raise AcceptanceValidationError("project.godot not found")
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.startswith("config_version="):
            raw = line.split("=", 1)[1].strip()
            if not raw.isdigit():
                raise AcceptanceValidationError(f"config_version is not an integer: {raw!r}")
            version = int(raw)
            if not MIN_GODOT_CONFIG_VERSION <= version <= MAX_GODOT_CONFIG_VERSION:
                raise AcceptanceValidationError(f"config_version is unsupported: {version}")
            return version
    raise AcceptanceValidationError("project.godot has no config_version")


__all__ = [
    "MAX_GODOT_CONFIG_VERSION",
    "MIN_GODOT_CONFIG_VERSION",
    "MIN_UNITY_EDITOR_MAJOR",
    "MIN_UNREAL_ENGINE_MAJOR",
    "godot_project",
    "godot_version_probe",
    "unity_project_version",
    "unreal_project",
]
