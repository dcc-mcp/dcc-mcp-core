from __future__ import annotations

from pathlib import Path
import subprocess
import zipfile

from scripts.release import generate_third_party_licenses as generator
from scripts.release.extract_third_party_licenses import extract


def test_rust_metadata_is_decoded_as_utf8(tmp_path: Path, monkeypatch) -> None:
    captured = {}

    def fake_run(*args, **kwargs):
        captured.update(kwargs)
        return subprocess.CompletedProcess(args[0], 0, '{"packages": []}', "")

    monkeypatch.setattr(generator.subprocess, "run", fake_run)

    assert generator._rust_packages(tmp_path) == []
    assert captured["encoding"] == "utf-8"


def test_generate_uses_vendored_spdx_text_when_package_has_no_license_file(tmp_path: Path, monkeypatch) -> None:
    package_root = tmp_path / "crate"
    package_root.mkdir()
    (package_root / "Cargo.toml").write_text("[package]\n", encoding="utf-8")
    (tmp_path / "licenses" / "spdx").mkdir(parents=True)
    (tmp_path / "licenses" / "spdx" / "MIT.txt").write_text("MIT text\n", encoding="utf-8")
    packages = [
        {
            "name": "fixture",
            "version": "1.0.0",
            "license": "MIT",
            "manifest_path": str(package_root / "Cargo.toml"),
        }
    ]
    monkeypatch.setattr(generator, "_rust_packages", lambda root: packages)

    output = generator.generate(tmp_path)

    assert "fixture 1.0.0" in output
    assert "Text source: SPDX" in output
    assert "MIT text" in output


def test_extract_requires_identical_license_bundles(tmp_path: Path) -> None:
    first = tmp_path / "first.zip"
    second = tmp_path / "second.zip"
    for path in (first, second):
        with zipfile.ZipFile(path, "w") as archive:
            archive.writestr("THIRD_PARTY_LICENSES.txt", "same\n")

    output = tmp_path / "THIRD_PARTY_LICENSES.txt"
    extract([first, second], output)

    assert output.read_text(encoding="utf-8") == "same\n"
