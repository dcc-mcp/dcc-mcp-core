"""Unit tests for Python wheel compatibility validation."""

from __future__ import annotations

from pathlib import Path
import zipfile

from scripts.ci.check_python_wheel import _expanded_filename_tags
from scripts.ci.check_python_wheel import validate_wheel
from scripts.ci.python_support_contract import load_contract

_REPO_ROOT = Path(__file__).resolve().parents[1]


def _write_wheel(
    path: Path,
    *,
    pure: bool,
    with_core: bool,
    requires_python: str = ">=3.7",
    tags: list[str] | None = None,
) -> None:
    root_is_pure = "true" if pure else "false"
    wheel_tags = tags or sorted(_expanded_filename_tags(path))
    with zipfile.ZipFile(path, "w") as archive:
        archive.writestr(
            "dcc_mcp_core-1.0.0.dist-info/METADATA",
            f"Metadata-Version: 2.1\nName: dcc-mcp-core\nVersion: 1.0.0\nRequires-Python: {requires_python}\n",
        )
        archive.writestr(
            "dcc_mcp_core-1.0.0.dist-info/WHEEL",
            "Wheel-Version: 1.0\nRoot-Is-Purelib: {}\n{}".format(
                root_is_pure,
                "".join(f"Tag: {tag}\n" for tag in wheel_tags),
            ),
        )
        archive.writestr("dcc_mcp_core/__init__.py", "")
        if with_core:
            archive.writestr("dcc_mcp_core/_core.pyd", b"native")


def test_native_py37_wheel_requires_cp37_tag_and_core(tmp_path: Path) -> None:
    wheel = tmp_path / "dcc_mcp_core-1.0.0-cp37-cp37m-win_amd64.whl"
    _write_wheel(wheel, pure=False, with_core=True)
    assert validate_wheel(wheel, "native_py37", load_contract(_REPO_ROOT)) == []


def test_lite_py37_wheel_rejects_compiled_core(tmp_path: Path) -> None:
    wheel = tmp_path / "dcc_mcp_core-1.0.0-py3-none-any.whl"
    _write_wheel(wheel, pure=True, with_core=True)
    errors = validate_wheel(wheel, "lite_py37", load_contract(_REPO_ROOT))
    assert any("compiled dcc_mcp_core._core" in error for error in errors)


def test_wheel_rejects_requires_python_drift(tmp_path: Path) -> None:
    wheel = tmp_path / "dcc_mcp_core-1.0.0-cp38-abi3-win_amd64.whl"
    _write_wheel(wheel, pure=False, with_core=True, requires_python=">=3.8")
    errors = validate_wheel(wheel, "abi3", load_contract(_REPO_ROOT))
    assert any("Requires-Python" in error for error in errors)


def test_wheel_rejects_internal_tag_drift(tmp_path: Path) -> None:
    wheel = tmp_path / "dcc_mcp_core-1.0.0-cp37-cp37m-win_amd64.whl"
    _write_wheel(wheel, pure=False, with_core=True, tags=["cp38-abi3-win_amd64"])
    errors = validate_wheel(wheel, "native_py37", load_contract(_REPO_ROOT))
    assert any("WHEEL metadata tags" in error for error in errors)


def test_manylinux_compressed_filename_tags_expand_to_wheel_metadata(tmp_path: Path) -> None:
    wheel = tmp_path / ("dcc_mcp_core-1.0.0-cp37-cp37m-manylinux_2_17_x86_64.manylinux2014_x86_64.whl")
    _write_wheel(wheel, pure=False, with_core=True)
    assert validate_wheel(wheel, "native_py37", load_contract(_REPO_ROOT)) == []
