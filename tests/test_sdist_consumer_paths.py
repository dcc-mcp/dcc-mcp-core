"""Source archive names must match the paths package consumers actually open."""

from __future__ import annotations

import copy
import inspect
from pathlib import Path
import tarfile

import pytest

from test_archive_publication_policy import _SDIST_ROOT
from test_archive_publication_policy import _publication_cli
from test_archive_publication_policy import _write_distribution_set
from test_archive_publication_policy import _zip_entry


def _rename_member(source: Path, destination: Path, old: str, new: str) -> None:
    with tarfile.open(source) as original, tarfile.open(destination, "w:gz") as mutated:
        for member in original:
            renamed = copy.copy(member)
            if member.name == old:
                renamed.name = new
            mutated.addfile(renamed, original.extractfile(member) if member.isfile() else None)


def test_publication_rejects_nfkc_split_sdist_project_root(tmp_path: Path) -> None:
    directory = tmp_path / "dist"
    artifact = _write_distribution_set(
        directory,
        "sdist",
        [_zip_entry("pyproject.toml", b'[build-system]\nrequires = ["maturin"]\nbuild-backend = "maturin"\n')],
    )
    result = _publication_cli(directory)
    assert result.returncode == 0, (result.stdout, result.stderr)
    original = tmp_path / "original.tar.gz"
    artifact.rename(original)
    _rename_member(
        original,
        artifact,
        f"{_SDIST_ROOT}/pyproject.toml",
        f"{_SDIST_ROOT.replace('_', chr(0xFF3F), 1)}/pyproject.toml",
    )
    result = _publication_cli(directory)
    assert result.returncode == 1, (result.stdout, result.stderr)
    assert "paths outside" in result.stderr


@pytest.mark.parametrize(
    "replacement",
    [
        f"{_SDIST_ROOT.replace('_', chr(0xFF3F), 1)}/PKG-INFO",
        f"{_SDIST_ROOT}/\uff30KG-INFO",
        f"{_SDIST_ROOT}/PKG-\uff29NFO",
        f"{_SDIST_ROOT}/pkg-info",
    ],
)
def test_publication_requires_literal_root_metadata(tmp_path: Path, replacement: str) -> None:
    directory = tmp_path / "dist"
    artifact = _write_distribution_set(directory, "sdist")
    original = tmp_path / "original.tar.gz"
    artifact.rename(original)
    _rename_member(original, artifact, f"{_SDIST_ROOT}/PKG-INFO", replacement)
    result = _publication_cli(directory)
    assert result.returncode == 1, (result.stdout, result.stderr)
    assert "exactly one" in result.stderr


def test_sdist_portable_alias_checks_still_reject_duplicate_metadata(tmp_path: Path) -> None:
    directory = tmp_path / "dist"
    _write_distribution_set(directory, "sdist", [_zip_entry("\uff30KG-INFO")])
    result = _publication_cli(directory)
    assert result.returncode == 1, (result.stdout, result.stderr)
    assert "duplicate portable member paths" in result.stderr


def test_sdist_pip_discovery_uses_literal_project_paths(tmp_path: Path) -> None:
    unpacking = pytest.importorskip("pip._internal.utils.unpacking")
    pyproject = pytest.importorskip("pip._internal.pyproject")
    exceptions = pytest.importorskip("pip._internal.exceptions")
    directory = tmp_path / "dist"
    artifact = _write_distribution_set(
        directory,
        "sdist",
        [_zip_entry("pyproject.toml", b'[build-system]\nrequires = ["maturin"]\nbuild-backend = "maturin"\n')],
    )
    split = tmp_path / "split.tar.gz"
    _rename_member(
        artifact, split, f"{_SDIST_ROOT}/pyproject.toml", f"{_SDIST_ROOT.replace('_', chr(0xFF3F), 1)}/pyproject.toml"
    )
    for source, valid in [(artifact, True), (split, False)]:
        target = tmp_path / ("original-unpack" if valid else "split-unpack")
        target.mkdir()
        unpacking.unpack_file(str(source), str(target))
        arguments = [str(target / "pyproject.toml"), str(target / "setup.py"), "fixture"]
        # pip 25 and 26 expose different signatures for the same discovery step.
        if "use_pep517" in inspect.signature(pyproject.load_pyproject_toml).parameters:
            arguments.insert(0, None)
        if valid:
            assert pyproject.load_pyproject_toml(*arguments).backend == "maturin"
            assert (target / "PKG-INFO").is_file()
        else:
            assert not (target / "pyproject.toml").exists()
            with pytest.raises(exceptions.InstallationError, match=r"neither 'setup\.py' nor 'pyproject\.toml'"):
                pyproject.load_pyproject_toml(*arguments)


def test_sdist_keeps_unicode_content_under_literal_root(tmp_path: Path) -> None:
    directory = tmp_path / "dist"
    artifact = _write_distribution_set(directory, "sdist", [_zip_entry("docs/\uff41nnotations.txt", b"content")])
    result = _publication_cli(directory)
    assert result.returncode == 0, (result.stdout, result.stderr)
    with tarfile.open(artifact) as archive:
        assert archive.extractfile(f"{_SDIST_ROOT}/docs/\uff41nnotations.txt").read() == b"content"
