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


def _metadata_bytes(name: str, version: str) -> bytes:
    message = Message()
    message["Metadata-Version"] = "2.1"
    message["Name"] = name
    message["Version"] = version
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
