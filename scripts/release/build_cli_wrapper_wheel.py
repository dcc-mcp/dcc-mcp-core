#!/usr/bin/env python3
"""Build the platform-specific ``dcc-mcp-cli`` PyPI wrapper wheel.

The wheel embeds the compressed release archive produced by
:mod:`scripts.release.build_standalone_bundle` instead of the raw executable:
PyPI meters a project's *total* storage, and the deflated archive costs roughly
a third of the raw binary per release. The archive is unpacked on first use by
``pkg/dcc-mcp-cli-bin/python/dcc_mcp_cli/_bootstrap.py``.

One job can build every platform's wheel: nothing here compiles, so the
release workflow runs this once per platform on a single runner using the
archives the ``build-binaries`` job already uploaded.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import zipfile

REPO_ROOT = Path(__file__).resolve().parent.parent.parent
DEFAULT_PROJECT_DIR = REPO_ROOT / "pkg" / "dcc-mcp-cli-bin"
PAYLOAD_SCHEMA_VERSION = 1
BINARY_STEM = "dcc-mcp-cli"

# Rust's `x86_64-unknown-linux-gnu` target requires glibc >= 2.17, and the
# project's other Linux wheels already target manylinux2014 (== 2.17).
MANYLINUX_FLOOR = (2, 17)
# maturin's deployment target for a universal2 wheel; the runner builds arm64
# halves that cannot run below macOS 11.
MACOS_UNIVERSAL2_TAG = "macosx_11_0_universal2"

PLATFORM_TAGS = {
    "windows-x86_64": "win_amd64",
    "macos-universal2": MACOS_UNIVERSAL2_TAG,
}
_ARCHIVE_SUFFIX = {"windows-x86_64": ".exe"}


def _github_error(message: str) -> None:
    print(f"::error::{message}", file=sys.stderr)


def _member_name(platform: str) -> str:
    """Return the archive member name for one release platform."""
    return f"{BINARY_STEM}{_ARCHIVE_SUFFIX.get(platform, '')}"


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _version_key(value: str) -> tuple:
    parts = []
    for chunk in value.split("."):
        parts.append(int(chunk) if chunk.isdigit() else 0)
    return tuple(parts)


def _detect_glibc(binary: Path) -> tuple | None:
    """Return the highest ``GLIBC_x.y`` version the binary requires.

    Args:
        binary: Executable to inspect.

    Returns:
        ``(major, minor)`` or ``None`` when no tool can answer.

    """
    for command in (["objdump", "-T", str(binary)], ["nm", "-D", str(binary)]):
        try:
            completed = subprocess.run(command, capture_output=True, text=True, check=False)
        except OSError:
            continue
        versions = set(re.findall(r"GLIBC_(\d+)\.(\d+)", completed.stdout))
        if versions:
            return max((int(major), int(minor)) for major, minor in versions)
    return None


def _runner_glibc() -> tuple | None:
    """Return the glibc version of the machine running this script."""
    try:
        completed = subprocess.run(["ldd", "--version"], capture_output=True, text=True, check=False)
    except OSError:
        return None
    match = re.search(r"(\d+)\.(\d+)", completed.stdout)
    return (int(match.group(1)), int(match.group(2))) if match else None


def linux_platform_tag(binary: Path, platform: str) -> str:
    """Return the PEP 600 manylinux tag for a Linux release archive.

    Args:
        binary: Extracted executable to inspect.
        platform: Release platform label, for example ``linux-x86_64``.

    Returns:
        A ``manylinux_<major>_<minor>_<arch>`` tag.

    """
    arch = platform.split("-", 1)[1] if "-" in platform else "x86_64"
    detected = _detect_glibc(binary) or _runner_glibc()
    if detected is None:
        raise SystemExit("cannot determine the glibc requirement of the Linux dcc-mcp-cli binary")
    major, minor = max(detected, MANYLINUX_FLOOR)
    return f"manylinux_{major}_{minor}_{arch}"


def platform_tag(archive: Path, platform: str, member: str) -> str:
    """Return the wheel platform tag for one release archive.

    Args:
        archive: Release zip holding the CLI binary.
        platform: Release platform label.
        member: Archive member holding the executable.

    Returns:
        The wheel platform tag.

    Raises:
        SystemExit: if the archive is missing ``member``.

    """
    if platform in PLATFORM_TAGS:
        return PLATFORM_TAGS[platform]
    with tempfile.TemporaryDirectory() as temporary:
        extracted = Path(temporary) / member
        with zipfile.ZipFile(archive) as archive_file:
            try:
                with archive_file.open(member) as source, extracted.open("wb") as target:
                    shutil.copyfileobj(source, target)
            except KeyError as exc:
                _github_error(f"{archive.name} does not contain {member!r}")
                raise SystemExit(1) from exc
        return linux_platform_tag(extracted, platform)


def stage_payload(*, project_dir: Path, archive: Path, version: str, platform: str, member: str) -> Path:
    """Copy the release archive into the wrapper package and write its manifest.

    Args:
        project_dir: Wrapper package root (``pkg/dcc-mcp-cli-bin``).
        archive: Release zip to embed.
        version: Release version without the ``v`` prefix.
        platform: Release platform label.
        member: Archive member holding the executable.

    Returns:
        Directory the payload was staged into.

    """
    payload_dir = project_dir / "python" / "dcc_mcp_cli" / "_payload"
    if payload_dir.is_dir():
        shutil.rmtree(payload_dir)
    payload_dir.mkdir(parents=True)
    staged = payload_dir / archive.name
    shutil.copyfile(archive, staged)
    metadata = {
        "schema_version": PAYLOAD_SCHEMA_VERSION,
        "distribution": "dcc-mcp-cli",
        "version": version,
        "platform": platform,
        "archive": archive.name,
        "archive_sha256": _sha256(staged),
        "member": member,
        "wheel_platform_tag": platform_tag(archive, platform, member),
    }
    (payload_dir / "payload.json").write_text(json.dumps(metadata, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return payload_dir


def build_wheel(*, project_dir: Path, out_dir: Path) -> list:
    """Build the platform-independent wheel for the staged payload.

    Args:
        project_dir: Wrapper package root.
        out_dir: Directory the wheel is written to.

    Returns:
        Paths of the wheels produced (exactly one).

    """
    out_dir.mkdir(parents=True, exist_ok=True)
    before = set(out_dir.glob("*.whl"))
    completed = subprocess.run(
        [
            sys.executable,
            "-m",
            "build",
            "--wheel",
            "--no-isolation",
            "--outdir",
            str(out_dir),
            str(project_dir),
        ],
        check=False,
    )
    if completed.returncode != 0:
        _github_error("hatchling failed to build the dcc-mcp-cli wrapper wheel")
        raise SystemExit(completed.returncode)
    produced = sorted(set(out_dir.glob("*.whl")) - before)
    if len(produced) != 1:
        _github_error(f"expected exactly one wrapper wheel, got {[p.name for p in produced]}")
        raise SystemExit(1)
    return produced


def main() -> int:
    """Run the wrapper wheel builder."""
    parser = argparse.ArgumentParser()
    parser.add_argument("--version", required=True)
    parser.add_argument("--platform", required=True)
    parser.add_argument("--zip", type=Path, required=True, dest="archive")
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--project-dir", type=Path, default=DEFAULT_PROJECT_DIR)
    args = parser.parse_args()

    archive = args.archive
    if not archive.is_file():
        _github_error(f"release archive not found: {archive}")
        return 1

    member = _member_name(args.platform)
    try:
        stage_payload(
            project_dir=args.project_dir,
            archive=archive,
            version=args.version,
            platform=args.platform,
            member=member,
        )
        wheels = build_wheel(project_dir=args.project_dir, out_dir=args.out_dir)
    except SystemExit as exc:
        return int(exc.code) if isinstance(exc.code, int) else 1

    wheel = wheels[0]
    print(f"built {wheel}")
    github_output = os.environ.get("GITHUB_OUTPUT")
    if github_output:
        with Path(github_output).open("a", encoding="utf-8") as handle:
            handle.write(f"wheel_path={wheel.as_posix()}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
