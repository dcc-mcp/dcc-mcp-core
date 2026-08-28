"""Real archive regressions through the unmodified prepublication CLI.

Native fixture entries are inert placeholders for archive validation, not
evidence of a working native extension. Runtime smoke uses built wheels.
"""

from __future__ import annotations

import io
import os
from pathlib import Path
import py_compile
import stat
import subprocess
import sys
import tarfile
import zipfile

import pytest
from scripts.ci.check_python_wheel import validate_wheel
from scripts.ci.check_release_distribution_set import DistributionSetError
from scripts.ci.check_release_distribution_set import _validate_sdist
from scripts.ci.python_support_contract import load_contract
from scripts.ci.smoke_zero_typing_extensions import _check_wheel_archive

from test_python_wheel_contract import _write_wheel

_ROOT = Path(__file__).resolve().parents[1]
_VERSION = "0.20.22"
_SDIST_ROOT = f"dcc_mcp_core-{_VERSION}"
_WHEEL_TAGS = (
    "cp37-cp37m-manylinux_2_17_x86_64.manylinux2014_x86_64",
    "cp37-cp37m-win_amd64",
    "cp38-abi3-manylinux_2_17_x86_64.manylinux2014_x86_64",
    "cp38-abi3-win_amd64",
    "cp38-abi3-macosx_10_12_universal2",
    "py3-none-any",
)


def _zip_entry(name: str, data: bytes = b"fixture\n", directory: bool = False):
    member = zipfile.ZipInfo(name)
    member.create_system = 3
    member.external_attr = ((stat.S_IFDIR if directory else stat.S_IFREG) | 0o755) << 16
    if directory:
        member.external_attr |= 0x10
    return member, b"" if directory else data


def _write_sdist(path: Path, entries=(), requirement: str = "") -> None:
    metadata = (f"Metadata-Version: 2.1\nName: dcc-mcp-core\nVersion: {_VERSION}\nRequires-Python: >=3.7\n").encode()
    if requirement:
        metadata += f"Requires-Dist: {requirement}\n".encode()
    with tarfile.open(path, "w:gz") as archive:
        for item, data in [_zip_entry("PKG-INFO", metadata), *entries]:
            member = tarfile.TarInfo(f"{_SDIST_ROOT}/{item.filename}")
            member.type = tarfile.DIRTYPE if item.is_dir() else tarfile.REGTYPE
            member.mode = stat.S_IMODE(item.external_attr >> 16)
            member.size = len(data)
            archive.addfile(member, None if member.isdir() else io.BytesIO(data))


def test_sdist_fixture_preserves_declared_file_and_directory_permissions(tmp_path: Path) -> None:
    directory, empty = _zip_entry("pkg/resources/", directory=True)
    resource, data = _zip_entry("pkg/resources/manifest.json")
    directory.external_attr = (stat.S_IFDIR | 0o750) << 16
    resource.external_attr = (stat.S_IFREG | 0o640) << 16
    sdist = tmp_path / "fixture.tar.gz"
    _write_sdist(sdist, [(directory, empty), (resource, data)])
    with tarfile.open(sdist) as archive:
        assert archive.getmember(f"{_SDIST_ROOT}/pkg/resources").mode == 0o750
        assert archive.getmember(f"{_SDIST_ROOT}/pkg/resources/manifest.json").mode == 0o640


def _write_distribution_set(directory: Path, target: str, entries=(), requirement: str = "") -> Path:
    directory.mkdir()
    for tag in _WHEEL_TAGS:
        pure = tag == "py3-none-any"
        wheel = directory / f"{_SDIST_ROOT}-{tag}.whl"
        _write_wheel(
            wheel,
            pure=pure,
            with_core=not pure,
            version=_VERSION,
            requires_dist=[requirement] if pure and target == "wheel" and requirement else None,
        )
        if pure and target == "wheel":
            with zipfile.ZipFile(wheel, "a") as archive:
                for item, data in entries:
                    archive.writestr(item, data)
    sdist = directory / f"{_SDIST_ROOT}.tar.gz"
    _write_sdist(sdist, entries if target == "sdist" else (), requirement if target == "sdist" else "")
    return sdist if target == "sdist" else directory / f"{_SDIST_ROOT}-py3-none-any.whl"


def _publication_cli(directory: Path):
    return subprocess.run(
        [
            sys.executable,
            str(_ROOT / "scripts/ci/check_release_distribution_set.py"),
            "--dist-dir",
            str(directory),
            "--version",
            _VERSION,
        ],
        cwd=_ROOT,
        capture_output=True,
        text=True,
        timeout=30,
    )


def _assert_consumers_reject(tmp_path: Path, target: str, entries, reason: str) -> None:
    directory = tmp_path / "dist"
    artifact = _write_distribution_set(directory, target, entries)
    # Run the exact CLI used before publishing, with all six wheel contracts
    # and the sdist present. No validator or contract loader is patched.
    result = _publication_cli(directory)
    assert result.returncode == 1, (result.stdout, result.stderr)
    assert reason in result.stderr
    if target == "wheel":
        errors = validate_wheel(artifact, "lite_py37", "any", load_contract(_ROOT))
        assert any(reason in error for error in errors), errors
        with pytest.raises(RuntimeError, match=reason):
            _check_wheel_archive(artifact, tmp_path / "blocked-extraction")
        assert not (tmp_path / "blocked-extraction").exists()
    else:
        with pytest.raises(DistributionSetError, match=reason):
            _validate_sdist(artifact, _VERSION, load_contract(_ROOT))


@pytest.mark.parametrize("target", ["wheel", "sdist"])
def test_complete_prepublication_cli_accepts_clean_set(tmp_path: Path, target: str) -> None:
    _write_distribution_set(tmp_path / "dist", target)
    result = _publication_cli(tmp_path / "dist")
    assert result.returncode == 0, (result.stdout, result.stderr)
    assert "Validated 7 fresh Core distribution(s)" in result.stdout


@pytest.mark.parametrize("target", ["wheel", "sdist"])
def test_quoted_extra_text_cannot_hide_default_dependency(tmp_path: Path, target: str) -> None:
    requirement = 'typing-extensions==4.7.1; python_version < "3.8" and os_name != "extra == \'test\'"'
    directory = tmp_path / "dist"
    artifact = _write_distribution_set(directory, target, requirement=requirement)
    result = _publication_cli(directory)
    assert result.returncode == 1, (result.stdout, result.stderr)
    assert "forbidden runtime dependency" in result.stderr
    if target == "wheel":
        errors = validate_wheel(artifact, "lite_py37", "any", load_contract(_ROOT))
        assert any("forbidden runtime dependency" in error for error in errors), errors
        with pytest.raises(RuntimeError, match="metadata contains typing-extensions"):
            _check_wheel_archive(artifact, tmp_path / "blocked")
        assert not (tmp_path / "blocked").exists()
    else:
        with pytest.raises(DistributionSetError, match="forbidden runtime dependency"):
            _validate_sdist(artifact, _VERSION, load_contract(_ROOT))


@pytest.mark.parametrize("target", ["wheel", "sdist"])
@pytest.mark.parametrize(
    "marker",
    [
        "python_version < '3.8' and extra == 'test'",
        "(extra == 'test' or 'dev' == extra) and os_name != \"extra == 'other'\"",
    ],
)
def test_real_extra_only_requirements_remain_publishable(tmp_path: Path, target: str, marker: str) -> None:
    directory = tmp_path / "dist"
    artifact = _write_distribution_set(directory, target, requirement="typing-extensions==4.7.1; " + marker)
    result = _publication_cli(directory)
    assert result.returncode == 0, (result.stdout, result.stderr)
    if target == "wheel":
        assert validate_wheel(artifact, "lite_py37", "any", load_contract(_ROOT)) == []
        _check_wheel_archive(artifact, tmp_path / "extracted")
    else:
        _validate_sdist(artifact, _VERSION, load_contract(_ROOT))


@pytest.mark.parametrize("target", ["wheel", "sdist"])
@pytest.mark.parametrize("character", list('?*<>|"'))
def test_all_consumers_reject_windows_extraction_aliases(tmp_path: Path, target: str, character: str) -> None:
    _assert_consumers_reject(tmp_path, target, [_zip_entry(f"typing{character}extensions.py")], "unsafe archive member")


@pytest.mark.parametrize("kind", [stat.S_IFIFO, stat.S_IFCHR, stat.S_IFBLK, stat.S_IFSOCK, 0o150000])
def test_all_wheel_consumers_reject_unix_special_member_types(tmp_path: Path, kind: int) -> None:
    member, data = _zip_entry("dcc_mcp_core/special")
    member.external_attr = (kind | 0o600) << 16
    _assert_consumers_reject(tmp_path, "wheel", [(member, data)], "forbidden special member")
    with zipfile.ZipFile(tmp_path / "dist" / f"{_SDIST_ROOT}-py3-none-any.whl") as archive:
        assert stat.S_IFMT(archive.getinfo(member.filename).external_attr >> 16) == kind


@pytest.mark.parametrize("kind", [0, stat.S_IFREG])
@pytest.mark.parametrize("reverse", [False, True])
def test_regular_and_permission_only_zip_modes_remain_publishable(tmp_path: Path, kind: int, reverse: bool) -> None:
    member, data = _zip_entry("pkg/resources/file.json")
    member.external_attr = (kind | 0o644) << 16
    entries = [_zip_entry("pkg/resources/", directory=True), (member, data)]
    if reverse:
        entries.reverse()
    artifact = _write_distribution_set(tmp_path / "dist", "wheel", entries)
    result = _publication_cli(tmp_path / "dist")
    assert result.returncode == 0, (result.stdout, result.stderr)
    assert validate_wheel(artifact, "lite_py37", "any", load_contract(_ROOT)) == []
    _check_wheel_archive(artifact, tmp_path / "extracted")
    assert (tmp_path / "extracted/pkg/resources/file.json").read_bytes() == data


@pytest.mark.skipif(os.name != "nt", reason="Requires real Windows ZipFile extraction semantics")
@pytest.mark.parametrize("character", list('?*<>|"'))
def test_windows_zip_alias_really_extracts_as_importable_backport_name(tmp_path: Path, character: str) -> None:
    wheel = tmp_path / "alias.zip"
    item, data = _zip_entry(f"typing{character}extensions.py", b"fixture_marker = 2388\n")
    with zipfile.ZipFile(wheel, "w") as archive:
        archive.writestr(item, data)
    extracted = tmp_path / "extracted"
    with zipfile.ZipFile(wheel) as archive:
        assert archive.infolist()[0].filename == item.filename
        archive.extractall(extracted)
    assert (extracted / "typing_extensions.py").read_bytes() == data
    result = subprocess.run(
        [
            sys.executable,
            "-I",
            "-S",
            "-c",
            "import sys; sys.path.insert(0, sys.argv[1]); import typing_extensions as t; "
            "assert t.fixture_marker == 2388; print(t.__file__)",
            str(extracted),
        ],
        capture_output=True,
        text=True,
        timeout=30,
    )
    assert result.returncode == 0, result.stderr
    assert str(extracted / "typing_extensions.py") in result.stdout


def _source_free_bytecode(tmp_path: Path) -> bytes:
    source = tmp_path / "compile-input.py"
    source.write_text("fixture_marker = 2388\n", encoding="utf-8")
    bytecode = tmp_path / "compile-output.pyc"
    py_compile.compile(str(source), cfile=str(bytecode), dfile="typing_extensions.py", doraise=True)
    return bytecode.read_bytes()


@pytest.mark.parametrize("target", ["wheel", "sdist"])
@pytest.mark.parametrize("optimization", [0, 1, 2])
def test_all_consumers_reject_cpython_tagged_backport_cache(tmp_path: Path, target: str, optimization: int) -> None:
    source = tmp_path / "typing_extensions.py"
    source.write_text("fixture_marker = 2388\n", encoding="utf-8")
    bytecode = Path(py_compile.compile(str(source), optimize=optimization, doraise=True))
    assert bytecode.parent.name == "__pycache__"
    _assert_consumers_reject(
        tmp_path,
        target,
        [_zip_entry(f"vendor/__pycache__/{bytecode.name}", bytecode.read_bytes())],
        "typing_extensions payload",
    )


@pytest.mark.parametrize("target", ["wheel", "sdist"])
@pytest.mark.parametrize("tag", ["37", "38", "39", "310", "311", "312", "313", "314"])
def test_backport_cache_tags_remain_forbidden_across_supported_cpython_versions(
    tmp_path: Path, target: str, tag: str
) -> None:
    _assert_consumers_reject(
        tmp_path,
        target,
        [_zip_entry(f"vendor/__pycache__/typing_extensions.cpython-{tag}.opt-2.pyc")],
        "typing_extensions payload",
    )


@pytest.mark.parametrize("target", ["wheel", "sdist"])
@pytest.mark.parametrize(
    "name",
    [
        "Typing.Extensions.CPYTHON-37.PYC",
        "typing\uff3fextensions.cpython-37.opt-1.pyc",
        "typing-extensions.cpython-314.opt-2.pyc",
    ],
)
def test_backport_cache_tags_preserve_portable_alias_rejection(tmp_path: Path, target: str, name: str) -> None:
    _assert_consumers_reject(tmp_path, target, [_zip_entry(f"vendor/__pycache__/{name}")], "typing_extensions payload")


@pytest.mark.parametrize("target", ["wheel", "sdist"])
@pytest.mark.parametrize("optimization", [0, 1, 2])
@pytest.mark.parametrize("module", ["typing", "typing_extensions_helpers", "my_typing_extensions"])
def test_ordinary_and_similarly_named_cpython_caches_remain_publishable(
    tmp_path: Path, target: str, optimization: int, module: str
) -> None:
    source = tmp_path / f"{module}.py"
    source.write_text('"""Fixture module."""\nfixture_debug = __debug__\n', encoding="utf-8")
    bytecode = Path(py_compile.compile(str(source), optimize=optimization, doraise=True))
    data = bytecode.read_bytes()
    name = f"vendor/__pycache__/{bytecode.name}"
    directory = tmp_path / "dist"
    artifact = _write_distribution_set(directory, target, [_zip_entry(name, data)])
    result = _publication_cli(directory)
    assert result.returncode == 0, (result.stdout, result.stderr)
    if target == "wheel":
        assert validate_wheel(artifact, "lite_py37", "any", load_contract(_ROOT)) == []
        _check_wheel_archive(artifact, tmp_path / "extracted")
        assert (tmp_path / "extracted" / name).read_bytes() == data
    else:
        _validate_sdist(artifact, _VERSION, load_contract(_ROOT))
        with tarfile.open(artifact) as archive:
            assert archive.extractfile(f"{_SDIST_ROOT}/{name}").read() == data


@pytest.mark.parametrize("target", ["wheel", "sdist"])
@pytest.mark.parametrize(
    "name",
    ["typing_extensions.cpython-37.py", "typing_extensions.cpython-37.pyc.txt", "typing_extensions_notes.pyc"],
)
def test_cache_tag_rule_does_not_match_unrelated_filename_suffixes(tmp_path: Path, target: str, name: str) -> None:
    _write_distribution_set(tmp_path / "dist", target, [_zip_entry(name)])
    result = _publication_cli(tmp_path / "dist")
    assert result.returncode == 0, (result.stdout, result.stderr)


@pytest.mark.parametrize("target", ["wheel", "sdist"])
@pytest.mark.parametrize(
    "name", ["typing_extensions.pyc", "Typing.Extensions.PYC", "vendor/typing\uff3fextensions.pyc"]
)
def test_all_consumers_reject_source_free_backport_bytecode(tmp_path: Path, target: str, name: str) -> None:
    _assert_consumers_reject(
        tmp_path, target, [_zip_entry(name, _source_free_bytecode(tmp_path))], "typing_extensions payload"
    )


def test_source_free_zip_module_is_importable_without_source_or_site_packages(tmp_path: Path) -> None:
    wheel = tmp_path / "bytecode.zip"
    item, data = _zip_entry("typing_extensions.pyc", _source_free_bytecode(tmp_path))
    with zipfile.ZipFile(wheel, "w") as archive:
        archive.writestr(item, data)
    extracted = tmp_path / "source-free"
    with zipfile.ZipFile(wheel) as archive:
        archive.extractall(extracted)
    assert [path.name for path in extracted.iterdir()] == ["typing_extensions.pyc"]
    result = subprocess.run(
        [
            sys.executable,
            "-I",
            "-S",
            "-c",
            "import sys; sys.path.insert(0, sys.argv[1]); import typing_extensions as t; "
            "assert t.fixture_marker == 2388; assert t.__file__.endswith('.pyc'); print(t.__file__)",
            str(extracted),
        ],
        capture_output=True,
        text=True,
        timeout=30,
    )
    assert result.returncode == 0, result.stderr
    assert str(extracted / "typing_extensions.pyc") in result.stdout


@pytest.mark.parametrize("target", ["wheel", "sdist"])
@pytest.mark.parametrize("reverse", [False, True])
@pytest.mark.parametrize(
    "ancestor,child,directory",
    [
        ("pkg/resources", "pkg/resources/manifest.json", False),
        ("pkg", "pkg/resources/nested/manifest.json", False),
        ("pkg/RESOURCES", "pkg/resources/manifest.json", False),
        ("pkg/resources", "pkg/\uff52esources/manifest.json", False),
        ("pkg/resources", "pkg/resources/", True),
    ],
)
def test_all_consumers_reject_conflicting_archive_topology(
    tmp_path: Path, target: str, reverse: bool, ancestor: str, child: str, directory: bool
) -> None:
    entries = [_zip_entry(ancestor), _zip_entry(child, directory=directory)]
    _assert_consumers_reject(
        tmp_path, target, list(reversed(entries)) if reverse else entries, "file/directory conflict"
    )


@pytest.mark.parametrize("target", ["wheel", "sdist"])
@pytest.mark.parametrize("reverse", [False, True])
def test_valid_explicit_and_implied_directory_topology_is_publishable_and_extractable(
    tmp_path: Path, target: str, reverse: bool
) -> None:
    entries = [_zip_entry("pkg/resources/", directory=True), _zip_entry("pkg/resources/manifest.json")]
    if reverse:
        entries.reverse()
    directory = tmp_path / "dist"
    artifact = _write_distribution_set(directory, target, entries)
    result = _publication_cli(directory)
    assert result.returncode == 0, (result.stdout, result.stderr)
    extracted = tmp_path / "extracted"
    if target == "wheel":
        _check_wheel_archive(artifact, extracted)
        resource = extracted / "pkg/resources/manifest.json"
    else:
        with tarfile.open(artifact) as archive:
            archive.extractall(extracted)
        resource = extracted / _SDIST_ROOT / "pkg/resources/manifest.json"
    assert resource.read_bytes() == b"fixture\n"


@pytest.mark.parametrize("target", ["wheel", "sdist"])
@pytest.mark.parametrize("reverse", [False, True])
def test_conflicting_topology_really_fails_archive_extraction(tmp_path: Path, target: str, reverse: bool) -> None:
    entries = [_zip_entry("pkg/resources"), _zip_entry("pkg/resources/manifest.json")]
    if reverse:
        entries.reverse()
    artifact = _write_distribution_set(tmp_path / "dist", target, entries)
    with pytest.raises(OSError):
        if target == "wheel":
            with zipfile.ZipFile(artifact) as archive:
                archive.extractall(tmp_path / "extracted")
        else:
            with tarfile.open(artifact) as archive:
                archive.extractall(tmp_path / "extracted")
