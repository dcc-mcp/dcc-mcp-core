"""Tests for immutable Core release distribution verification."""

from __future__ import annotations

from email.generator import BytesGenerator
from email.message import Message
import hashlib
import io
import json
from pathlib import Path
import tarfile
import zipfile

import pytest
from scripts.ci import check_release_distribution_set
from scripts.ci.check_release_distribution_set import DistributionSetError
from scripts.ci.check_release_distribution_set import _validate_sdist
from scripts.ci.check_release_distribution_set import classify_distribution_assets
from scripts.ci.check_release_distribution_set import verify_distribution_set

VERSION = "0.20.8"
WHEEL_NAMES = [
    f"dcc_mcp_core-{VERSION}-cp37-cp37m-manylinux_2_17_x86_64.manylinux2014_x86_64.whl",
    f"dcc_mcp_core-{VERSION}-cp37-cp37m-win_amd64.whl",
    (f"dcc_mcp_core-{VERSION}-cp38-abi3-macosx_10_12_x86_64.macosx_11_0_arm64.macosx_10_12_universal2.whl"),
    f"dcc_mcp_core-{VERSION}-cp38-abi3-manylinux_2_17_x86_64.manylinux2014_x86_64.whl",
    f"dcc_mcp_core-{VERSION}-cp38-abi3-win_amd64.whl",
    f"dcc_mcp_core-{VERSION}-py3-none-any.whl",
]
SDIST_NAME = f"dcc_mcp_core-{VERSION}.tar.gz"


def test_classify_distribution_assets_rebuilds_when_release_has_no_core_assets() -> None:
    payload = {"assets": [{"name": "dcc_mcp_server-0.20.8-py3-none-any.whl"}]}

    assert classify_distribution_assets(payload, VERSION) == "rebuild"


def test_classify_distribution_assets_rejects_partial_core_asset_sets() -> None:
    payload = {"assets": [{"name": WHEEL_NAMES[0]}]}

    with pytest.raises(DistributionSetError, match="partial Core distribution set"):
        classify_distribution_assets(payload, VERSION)


def test_classify_distribution_assets_reuses_complete_core_asset_sets() -> None:
    payload = {"assets": [{"name": name} for name in [*WHEEL_NAMES, SDIST_NAME]]}

    assert classify_distribution_assets(payload, VERSION) == "reuse"


@pytest.mark.parametrize("requirement", ["typing-extensions==4.7.1", "typing.extensions==4.7.1"])
def test_sdist_rejects_typing_extensions_runtime_dependency(tmp_path: Path, requirement: str) -> None:
    sdist = tmp_path / SDIST_NAME
    pkg_info = _metadata_bytes("dcc-mcp-core", VERSION, (requirement,))
    with tarfile.open(sdist, "w:gz") as archive:
        member = tarfile.TarInfo(f"dcc_mcp_core-{VERSION}/PKG-INFO")
        member.size = len(pkg_info)
        archive.addfile(member, io.BytesIO(pkg_info))

    with pytest.raises(DistributionSetError, match="forbidden runtime dependency 'typing-extensions'"):
        _validate_sdist(
            sdist,
            VERSION,
            {
                "distributions": {
                    "dcc-mcp-core": {
                        "forbidden_runtime_dependencies": [{"name": "typing-extensions", "from_version": "0.20.0"}]
                    }
                }
            },
        )


@pytest.mark.parametrize(
    "member_name",
    [
        "dcc_mcp_core-1.0.0/typing_extensions.py",
        "dcc_mcp_core-1.0.0/./Typing-Extensions.py",
        "dcc_mcp_core-1.0.0/vendor/typing_extensions/__init__.py",
        "dcc_mcp_core-1.0.0/typing_extensions-4.12.2.dist-info/METADATA",
        "dcc_mcp_core-1.0.0/typing_extensions.py.",
        "dcc_mcp_core-1.0.0/typing_extensions.py ",
        "dcc_mcp_core-1.0.0/typing\uff3fextensions.py",
        "dcc_mcp_core-1.0.0/TYPING_EXTENSIONS.PY",
    ],
)
def test_sdist_rejects_normalized_typing_extensions_payloads(tmp_path: Path, member_name: str) -> None:
    sdist = tmp_path / "dcc_mcp_core-1.0.0.tar.gz"
    pkg_info = b"Metadata-Version: 2.1\nName: dcc-mcp-core\nVersion: 1.0.0\nRequires-Python: >=3.7\n"
    with tarfile.open(sdist, "w:gz") as archive:
        metadata = tarfile.TarInfo("dcc_mcp_core-1.0.0/PKG-INFO")
        metadata.size = len(pkg_info)
        archive.addfile(metadata, io.BytesIO(pkg_info))
        injected = tarfile.TarInfo(member_name)
        injected.size = len(b"injected")
        archive.addfile(injected, io.BytesIO(b"injected"))

    with pytest.raises(DistributionSetError, match="typing_extensions payload"):
        _validate_sdist(sdist, "1.0.0", check_release_distribution_set.load_contract())


@pytest.mark.parametrize(
    "member_name",
    [
        "dcc_mcp_core-1.0.0/CON.py",
        "dcc_mcp_core-1.0.0/payload.py:typing_extensions.py",
        "dcc_mcp_core-1.0.0/trailing. /payload.py",
        "dcc_mcp_core-1.0.0//payload.py",
    ],
)
def test_sdist_rejects_windows_unsafe_aliases(tmp_path: Path, member_name: str) -> None:
    sdist = tmp_path / "dcc_mcp_core-1.0.0.tar.gz"
    pkg_info = _metadata_bytes("dcc-mcp-core", "1.0.0")
    with tarfile.open(sdist, "w:gz") as archive:
        metadata = tarfile.TarInfo("dcc_mcp_core-1.0.0/PKG-INFO")
        metadata.size = len(pkg_info)
        archive.addfile(metadata, io.BytesIO(pkg_info))
        injected = tarfile.TarInfo(member_name)
        injected.size = len(b"injected")
        archive.addfile(injected, io.BytesIO(b"injected"))

    with pytest.raises(DistributionSetError, match="unsafe archive member"):
        _validate_sdist(sdist, "1.0.0", check_release_distribution_set.load_contract())


def test_sdist_rejects_duplicate_portable_member_paths(tmp_path: Path) -> None:
    sdist = tmp_path / "dcc_mcp_core-1.0.0.tar.gz"
    pkg_info = _metadata_bytes("dcc-mcp-core", "1.0.0")
    with tarfile.open(sdist, "w:gz") as archive:
        metadata = tarfile.TarInfo("dcc_mcp_core-1.0.0/PKG-INFO")
        metadata.size = len(pkg_info)
        archive.addfile(metadata, io.BytesIO(pkg_info))
        for name in ("dcc_mcp_core-1.0.0/duplicate.py", "dcc_mcp_core-1.0.0/./duplicate.py"):
            member = tarfile.TarInfo(name)
            member.size = 1
            archive.addfile(member, io.BytesIO(b"x"))

    with pytest.raises(DistributionSetError, match="duplicate"):
        _validate_sdist(sdist, "1.0.0", check_release_distribution_set.load_contract())


@pytest.mark.parametrize("link_type", [tarfile.SYMTYPE, tarfile.LNKTYPE])
def test_sdist_rejects_links_and_traversal_targets(tmp_path: Path, link_type: bytes) -> None:
    sdist = tmp_path / "dcc_mcp_core-1.0.0.tar.gz"
    pkg_info = _metadata_bytes("dcc-mcp-core", "1.0.0")
    with tarfile.open(sdist, "w:gz") as archive:
        metadata = tarfile.TarInfo("dcc_mcp_core-1.0.0/PKG-INFO")
        metadata.size = len(pkg_info)
        archive.addfile(metadata, io.BytesIO(pkg_info))
        link = tarfile.TarInfo("dcc_mcp_core-1.0.0/pkg/link.py")
        link.type = link_type
        link.linkname = "../../typing_extensions.py"
        archive.addfile(link)

    with pytest.raises(DistributionSetError, match="link"):
        _validate_sdist(sdist, "1.0.0", check_release_distribution_set.load_contract())


def test_sdist_rejects_special_device_members(tmp_path: Path) -> None:
    sdist = tmp_path / "dcc_mcp_core-1.0.0.tar.gz"
    pkg_info = _metadata_bytes("dcc-mcp-core", "1.0.0")
    with tarfile.open(sdist, "w:gz") as archive:
        metadata = tarfile.TarInfo("dcc_mcp_core-1.0.0/PKG-INFO")
        metadata.size = len(pkg_info)
        archive.addfile(metadata, io.BytesIO(pkg_info))
        fifo = tarfile.TarInfo("dcc_mcp_core-1.0.0/pkg/channel")
        fifo.type = tarfile.FIFOTYPE
        archive.addfile(fifo)

    with pytest.raises(DistributionSetError, match="special"):
        _validate_sdist(sdist, "1.0.0", check_release_distribution_set.load_contract())


def _metadata_bytes(name: str, version: str, requirements: tuple[str, ...] = ()) -> bytes:
    message = Message()
    message["Metadata-Version"] = "2.1"
    message["Name"] = name
    message["Version"] = version
    for requirement in requirements:
        message["Requires-Dist"] = requirement
    buffer = io.BytesIO()
    BytesGenerator(buffer).flatten(message)
    return buffer.getvalue()


def _write_fixture_set(dist_dir: Path) -> None:
    dist_dir.mkdir()
    for name in WHEEL_NAMES:
        with zipfile.ZipFile(dist_dir / name, "w") as archive:
            archive.writestr(
                f"dcc_mcp_core-{VERSION}.dist-info/METADATA",
                _metadata_bytes("dcc-mcp-core", VERSION),
            )
    pkg_info = _metadata_bytes("dcc-mcp-core", VERSION)
    with tarfile.open(dist_dir / SDIST_NAME, "w:gz") as archive:
        member = tarfile.TarInfo(f"dcc_mcp_core-{VERSION}/PKG-INFO")
        member.size = len(pkg_info)
        archive.addfile(member, io.BytesIO(pkg_info))


def _asset_payload(dist_dir: Path) -> dict:
    assets = []
    for path in sorted(dist_dir.iterdir()):
        data = path.read_bytes()
        assets.append(
            {
                "name": path.name,
                "size": len(data),
                "digest": f"sha256:{hashlib.sha256(data).hexdigest()}",
            }
        )
    return {"assets": assets}


def test_verify_distribution_set_requires_all_seven_contracts(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    dist_dir = tmp_path / "dist"
    _write_fixture_set(dist_dir)
    payload = _asset_payload(dist_dir)
    monkeypatch.setattr(check_release_distribution_set, "validate_wheel", lambda *_args: [])
    monkeypatch.setattr(check_release_distribution_set, "load_contract", lambda: {})

    evidence = verify_distribution_set(payload, dist_dir, VERSION)

    assert evidence == {"asset_count": 7, "total_bytes": sum(path.stat().st_size for path in dist_dir.iterdir())}


def test_verify_distribution_set_rejects_incomplete_release(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    dist_dir = tmp_path / "dist"
    _write_fixture_set(dist_dir)
    missing = dist_dir / WHEEL_NAMES[0]
    missing.unlink()
    payload = _asset_payload(dist_dir)
    monkeypatch.setattr(check_release_distribution_set, "validate_wheel", lambda *_args: [])
    monkeypatch.setattr(check_release_distribution_set, "load_contract", lambda: {})

    with pytest.raises(DistributionSetError, match="missing wheel contract"):
        verify_distribution_set(payload, dist_dir, VERSION)


def test_verify_distribution_set_rejects_digest_drift(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    dist_dir = tmp_path / "dist"
    _write_fixture_set(dist_dir)
    payload = _asset_payload(dist_dir)
    payload["assets"][0]["digest"] = f"sha256:{'0' * 64}"
    monkeypatch.setattr(check_release_distribution_set, "validate_wheel", lambda *_args: [])
    monkeypatch.setattr(check_release_distribution_set, "load_contract", lambda: {})

    with pytest.raises(DistributionSetError, match="digest mismatch"):
        verify_distribution_set(payload, dist_dir, VERSION)


def test_verify_distribution_set_runs_existing_wheel_contract(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    dist_dir = tmp_path / "dist"
    _write_fixture_set(dist_dir)
    payload = _asset_payload(dist_dir)
    monkeypatch.setattr(check_release_distribution_set, "load_contract", lambda: {})
    monkeypatch.setattr(
        check_release_distribution_set,
        "validate_wheel",
        lambda *_args: ["synthetic contract drift"],
    )

    with pytest.raises(DistributionSetError, match="synthetic contract drift"):
        verify_distribution_set(payload, dist_dir, VERSION)


def test_release_distribution_gate_rejects_injected_wheel_payload_before_contract_delegate(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    dist_dir = tmp_path / "dist"
    _write_fixture_set(dist_dir)
    with zipfile.ZipFile(dist_dir / WHEEL_NAMES[-1], "a") as archive:
        archive.writestr("typing_extensions.py", b"injected")
    payload = _asset_payload(dist_dir)
    monkeypatch.setattr(check_release_distribution_set, "validate_wheel", lambda *_args: [])
    monkeypatch.setattr(check_release_distribution_set, "load_contract", lambda: {})

    with pytest.raises(DistributionSetError, match="typing_extensions payload"):
        verify_distribution_set(payload, dist_dir, VERSION)


def test_release_distribution_gate_rejects_wheel_symlink(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    dist_dir = tmp_path / "dist"
    _write_fixture_set(dist_dir)
    link = zipfile.ZipInfo("dcc_mcp_core/link.py")
    link.create_system = 3
    link.external_attr = 0o120777 << 16
    with zipfile.ZipFile(dist_dir / WHEEL_NAMES[-1], "a") as archive:
        archive.writestr(link, b"../../typing_extensions.py")
    payload = _asset_payload(dist_dir)
    monkeypatch.setattr(check_release_distribution_set, "validate_wheel", lambda *_args: [])
    monkeypatch.setattr(check_release_distribution_set, "load_contract", lambda: {})

    with pytest.raises(DistributionSetError, match="symlink"):
        verify_distribution_set(payload, dist_dir, VERSION)


def test_release_distribution_gate_rejects_sdist_hardlink(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    dist_dir = tmp_path / "dist"
    _write_fixture_set(dist_dir)
    original = dist_dir / SDIST_NAME
    replacement = dist_dir / f"{SDIST_NAME}.new"
    with tarfile.open(original, "r:gz") as source, tarfile.open(replacement, "w:gz") as target:
        for member in source.getmembers():
            stream = source.extractfile(member) if member.isfile() else None
            target.addfile(member, stream)
        link = tarfile.TarInfo(f"dcc_mcp_core-{VERSION}/pkg/hard.py")
        link.type = tarfile.LNKTYPE
        link.linkname = "../../typing_extensions.py"
        target.addfile(link)
    replacement.replace(original)
    payload = _asset_payload(dist_dir)
    monkeypatch.setattr(check_release_distribution_set, "validate_wheel", lambda *_args: [])
    monkeypatch.setattr(check_release_distribution_set, "load_contract", lambda: {})

    with pytest.raises(DistributionSetError, match="link"):
        verify_distribution_set(payload, dist_dir, VERSION)


def test_cli_reads_release_asset_json(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    dist_dir = tmp_path / "dist"
    _write_fixture_set(dist_dir)
    assets_json = tmp_path / "assets.json"
    assets_json.write_text(json.dumps(_asset_payload(dist_dir)), encoding="utf-8")
    monkeypatch.setattr(check_release_distribution_set, "validate_wheel", lambda *_args: [])
    monkeypatch.setattr(check_release_distribution_set, "load_contract", lambda: {})

    assert (
        check_release_distribution_set.main(
            ["--assets-json", str(assets_json), "--dist-dir", str(dist_dir), "--version", VERSION]
        )
        == 0
    )


def test_cli_validates_fresh_local_distribution_set_without_release_assets(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    dist_dir = tmp_path / "dist"
    _write_fixture_set(dist_dir)
    monkeypatch.setattr(check_release_distribution_set, "validate_wheel", lambda *_args: [])
    monkeypatch.setattr(check_release_distribution_set, "load_contract", lambda: {})

    assert check_release_distribution_set.main(["--dist-dir", str(dist_dir), "--version", VERSION]) == 0


def test_cli_classifies_missing_assets_for_github_actions(tmp_path: Path) -> None:
    assets_json = tmp_path / "assets.json"
    assets_json.write_text(json.dumps({"assets": []}), encoding="utf-8")
    github_output = tmp_path / "github-output.txt"

    assert (
        check_release_distribution_set.main(
            [
                "--assets-json",
                str(assets_json),
                "--version",
                VERSION,
                "--classify-assets",
                "--github-output",
                str(github_output),
            ]
        )
        == 0
    )
    assert github_output.read_text(encoding="utf-8") == "mode=rebuild\n"
