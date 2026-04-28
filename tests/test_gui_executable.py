"""Tests for ``dcc_mcp_core.is_gui_executable`` / ``correct_python_executable`` (issue #524)."""

# Import built-in modules
from __future__ import annotations

from pathlib import Path

# Import third-party modules
import pytest

# Import local modules
import dcc_mcp_core
from dcc_mcp_core import GuiExecutableHint
from dcc_mcp_core import correct_python_executable
from dcc_mcp_core import is_gui_executable


def test_exports_available() -> None:
    """All three new symbols must be importable from the top-level package."""
    for name in ("GuiExecutableHint", "is_gui_executable", "correct_python_executable"):
        assert hasattr(dcc_mcp_core, name)
        assert name in dcc_mcp_core.__all__


@pytest.mark.parametrize(
    ("path", "expected_kind"),
    [
        ("C:/Program Files/Autodesk/Maya2024/bin/maya.exe", "maya"),
        ("/Applications/Autodesk/maya2024/Maya.app/Contents/bin/maya", "maya"),
        ("C:/Program Files/Side Effects Software/Houdini/bin/houdini.exe", "houdini"),
        ("houdinifx.exe", "houdini"),
        ("HoudiniCore", "houdini"),
        ("UnrealEditor.exe", "unreal"),
        ("blender", "blender"),
        ("blender.exe", "blender"),
        ("3dsmax.exe", "3dsmax"),
        ("nuke.exe", "nuke"),
        ("nukestudio", "nuke"),
        ("modo", "modo"),
        ("motionbuilder.exe", "motionbuilder"),
        ("cinema4d", "c4d"),
        ("c4d.exe", "c4d"),
        ("katana.exe", "katana"),
    ],
)
def test_is_gui_executable_detects_known_dccs(path: str, expected_kind: str) -> None:
    hint = is_gui_executable(path)
    assert hint is not None, f"{path} must be detected"
    assert isinstance(hint, GuiExecutableHint)
    assert hint.dcc_kind == expected_kind


@pytest.mark.parametrize(
    "path",
    [
        "python.exe",
        "/usr/bin/python3",
        "mayapy.exe",
        "hython",
        "vscode.exe",
        "/bin/ls",
        "",
    ],
)
def test_is_gui_executable_returns_none_for_non_dcc(path: str) -> None:
    assert is_gui_executable(path) is None


def test_hint_recommended_replacement_when_sibling_exists(tmp_path: Path) -> None:
    maya = tmp_path / "maya.exe"
    mayapy = tmp_path / "mayapy.exe"
    maya.write_bytes(b"")
    mayapy.write_bytes(b"")

    hint = is_gui_executable(str(maya))
    assert hint is not None
    assert hint.recommended_replacement is not None
    # str() to normalise path separators across Windows / POSIX.
    assert Path(hint.recommended_replacement).resolve() == mayapy.resolve()


def test_hint_recommended_replacement_none_when_sibling_missing(tmp_path: Path) -> None:
    maya = tmp_path / "maya.exe"
    maya.write_bytes(b"")
    hint = is_gui_executable(str(maya))
    assert hint is not None
    assert hint.recommended_replacement is None


def test_correct_python_executable_returns_sibling(tmp_path: Path) -> None:
    maya = tmp_path / "maya.exe"
    mayapy = tmp_path / "mayapy.exe"
    maya.write_bytes(b"")
    mayapy.write_bytes(b"")
    fixed = correct_python_executable(str(maya))
    assert Path(fixed).resolve() == mayapy.resolve()


def test_correct_python_executable_passes_through_for_python() -> None:
    assert correct_python_executable("python.exe") == Path("python.exe")
    assert correct_python_executable("/usr/bin/python3") == Path("/usr/bin/python3")


def test_correct_python_executable_passes_through_when_sibling_missing() -> None:
    # Detected as Maya GUI, but no sibling on disk → return path unchanged.
    p = "C:/nope/maya.exe"
    assert correct_python_executable(p) == Path(p)


def test_hint_repr_contains_dcc_kind() -> None:
    hint = is_gui_executable("blender")
    assert hint is not None
    assert "blender" in repr(hint)
    assert "GuiExecutableHint" in repr(hint)


def test_unreal_editor_recommends_cmd_sibling(tmp_path: Path) -> None:
    gui = tmp_path / "UnrealEditor.exe"
    cmd = tmp_path / "UnrealEditor-Cmd.exe"
    gui.write_bytes(b"")
    cmd.write_bytes(b"")
    hint = is_gui_executable(str(gui))
    assert hint is not None
    assert hint.dcc_kind == "unreal"
    assert hint.recommended_replacement is not None
    assert Path(hint.recommended_replacement).resolve() == cmd.resolve()
