"""Tests for the release script that builds the ``dcc-mcp-cli`` wrapper wheels.

Run with the wrapper package importable::

    PYTHONPATH=pkg/dcc-mcp-cli-bin/python pytest pkg/dcc-mcp-cli-bin/tests

These tests build real wheels with hatchling from synthetic release archives:
the release workflow builds one wheel per platform on a single runner, and a
bug in that loop is invisible to YAML-text assertions.
"""

from __future__ import annotations

from pathlib import Path
import shutil
import subprocess
import sys
import zipfile

import pytest

RELEASE_PLATFORMS = ("linux-x86_64", "macos-universal2", "windows-x86_64")
LINUX_TAG = "manylinux_2_17_x86_64"
EXPECTED_TAGS = {
    "linux-x86_64": LINUX_TAG,
    "macos-universal2": "macosx_11_0_universal2",
    "windows-x86_64": "win_amd64",
}
SYNTHETIC_BINARY = b"\x7fELF\x02\x01\x01\x00 synthetic stand-in for the release binary\n"


@pytest.fixture(scope="module")
def scripts():
    """Return the release scripts under test, skipping when build deps are absent."""
    pytest.importorskip("hatchling", reason="hatchling is required to build wrapper wheels")
    pytest.importorskip("build", reason="build is required to build wrapper wheels")
    pytest.importorskip("wheel", reason="wheel is required to retag wrapper wheels")
    from scripts.release import build_cli_wrapper_wheel
    from scripts.release import cli_wheel_tags

    return build_cli_wrapper_wheel, cli_wheel_tags


# Resolved from ``__file__`` rather than imported: both this directory and
# ``tests/`` hold a ``conftest.py``, and ``import conftest`` would bind to
# whichever landed on ``sys.path`` first.
PACKAGE_ROOT = Path(__file__).resolve().parent.parent


@pytest.fixture
def project_dir(tmp_path):
    """Return a writable copy of the wrapper package to stage payloads into."""
    target = tmp_path / "dcc-mcp-cli-bin"
    shutil.copytree(
        PACKAGE_ROOT,
        target,
        ignore=shutil.ignore_patterns("__pycache__", "_payload", "tests"),
    )
    return target


@pytest.fixture
def archives(tmp_path):
    """Create one synthetic release archive per platform."""
    directory = tmp_path / "dist-binaries"
    directory.mkdir()
    version = "9.9.9"
    for platform in RELEASE_PLATFORMS:
        member = "dcc-mcp-cli.exe" if platform == "windows-x86_64" else "dcc-mcp-cli"
        archive = directory / f"dcc-mcp-cli-{version}-{platform}.zip"
        with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as archive_file:
            archive_file.writestr(member, SYNTHETIC_BINARY)
    return directory


def _build(builder, *, version, platform, archive, out_dir, project_dir):
    """Invoke the builder the way the workflow does and return its exit code."""
    saved = list(sys.argv)
    sys.argv = [
        "build_cli_wrapper_wheel.py",
        "--version",
        version,
        "--platform",
        platform,
        "--zip",
        str(archive),
        "--out-dir",
        str(out_dir),
        "--project-dir",
        str(project_dir),
    ]
    try:
        return builder.main()
    except SystemExit as exc:
        return int(exc.code) if isinstance(exc.code, int) else 1
    finally:
        sys.argv = saved


def _retag(helper, wheel_dir: Path) -> int:
    """Run the retag helper the way the workflow does and return its exit code."""
    return int(
        subprocess.run(
            [sys.executable, str(Path(helper.__file__).resolve()), "retag", "--wheel-dir", str(wheel_dir)],
            check=False,
        ).returncode
    )


def _validate(helper, wheel_dir: Path, version: str) -> int:
    """Run the validate helper the way the workflow does and return its exit code."""
    return int(
        subprocess.run(
            [
                sys.executable,
                str(Path(helper.__file__).resolve()),
                "validate",
                "--wheel-dir",
                str(wheel_dir),
                "--version",
                version,
            ],
            check=False,
        ).returncode
    )


def _loop(scripts, *, version, archives, work, project_dir, per_platform_dir):
    """Run the release workflow's platform loop.

    ``per_platform_dir=False`` reproduces the pre-fix shape, where every
    platform built into one shared output directory.

    Returns:
        A ``(exit_code, shared_directory)`` tuple.

    """
    builder, helper = scripts
    shared = work / "dist-cli"
    shared.mkdir(parents=True, exist_ok=True)
    for platform in RELEASE_PLATFORMS:
        out_dir = work / "dist-cli-build" / platform if per_platform_dir else shared
        code = _build(
            builder,
            version=version,
            platform=platform,
            archive=archives / f"dcc-mcp-cli-{version}-{platform}.zip",
            out_dir=out_dir,
            project_dir=project_dir,
        )
        if code != 0:
            return code, shared
        if per_platform_dir:
            code = _retag(helper, out_dir)
            if code != 0:
                return code, shared
            for wheel in out_dir.glob("*.whl"):
                shutil.move(str(wheel), str(shared / wheel.name))
    return 0, shared


def test_shared_output_directory_cannot_hold_every_platform(scripts, archives, tmp_path, project_dir, monkeypatch):
    """Every build is named ``py3-none-any``, so a shared directory collides.

    Guards the *reason* the workflow gives each platform its own directory:
    hatchling always emits ``dcc_mcp_cli-<version>-py3-none-any.whl``, so the
    second platform's build overwrites the first and ``build_wheel()`` finds
    no new file. Without this test the loop below could silently regress to
    the shared-directory shape again.
    """
    builder, _ = scripts
    monkeypatch.setattr(builder, "linux_platform_tag", lambda binary, platform: LINUX_TAG)

    code, shared = _loop(
        scripts,
        version="9.9.9",
        archives=archives,
        work=tmp_path / "shared",
        project_dir=project_dir,
        per_platform_dir=False,
    )

    assert code == 1, "the shared output directory must fail on the second platform"
    assert len(list(shared.glob("*.whl"))) == 1


def test_each_platform_builds_a_distinctly_tagged_wheel(scripts, archives, tmp_path, project_dir, monkeypatch):
    """The workflow loop produces one correctly retagged wheel per platform.

    This is the end-to-end shape of the release job: build, retag and collect,
    then validate the collected set. A platform tag that is missing or wrong
    lets pip resolve a Linux binary onto Windows.
    """
    builder, helper = scripts
    monkeypatch.setattr(builder, "linux_platform_tag", lambda binary, platform: LINUX_TAG)

    code, shared = _loop(
        scripts,
        version="9.9.9",
        archives=archives,
        work=tmp_path / "per-platform",
        project_dir=project_dir,
        per_platform_dir=True,
    )

    assert code == 0
    names = sorted(wheel.name for wheel in shared.glob("*.whl"))
    assert len(names) == 3, names
    for platform, tag in EXPECTED_TAGS.items():
        assert any(f"-py3-none-{tag}.whl" in name for name in names), f"{platform} wheel missing for {tag}: {names}"
    assert len(set(names)) == 3, "two platforms produced the same wheel name"

    assert _validate(helper, shared, "9.9.9") == 0


def test_retagging_an_already_tagged_wheel_is_idempotent(scripts, archives, tmp_path, project_dir, monkeypatch):
    """The safety-net retag step must not rename or drop a collected wheel."""
    builder, helper = scripts
    monkeypatch.setattr(builder, "linux_platform_tag", lambda binary, platform: LINUX_TAG)

    code, shared = _loop(
        scripts,
        version="9.9.9",
        archives=archives,
        work=tmp_path / "idempotent",
        project_dir=project_dir,
        per_platform_dir=True,
    )
    assert code == 0
    before = sorted(wheel.name for wheel in shared.glob("*.whl"))

    assert _retag(helper, shared) == 0
    assert sorted(wheel.name for wheel in shared.glob("*.whl")) == before
    assert _validate(helper, shared, "9.9.9") == 0
