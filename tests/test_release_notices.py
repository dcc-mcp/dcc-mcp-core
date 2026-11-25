from __future__ import annotations

from pathlib import Path
import tarfile
from zipfile import ZIP_DEFLATED
from zipfile import ZipFile

from scripts.ci.check_release_notices import _expected_files
from scripts.ci.check_release_notices import validate_archive
from scripts.release.stage_package_notices import PACKAGE_ROOTS
from scripts.release.stage_package_notices import stage


def _write_repository_notices(root: Path) -> None:
    (root / "LICENSE").write_text("license\n", encoding="utf-8")
    (root / "NOTICE").write_text("notice\n", encoding="utf-8")
    (root / "THIRD_PARTY_NOTICES.txt").write_text("third-party\n", encoding="utf-8")


def _write_zip(path: Path, root: Path, *, alter_notice: bool = False) -> None:
    with ZipFile(path, "w", ZIP_DEFLATED) as archive:
        for name in ("LICENSE", "NOTICE", "THIRD_PARTY_NOTICES.txt"):
            content = (root / name).read_bytes()
            if alter_notice and name == "NOTICE":
                content = b"changed\n"
            archive.writestr(name, content)


def _write_sdist(path: Path, root: Path) -> None:
    package_root = "sample-1.0.0"
    with tarfile.open(path, "w:gz") as archive:
        for name in ("LICENSE", "NOTICE", "THIRD_PARTY_NOTICES.txt"):
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


def test_stage_package_notices_copies_root_files(tmp_path: Path) -> None:
    _write_repository_notices(tmp_path)

    stage(tmp_path)

    for package_root in PACKAGE_ROOTS:
        for name in ("LICENSE", "NOTICE", "THIRD_PARTY_NOTICES.txt"):
            assert (tmp_path / package_root / name).read_bytes() == (tmp_path / name).read_bytes()
