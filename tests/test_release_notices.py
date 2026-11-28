from __future__ import annotations

from pathlib import Path
import tarfile
from zipfile import ZIP_DEFLATED
from zipfile import ZipFile

from scripts.ci.check_release_notices import _expected_files
from scripts.ci.check_release_notices import validate_archive
from scripts.release.stage_package_notices import NOTICE_FILES
from scripts.release.stage_package_notices import PACKAGE_ROOTS
from scripts.release.stage_package_notices import stage


def _write_repository_notices(root: Path) -> None:
    (root / "LICENSE").write_text("license\n", encoding="utf-8")
    (root / "NOTICE").write_text("notice\n", encoding="utf-8")
    (root / "THIRD_PARTY_NOTICES.txt").write_text("third-party\n", encoding="utf-8")
    (root / "THIRD_PARTY_LICENSES.txt").write_text("license texts\n", encoding="utf-8")


def _write_zip(path: Path, root: Path, *, alter_notice: bool = False) -> None:
    with ZipFile(path, "w", ZIP_DEFLATED) as archive:
        for name in ("LICENSE", "NOTICE", "THIRD_PARTY_NOTICES.txt", "THIRD_PARTY_LICENSES.txt"):
            content = (root / name).read_bytes()
            if alter_notice and name == "NOTICE":
                content = b"changed\n"
            archive.writestr(name, content)


def _write_partial_zip(path: Path, root: Path, names: tuple[str, ...]) -> None:
    with ZipFile(path, "w", ZIP_DEFLATED) as archive:
        for name in names:
            archive.writestr(name, (root / name).read_bytes())


def _write_sdist(path: Path, root: Path) -> None:
    package_root = "sample-1.0.0"
    with tarfile.open(path, "w:gz") as archive:
        for name in ("LICENSE", "NOTICE", "THIRD_PARTY_NOTICES.txt", "THIRD_PARTY_LICENSES.txt"):
            source = root / name
            info = tarfile.TarInfo(f"{package_root}/{name}")
            info.size = source.stat().st_size
            with source.open("rb") as stream:
                archive.addfile(info, stream)


def test_release_notice_gate_accepts_zip_and_sdist(tmp_path: Path) -> None:
    _write_repository_notices(tmp_path)
    expected = _expected_files(tmp_path)
    zip_path = tmp_path / "sample.whl"
    sdist_path = tmp_path / "sample.tar.gz"
    _write_zip(zip_path, tmp_path)
    _write_sdist(sdist_path, tmp_path)

    assert validate_archive(zip_path, expected) == []
    assert validate_archive(sdist_path, expected) == []


def test_release_notice_gate_rejects_changed_notice(tmp_path: Path) -> None:
    _write_repository_notices(tmp_path)
    archive = tmp_path / "sample.zip"
    _write_zip(archive, tmp_path, alter_notice=True)

    errors = validate_archive(archive, _expected_files(tmp_path))

    assert errors == ["sample.zip: NOTICE does not match the repository notice in ['NOTICE']"]


def test_release_notice_gate_accepts_maturin_wheel_contract(tmp_path: Path) -> None:
    _write_repository_notices(tmp_path)
    archive = tmp_path / "dcc_mcp_core-0.22.0-cp38-abi3-win_amd64.whl"
    _write_partial_zip(archive, tmp_path, ("THIRD_PARTY_NOTICES.txt", "THIRD_PARTY_LICENSES.txt"))

    assert validate_archive(archive, _expected_files(tmp_path)) == []


def test_release_notice_gate_accepts_core_sdist_contract(tmp_path: Path) -> None:
    _write_repository_notices(tmp_path)
    archive = tmp_path / "dcc_mcp_core-0.22.0.tar.gz"
    package_root = "dcc_mcp_core-0.22.0"
    with tarfile.open(archive, "w:gz") as output:
        for name in ("LICENSE", "NOTICE", "THIRD_PARTY_NOTICES.txt"):
            source = tmp_path / name
            info = tarfile.TarInfo(f"{package_root}/{name}")
            info.size = source.stat().st_size
            with source.open("rb") as stream:
                output.addfile(info, stream)

    assert validate_archive(archive, _expected_files(tmp_path)) == []


def test_release_notice_gate_rejects_missing_maturin_notice(tmp_path: Path) -> None:
    _write_repository_notices(tmp_path)
    archive = tmp_path / "dcc_mcp_server-0.22.0-py3-none-win_amd64.whl"
    _write_partial_zip(archive, tmp_path, ("THIRD_PARTY_NOTICES.txt",))

    errors = validate_archive(archive, _expected_files(tmp_path))

    assert errors == [f"{archive.name}: missing THIRD_PARTY_LICENSES.txt"]


def test_stage_package_notices_copies_generated_package_resources(tmp_path: Path) -> None:
    _write_repository_notices(tmp_path)

    stage(tmp_path)

    for package_root in PACKAGE_ROOTS:
        for name in NOTICE_FILES:
            assert (tmp_path / package_root / name).read_bytes() == (tmp_path / name).read_bytes()


def test_core_maturin_config_includes_generated_package_notices() -> None:
    root = Path(__file__).resolve().parents[1]
    config = (root / "pyproject.toml").read_text(encoding="utf-8")

    assert (
        "include = [\n"
        '    "python/dcc_mcp_core/THIRD_PARTY_NOTICES.txt",\n'
        '    "python/dcc_mcp_core/THIRD_PARTY_LICENSES.txt",\n'
        "]"
    ) in config
