"""Unit tests for Python wheel compatibility validation."""

from __future__ import annotations

from pathlib import Path
import stat
import subprocess
import zipfile

import pytest
from scripts.ci.check_python_wheel import _expanded_filename_tags
from scripts.ci.check_python_wheel import main
from scripts.ci.check_python_wheel import validate_wheel
from scripts.ci.python_support_contract import load_contract
from scripts.ci.smoke_zero_typing_extensions import _check_wheel_archive
from scripts.ci.smoke_zero_typing_extensions import _is_default_requirement as smoke_is_default_requirement

_REPO_ROOT = Path(__file__).resolve().parents[1]
_INSTALL_SOP_SCHEMA_SOURCE = "python/dcc_mcp_core/schemas/adapter-install-sop-v1.schema.json"
_INSTALL_SOP_SCHEMA_MEMBER = "dcc_mcp_core/schemas/adapter-install-sop-v1.schema.json"
_INSTALL_SOP_SCHEMA_GIT_BYTES = subprocess.run(
    ["git", "cat-file", "blob", f"HEAD:{_INSTALL_SOP_SCHEMA_SOURCE}"],
    cwd=_REPO_ROOT,
    check=True,
    stdout=subprocess.PIPE,
).stdout


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
    ui_control_contract: str = "canonical",
    install_sop_schema_bytes: bytes = _INSTALL_SOP_SCHEMA_GIT_BYTES,
    requires_dist: list[str] | None = None,
    extra_members: list[str | zipfile.ZipInfo] | None = None,
) -> None:
    root_is_pure = "true" if pure else "false"
    wheel_tags = tags or sorted(_expanded_filename_tags(path))
    dist_info = distribution.replace("-", "_")
    with zipfile.ZipFile(path, "w") as archive:
        archive.writestr(
            f"{dist_info}-{version}.dist-info/METADATA",
            f"Metadata-Version: 2.1\nName: {distribution}\nVersion: {version}\n"
            f"Requires-Python: {requires_python}\n"
            + "".join(f"Requires-Dist: {requirement}\n" for requirement in requires_dist or []),
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
        if distribution == "dcc-mcp-core":
            archive.writestr(_INSTALL_SOP_SCHEMA_MEMBER, install_sop_schema_bytes)
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
        for member in extra_members or []:
            archive.writestr(member, b"injected")


def test_native_py37_wheel_requires_cp37_tag_and_core(tmp_path: Path) -> None:
    wheel = tmp_path / "dcc_mcp_core-1.0.0-cp37-cp37m-win_amd64.whl"
    _write_wheel(wheel, pure=False, with_core=True)
    assert validate_wheel(wheel, "native_py37", "windows-x86_64", load_contract(_REPO_ROOT)) == []


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


def test_core_wheel_rejects_install_sop_schema_byte_drift(tmp_path: Path) -> None:
    wheel = tmp_path / "dcc_mcp_core-1.0.0-cp38-abi3-win_amd64.whl"
    crlf_schema = _INSTALL_SOP_SCHEMA_GIT_BYTES.replace(b"\n", b"\r\n")
    _write_wheel(
        wheel,
        pure=False,
        with_core=True,
        install_sop_schema_bytes=crlf_schema,
    )

    errors = validate_wheel(wheel, "abi3", "windows-x86_64", load_contract(_REPO_ROOT))

    assert any("adapter-install-sop-v1.schema.json SHA-256" in error for error in errors)


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


@pytest.mark.parametrize(
    "profile,filename,platform,pure,with_core",
    [
        ("native_py37", "dcc_mcp_core-1.0.0-cp37-cp37m-win_amd64.whl", "windows-x86_64", False, True),
        ("lite_py37", "dcc_mcp_core-1.0.0-py3-none-any.whl", "any", True, False),
        ("abi3", "dcc_mcp_core-1.0.0-cp38-abi3-win_amd64.whl", "windows-x86_64", False, True),
    ],
)
@pytest.mark.parametrize(
    "requirement",
    [
        "typing_extensions==4.7.1; python_version < '3.8'",
        "typing.extensions==4.7.1; python_version < '3.8'",
    ],
)
def test_core_wheels_reject_typing_extensions_runtime_dependency(
    tmp_path: Path,
    profile: str,
    filename: str,
    platform: str,
    pure: bool,
    with_core: bool,
    requirement: str,
) -> None:
    wheel = tmp_path / filename
    _write_wheel(
        wheel,
        pure=pure,
        with_core=with_core,
        requires_dist=[requirement],
    )

    errors = validate_wheel(wheel, profile, platform, load_contract(_REPO_ROOT))

    assert any("forbidden runtime dependency 'typing-extensions'" in error for error in errors)


def test_core_wheel_allows_explicit_test_only_typing_extensions(tmp_path: Path) -> None:
    wheel = tmp_path / "dcc_mcp_core-1.0.0-cp38-abi3-win_amd64.whl"
    _write_wheel(
        wheel,
        pure=False,
        with_core=True,
        requires_dist=["typing-extensions==4.7.1; python_version < '3.8' and extra == 'test'"],
    )

    errors = validate_wheel(wheel, "abi3", "windows-x86_64", load_contract(_REPO_ROOT))

    assert not any("forbidden runtime dependency 'typing-extensions'" in error for error in errors)


@pytest.mark.parametrize(
    "member",
    [
        "typing_extensions.py",
        "./Typing-Extensions.py",
        "vendor/typing_extensions/__init__.py",
        "typing_extensions-4.12.2.dist-info/METADATA",
    ],
)
def test_wheel_gates_reject_normalized_typing_extensions_payloads(tmp_path: Path, member: str) -> None:
    wheel = tmp_path / "dcc_mcp_core-1.0.0-py3-none-any.whl"
    _write_wheel(wheel, pure=True, with_core=False, extra_members=[member])

    errors = validate_wheel(wheel, "lite_py37", "any", load_contract(_REPO_ROOT))
    assert any("typing_extensions payload" in error for error in errors)

    with pytest.raises(RuntimeError, match="typing_extensions payload"):
        _check_wheel_archive(wheel, tmp_path / "isolated")


@pytest.mark.parametrize("member", ["../typing_extensions.py", "/typing_extensions.py", "C:\\typing_extensions.py"])
def test_wheel_gates_reject_unsafe_archive_member_paths(tmp_path: Path, member: str) -> None:
    wheel = tmp_path / "dcc_mcp_core-1.0.0-py3-none-any.whl"
    _write_wheel(wheel, pure=True, with_core=False, extra_members=[member])

    errors = validate_wheel(wheel, "lite_py37", "any", load_contract(_REPO_ROOT))
    assert any("unsafe archive member" in error for error in errors)

    with pytest.raises(RuntimeError, match="unsafe archive member"):
        _check_wheel_archive(wheel, tmp_path / "isolated")


@pytest.mark.parametrize(
    "member",
    [
        "dcc_mcp_core/typing_extensions.py.",
        "dcc_mcp_core/typing_extensions.py ",
        "dcc_mcp_core/typing\uff3fextensions.py",
        "dcc_mcp_core/TYPING_EXTENSIONS.PY",
    ],
)
def test_wheel_gates_reject_portable_typing_extensions_aliases(tmp_path: Path, member: str) -> None:
    wheel = tmp_path / "dcc_mcp_core-1.0.0-py3-none-any.whl"
    _write_wheel(wheel, pure=True, with_core=False, extra_members=[member])

    errors = validate_wheel(wheel, "lite_py37", "any", load_contract(_REPO_ROOT))
    assert any("typing_extensions payload" in error for error in errors)

    with pytest.raises(RuntimeError, match="typing_extensions payload"):
        _check_wheel_archive(wheel, tmp_path / "isolated")


@pytest.mark.parametrize(
    "member",
    [
        "dcc_mcp_core/CON.py",
        "dcc_mcp_core/payload.py:typing_extensions.py",
        "dcc_mcp_core/trailing. /payload.py",
        "dcc_mcp_core//payload.py",
    ],
)
def test_wheel_gates_reject_windows_unsafe_aliases(tmp_path: Path, member: str) -> None:
    wheel = tmp_path / "dcc_mcp_core-1.0.0-py3-none-any.whl"
    _write_wheel(wheel, pure=True, with_core=False, extra_members=[member])

    errors = validate_wheel(wheel, "lite_py37", "any", load_contract(_REPO_ROOT))
    assert any("unsafe archive member" in error for error in errors)

    with pytest.raises(RuntimeError, match="unsafe archive member"):
        _check_wheel_archive(wheel, tmp_path / "isolated")


@pytest.mark.parametrize(
    "members",
    [
        ["dcc_mcp_core/duplicate.py", "dcc_mcp_core/./duplicate.py"],
        ["dcc_mcp_core/Readme.py", "dcc_mcp_core/\uff32\uff25\uff21\uff24\uff2d\uff25.py"],
    ],
)
def test_wheel_gates_reject_duplicate_portable_member_paths(tmp_path: Path, members: list[str]) -> None:
    wheel = tmp_path / "dcc_mcp_core-1.0.0-py3-none-any.whl"
    _write_wheel(
        wheel,
        pure=True,
        with_core=False,
        extra_members=members,
    )

    errors = validate_wheel(wheel, "lite_py37", "any", load_contract(_REPO_ROOT))
    assert any("duplicate" in error.lower() for error in errors)

    with pytest.raises(RuntimeError, match="duplicate"):
        _check_wheel_archive(wheel, tmp_path / "isolated")


def test_wheel_gates_reject_symlink_members(tmp_path: Path) -> None:
    wheel = tmp_path / "dcc_mcp_core-1.0.0-py3-none-any.whl"
    link = zipfile.ZipInfo("dcc_mcp_core/link.py")
    link.create_system = 3
    link.external_attr = (stat.S_IFLNK | 0o777) << 16
    _write_wheel(wheel, pure=True, with_core=False, extra_members=[link])

    errors = validate_wheel(wheel, "lite_py37", "any", load_contract(_REPO_ROOT))
    assert any("symlink" in error.lower() for error in errors)

    with pytest.raises(RuntimeError, match="symlink"):
        _check_wheel_archive(wheel, tmp_path / "isolated")


def test_zero_backport_smoke_fails_closed_for_ambiguous_extra_marker() -> None:
    assert not smoke_is_default_requirement("typing-extensions; python_version < '3.8' and extra == 'test'")
    assert smoke_is_default_requirement("typing-extensions; python_version < '3.8'")
    assert smoke_is_default_requirement("typing-extensions; extra == 'test' or python_version < '3.8'")


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
