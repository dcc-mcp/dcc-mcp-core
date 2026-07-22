"""Unit tests for Python wheel compatibility validation."""

from __future__ import annotations

from pathlib import Path
import zipfile

import pytest
from scripts.ci.check_python_wheel import _expanded_filename_tags
from scripts.ci.check_python_wheel import main
from scripts.ci.check_python_wheel import validate_wheel
from scripts.ci.python_support_contract import load_contract

_REPO_ROOT = Path(__file__).resolve().parents[1]


def _write_wheel(
    path: Path,
    *,
    pure: bool,
    with_core: bool,
    version: str = "1.0.0",
    requires_python: str = ">=3.7",
    tags: list[str] | None = None,
    distribution: str = "dcc-mcp-core",
    extension_module: str = "dcc_mcp_core/_core",
    with_removed_capture_helper: bool = False,
    with_ui_control_host: bool | None = None,
    ui_control_contract: str = "canonical",
) -> None:
    root_is_pure = "true" if pure else "false"
    wheel_tags = tags or sorted(_expanded_filename_tags(path))
    dist_info = distribution.replace("-", "_")
    with zipfile.ZipFile(path, "w") as archive:
        archive.writestr(
            f"{dist_info}-{version}.dist-info/METADATA",
            f"Metadata-Version: 2.1\nName: {distribution}\nVersion: {version}\nRequires-Python: {requires_python}\n",
        )
        archive.writestr(
            f"{dist_info}-{version}.dist-info/WHEEL",
            "Wheel-Version: 1.0\nRoot-Is-Purelib: {}\n{}".format(
                root_is_pure,
                "".join(f"Tag: {tag}\n" for tag in wheel_tags),
            ),
        )
        package = extension_module.split("/", 1)[0]
        archive.writestr(f"{package}/__init__.py", "")
        if distribution == "dcc-mcp-core" and ui_control_contract == "canonical":
            archive.writestr(
                "dcc_mcp_core/adapter_contracts.py",
                "class UiControlPolicy:\n    pass\n\nclass UiControlAuditRecord:\n    pass\n",
            )
            archive.writestr(
                "dcc_mcp_core/skills/ui-control/SKILL.md",
                "---\nname: ui-control\ndescription: canonical ui_control__snapshot skill\n---\n",
            )
            archive.writestr("dcc_mcp_core/skills/ui-control/tools.yaml", "tools: []\n")
        elif distribution == "dcc-mcp-core" and ui_control_contract == "legacy-skill":
            archive.writestr(
                "dcc_mcp_core/adapter_contracts.py",
                "class UiControlPolicy:\n    pass\n\nclass UiControlAuditRecord:\n    pass\n",
            )
            archive.writestr(
                "dcc_mcp_core/skills/app-ui/SKILL.md",
                "---\nname: app-ui\ndescription: legacy app_ui__snapshot skill\n---\n",
            )
            archive.writestr("dcc_mcp_core/skills/app-ui/tools.yaml", "tools: []\n")
        if with_core:
            archive.writestr(f"{extension_module}.pyd", b"native")
        if with_removed_capture_helper:
            archive.writestr("dcc_mcp_core/bin/dcc-mcp-capture-helper.exe", b"MZremoved")
        if with_ui_control_host is None:
            with_ui_control_host = distribution == "dcc-mcp-core" and with_core and "win_amd64" in path.name
        if with_ui_control_host:
            archive.writestr("dcc_mcp_core/bin/dcc-mcp-ui-control-host.exe", b"MZhost")


def test_native_py37_wheel_requires_cp37_tag_and_core(tmp_path: Path) -> None:
    wheel = tmp_path / "dcc_mcp_core-1.0.0-cp37-cp37m-win_amd64.whl"
    _write_wheel(wheel, pure=False, with_core=True)
    assert validate_wheel(wheel, "native_py37", "windows-x86_64", load_contract(_REPO_ROOT)) == []


def test_windows_core_wheel_requires_ui_control_host_from_01960(tmp_path: Path) -> None:
    wheel = tmp_path / "dcc_mcp_core-0.19.60-cp38-abi3-win_amd64.whl"
    _write_wheel(
        wheel,
        pure=False,
        with_core=True,
        version="0.19.60",
        with_ui_control_host=False,
    )
    errors = validate_wheel(wheel, "abi3", "windows-x86_64", load_contract(_REPO_ROOT))
    assert any("missing required member" in error and "ui-control-host.exe" in error for error in errors)


def test_core_wheel_rejects_legacy_app_ui_skill_with_new_python_contracts(tmp_path: Path) -> None:
    wheel = tmp_path / "dcc_mcp_core-0.19.63-cp38-abi3-win_amd64.whl"
    _write_wheel(
        wheel,
        pure=False,
        with_core=True,
        version="0.19.63",
        ui_control_contract="legacy-skill",
    )

    errors = validate_wheel(wheel, "abi3", "windows-x86_64", load_contract(_REPO_ROOT))

    assert any("missing required UI Control member" in error for error in errors)
    assert any("removed app-ui skill" in error for error in errors)


def test_windows_core_wheel_rejects_removed_capture_helper(tmp_path: Path) -> None:
    wheel = tmp_path / "dcc_mcp_core-1.0.0-cp38-abi3-win_amd64.whl"
    _write_wheel(wheel, pure=False, with_core=True, with_removed_capture_helper=True)
    errors = validate_wheel(wheel, "abi3", "windows-x86_64", load_contract(_REPO_ROOT))
    assert any("forbidden member" in error and "capture-helper.exe" in error for error in errors)


def test_lite_wheel_rejects_staged_ui_control_host(tmp_path: Path) -> None:
    wheel = tmp_path / "dcc_mcp_core-1.0.0-py3-none-any.whl"
    _write_wheel(wheel, pure=True, with_core=False, with_ui_control_host=True)
    errors = validate_wheel(wheel, "lite_py37", "any", load_contract(_REPO_ROOT))
    assert any("forbidden member" in error and "ui-control-host.exe" in error for error in errors)


def test_linux_native_wheel_rejects_staged_ui_control_host(tmp_path: Path) -> None:
    wheel = tmp_path / "dcc_mcp_core-1.0.0-cp37-cp37m-manylinux2014_x86_64.whl"
    _write_wheel(wheel, pure=False, with_core=True, with_ui_control_host=True)
    errors = validate_wheel(wheel, "native_py37", "linux-x86_64", load_contract(_REPO_ROOT))
    assert any("forbidden member" in error and "ui-control-host.exe" in error for error in errors)


def test_lite_py37_wheel_rejects_compiled_core(tmp_path: Path) -> None:
    wheel = tmp_path / "dcc_mcp_core-1.0.0-py3-none-any.whl"
    _write_wheel(wheel, pure=True, with_core=True)
    errors = validate_wheel(wheel, "lite_py37", "any", load_contract(_REPO_ROOT))
    assert any("Root-Is-Purelib" not in error and "compiled" in error for error in errors)


def test_wheel_rejects_requires_python_drift(tmp_path: Path) -> None:
    wheel = tmp_path / "dcc_mcp_core-1.0.0-cp38-abi3-win_amd64.whl"
    _write_wheel(wheel, pure=False, with_core=True, requires_python=">=3.8")
    errors = validate_wheel(wheel, "abi3", "windows-x86_64", load_contract(_REPO_ROOT))
    assert any("Requires-Python" in error for error in errors)


def test_wheel_rejects_internal_tag_drift(tmp_path: Path) -> None:
    wheel = tmp_path / "dcc_mcp_core-1.0.0-cp37-cp37m-win_amd64.whl"
    _write_wheel(wheel, pure=False, with_core=True, tags=["cp38-abi3-win_amd64"])
    errors = validate_wheel(wheel, "native_py37", "windows-x86_64", load_contract(_REPO_ROOT))
    assert any("WHEEL metadata tags" in error for error in errors)


def test_manylinux_compressed_filename_tags_expand_to_wheel_metadata(tmp_path: Path) -> None:
    wheel = tmp_path / ("dcc_mcp_core-1.0.0-cp37-cp37m-manylinux_2_17_x86_64.manylinux2014_x86_64.whl")
    _write_wheel(wheel, pure=False, with_core=True)
    assert validate_wheel(wheel, "native_py37", "linux-x86_64", load_contract(_REPO_ROOT)) == []


def test_core_native_py37_rejects_manylinux_2_28(tmp_path: Path) -> None:
    wheel = tmp_path / "dcc_mcp_core-1.0.0-cp37-cp37m-manylinux_2_28_x86_64.whl"
    _write_wheel(wheel, pure=False, with_core=True)
    errors = validate_wheel(wheel, "native_py37", "linux-x86_64", load_contract(_REPO_ROOT))
    assert any("manylinux_2_28_x86_64" in error and "not allowed" in error for error in errors)


def test_semantic_native_py37_has_an_explicit_manylinux_2_28_profile(tmp_path: Path) -> None:
    wheel = tmp_path / "dcc_mcp_core_semantic-1.0.0-cp37-cp37m-manylinux_2_28_x86_64.whl"
    _write_wheel(
        wheel,
        pure=False,
        with_core=True,
        distribution="dcc-mcp-core-semantic",
        extension_module="dcc_mcp_core_semantic/_native",
    )
    contract = load_contract(_REPO_ROOT)
    assert validate_wheel(wheel, "semantic_native_py37", "linux-x86_64", contract) == []

    legacy = tmp_path / "dcc_mcp_core_semantic-1.0.0-cp37-cp37m-manylinux_2_17_x86_64.whl"
    _write_wheel(
        legacy,
        pure=False,
        with_core=True,
        distribution="dcc-mcp-core-semantic",
        extension_module="dcc_mcp_core_semantic/_native",
    )
    errors = validate_wheel(legacy, "semantic_native_py37", "linux-x86_64", contract)
    assert any("manylinux_2_17_x86_64" in error and "not allowed" in error for error in errors)


def test_semantic_native_py37_accepts_windows_x86_64(tmp_path: Path) -> None:
    wheel = tmp_path / "dcc_mcp_core_semantic-1.0.0-cp37-cp37m-win_amd64.whl"
    _write_wheel(
        wheel,
        pure=False,
        with_core=True,
        distribution="dcc-mcp-core-semantic",
        extension_module="dcc_mcp_core_semantic/_native",
    )
    assert validate_wheel(wheel, "semantic_native_py37", "windows-x86_64", load_contract(_REPO_ROOT)) == []


@pytest.mark.parametrize(
    ("platform", "platform_tag"),
    [
        ("linux-x86_64", "manylinux_2_28_x86_64"),
        ("windows-x86_64", "win_amd64"),
        ("macos-native", "macosx_11_0_arm64"),
    ],
)
def test_semantic_abi3_contract_covers_every_release_platform(
    tmp_path: Path,
    platform: str,
    platform_tag: str,
) -> None:
    wheel = tmp_path / f"dcc_mcp_core_semantic-1.0.0-cp38-abi3-{platform_tag}.whl"
    _write_wheel(
        wheel,
        pure=False,
        with_core=True,
        distribution="dcc-mcp-core-semantic",
        extension_module="dcc_mcp_core_semantic/_native",
    )
    assert validate_wheel(wheel, "semantic_abi3", platform, load_contract(_REPO_ROOT)) == []


def test_abi3_accepts_historical_compressed_linux_tags(tmp_path: Path) -> None:
    wheel = tmp_path / ("dcc_mcp_core-1.0.0-cp38-abi3-manylinux_2_17_x86_64.manylinux2014_x86_64.whl")
    _write_wheel(wheel, pure=False, with_core=True)
    assert validate_wheel(wheel, "abi3", "linux-x86_64", load_contract(_REPO_ROOT)) == []


def test_core_abi3_rejects_manylinux_2_28(tmp_path: Path) -> None:
    wheel = tmp_path / "dcc_mcp_core-1.0.0-cp38-abi3-manylinux_2_28_x86_64.whl"
    _write_wheel(wheel, pure=False, with_core=True)
    errors = validate_wheel(wheel, "abi3", "linux-x86_64", load_contract(_REPO_ROOT))
    assert any("manylinux_2_28_x86_64" in error and "not allowed" in error for error in errors)


def test_abi3_accepts_historical_compressed_macos_universal2_tags(tmp_path: Path) -> None:
    wheel = tmp_path / (
        "dcc_mcp_core-1.0.0-cp38-abi3-macosx_10_12_x86_64.macosx_11_0_arm64.macosx_10_12_universal2.whl"
    )
    _write_wheel(wheel, pure=False, with_core=True)
    assert validate_wheel(wheel, "abi3", "macos-universal2", load_contract(_REPO_ROOT)) == []


def test_abi3_universal2_profile_rejects_a_single_arch_wheel(tmp_path: Path) -> None:
    wheel = tmp_path / "dcc_mcp_core-1.0.0-cp38-abi3-macosx_10_12_x86_64.whl"
    _write_wheel(wheel, pure=False, with_core=True)
    errors = validate_wheel(wheel, "abi3", "macos-universal2", load_contract(_REPO_ROOT))
    assert any("must include a tag matching" in error for error in errors)


@pytest.mark.parametrize("platform_tag", ["macosx_10_12_x86_64", "macosx_11_0_arm64"])
def test_abi3_accepts_native_macos_runner_tags(tmp_path: Path, platform_tag: str) -> None:
    wheel = tmp_path / f"dcc_mcp_core-1.0.0-cp38-abi3-{platform_tag}.whl"
    _write_wheel(wheel, pure=False, with_core=True)
    assert validate_wheel(wheel, "abi3", "macos-native", load_contract(_REPO_ROOT)) == []


def test_wheel_profile_rejects_an_undeclared_platform(tmp_path: Path) -> None:
    wheel = tmp_path / "dcc_mcp_core-1.0.0-cp37-cp37m-macosx_10_9_x86_64.whl"
    _write_wheel(wheel, pure=False, with_core=True)
    errors = validate_wheel(wheel, "native_py37", "macos-x86_64", load_contract(_REPO_ROOT))
    assert errors == ["profile 'native_py37' does not support platform 'macos-x86_64'"]


def test_cli_accepts_explicit_lite_and_abi3_platforms(tmp_path: Path) -> None:
    lite = tmp_path / "dcc_mcp_core-1.0.0-py3-none-any.whl"
    _write_wheel(lite, pure=True, with_core=False)
    assert main(["--profile", "lite_py37", "--platform", "any", str(lite)]) == 0

    abi3 = tmp_path / "dcc_mcp_core-1.0.0-cp38-abi3-win_amd64.whl"
    _write_wheel(abi3, pure=False, with_core=True)
    assert main(["--profile", "abi3", "--platform", "windows-x86_64", str(abi3)]) == 0


def test_cli_requires_explicit_platform_for_every_profile(tmp_path: Path) -> None:
    wheel = tmp_path / "dcc_mcp_core-1.0.0-cp37-cp37m-win_amd64.whl"
    _write_wheel(wheel, pure=False, with_core=True)
    with pytest.raises(SystemExit, match="2"):
        main(["--profile", "native_py37", str(wheel)])
