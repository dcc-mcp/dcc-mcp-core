"""Local-only FFmpeg selection. Never launch a package manager or installer."""

from __future__ import annotations

import os
from pathlib import Path
import platform
import re
import shutil

FFMPEG_ENV = "DCC_MCP_MEDIA_FFMPEG_BIN"
FFPROBE_ENV = "DCC_MCP_MEDIA_FFPROBE_BIN"


def _fail(error_cls, message, code="command_unavailable"):
    raise error_cls(
        message,
        code,
        prompt=(
            "Preinstall a compatible FFmpeg/FFprobe pair, or set DCC_MCP_MEDIA_FFMPEG_BIN and "
            "DCC_MCP_MEDIA_FFPROBE_BIN to absolute executable paths. No installation was attempted."
        ),
    )


def _executable(path):
    if not path.is_file() or not os.access(str(path), os.X_OK):
        return False
    # Reject known vx shim directories and script wrappers. Native headers are
    # only a format filter, not proof of authenticity or absence of side effects.
    vx_home = Path(os.environ.get("VX_HOME", str(Path.home() / ".vx"))).resolve()
    if path.resolve().parent in {vx_home / "bin", vx_home / "shims"}:
        return False
    try:
        with path.open("rb") as stream:
            header = stream.read(4)
    except OSError:
        return False
    return header.startswith(b"MZ") or header in {
        b"\x7fELF",
        b"\xfe\xed\xfa\xce",
        b"\xce\xfa\xed\xfe",
        b"\xfe\xed\xfa\xcf",
        b"\xcf\xfa\xed\xfe",
        b"\xca\xfe\xba\xbe",
        b"\xbe\xba\xfe\xca",
        b"\xca\xfe\xba\xbf",
        b"\xbf\xba\xfe\xca",
    }


def _explicit(name, error_cls):
    value = os.environ[name]
    path = Path(value)
    if not value or not path.is_absolute() or not _executable(path):
        _fail(
            error_cls,
            name + " must identify an existing absolute native executable path (not a script or shim).",
            "invalid_runtime_override",
        )
    return path.resolve()


def _vx_candidates(probe_override=None):
    """Read the vx 0.9.37 store layout without invoking vx or scanning recursively.

    This deliberately does not evaluate vx.toml/providers or install anything.
    Operators needing a project pin must use the explicit executable override.
    """
    # Source: vx-org/vx v0.9.37 crates/vx-starlark/src/handle.rs, find_executable.
    root = Path(os.environ.get("VX_HOME", str(Path.home() / ".vx"))) / "store" / "ffmpeg"
    if not root.is_dir():
        return
    versions = []
    # Bound work in an unexpectedly large or corrupt store.
    try:
        with os.scandir(str(root)) as entries:
            for index, entry in enumerate(entries):
                if index >= 128:
                    return
                if entry.is_dir() and re.fullmatch(r"[0-9]{1,8}(?:\.[0-9]{1,8}){1,3}", entry.name):
                    versions.append(entry.name)
    except OSError:
        return  # An unreadable store is not an available installation.
    machine = platform.machine().lower()
    arch = {"amd64": "x64", "x86_64": "x64", "aarch64": "arm64", "i686": "x86", "i386": "x86", "armv7l": "arm"}.get(
        machine, machine
    )
    system = {"Linux": "linux", "Darwin": "darwin", "Windows": "windows"}.get(platform.system(), "unknown")
    platform_dir = system + "-" + arch
    executable = "ffmpeg.exe" if os.name == "nt" else "ffmpeg"
    for version in sorted(
        versions, key=lambda value: (tuple(int(part) for part in value.split(".")), value), reverse=True
    ):
        base = root / version
        for directory in (base, base / "bin", base / "ffmpeg", base / platform_dir, base / platform_dir / "bin"):
            candidate = directory / executable
            probe = directory / ("ffprobe.exe" if os.name == "nt" else "ffprobe")
            if _executable(candidate) and (probe_override is not None or _executable(probe)):
                resolved = candidate.resolve()
                try:
                    resolved.relative_to(base.resolve())
                except ValueError:
                    continue
                if probe_override is not None or probe.resolve().parent == resolved.parent:
                    yield resolved, version


def resolve_runtime(error_cls):
    """Select explicit override, existing vx store, then PATH, without subprocesses."""
    probe_override = _explicit(FFPROBE_ENV, error_cls) if FFPROBE_ENV in os.environ else None
    source, version = "environment", None
    if FFMPEG_ENV in os.environ:
        encoder = _explicit(FFMPEG_ENV, error_cls)
    else:
        candidate = next(_vx_candidates(probe_override), None)
        if candidate is not None:
            encoder, version = candidate
            source = "vx_store"
        else:
            found = shutil.which("ffmpeg")
            if found is None:
                _fail(error_cls, "FFmpeg is unavailable in the local vx store and on PATH.")
            encoder, source = Path(found).resolve(), "PATH"
    if probe_override is not None:
        probe = probe_override
        probe_source = "environment"
    else:
        probe = encoder.with_name("ffprobe.exe" if encoder.suffix.lower() == ".exe" else "ffprobe").resolve()
        probe_source = source + "_sibling"
        if probe.parent != encoder.parent:
            _fail(error_cls, "FFprobe resolves outside the selected FFmpeg directory.", "untrusted_executable")
    if not _executable(encoder) or not _executable(probe):
        _fail(error_cls, "The selected FFmpeg installation lacks an executable FFmpeg/FFprobe pair.")
    return {
        "ffmpeg": {
            "path": str(encoder),
            "source": source,
            "store_version": version,
            "selection_policy": "latest_numeric_installed_no_project_pins" if source == "vx_store" else source,
        },
        "ffprobe": {
            "path": str(probe),
            "source": probe_source,
            "store_version": None if probe_override is not None else version,
        },
    }


def inspect_versions(binaries, execute, error_cls):
    """Identify both tools and reject differing release/build/library identities."""
    identities = []
    for tool in ("ffmpeg", "ffprobe"):
        output = execute([binaries[tool]["path"], "-version"])
        lines = output.splitlines()
        prefix = tool + " version "
        if not lines or not lines[0].startswith(prefix) or not lines[0][len(prefix) :].split():
            _fail(error_cls, "Cannot identify the selected " + tool + ".", "invalid_encoder_version")
        version = lines[0][len(prefix) :].split()[0]
        configuration = next((line.strip() for line in lines if line.strip().startswith("configuration:")), None)
        libraries = [line.strip() for line in lines if re.match(r"\s*lib(?:av|sw|postproc)", line)]
        identities.append((version, configuration, libraries))
        binaries[tool].update(version=lines[0], build_configuration=configuration, library_versions=libraries)
    if identities[0] != identities[1]:
        _fail(error_cls, "FFmpeg and FFprobe release/build identities differ.", "runtime_mismatch")
    return binaries


def encoder_names(output):
    """Parse real encoder identifiers, not arbitrary substring matches."""
    return {
        parts[1]
        for line in output.splitlines()
        for parts in [line.split()]
        if len(parts) >= 2
        and re.fullmatch(r"[VAS][A-Z.]{5}", parts[0])
        and re.fullmatch(r"[A-Za-z0-9_][A-Za-z0-9_-]{0,79}", parts[1])
    }
