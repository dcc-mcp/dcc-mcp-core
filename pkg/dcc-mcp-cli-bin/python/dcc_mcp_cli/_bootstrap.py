r"""Unpack and execute the ``dcc-mcp-cli`` binary shipped inside this wheel.

The wheel carries the same compressed release archive the GitHub Release
publishes (``dcc-mcp-cli-<version>-<platform>.zip``) rather than the raw
executable: PyPI meters a project's *total* storage (10 GB by default), and the
deflated archive costs roughly a third of the raw binary per release.

The archive is unpacked lazily, on the first invocation, into the environment's
scripts directory (``<venv>/bin`` or ``<venv>\\Scripts``) and then executed. Two
properties fall out of that choice:

* The running process *is* the real binary, so ``std::env::current_exe()``
  resolves inside the scripts directory. ``dcc-mcp-cli components ensure
  dcc-cua`` installs its companion executable as a sibling of
  ``current_exe``, so a single directory keeps the component contract intact.
* The unpacked binary is package-manager owned. A marker file next to it
  (``dcc-mcp-cli.package-manager.json``) tells the CLI that self-update is
  disabled and that upgrades belong to the package manager.
"""

from __future__ import annotations

import contextlib
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import sysconfig
import tempfile
import zipfile

__all__ = [
    "PACKAGE_MANAGER_MARKER",
    "binary_path",
    "main",
    "payload_dir",
    "resolve_binary",
]

#: Marker written next to the unpacked binary. ``crates/dcc-mcp-cli`` reads the
#: same file name to decide whether a package manager owns the installation.
PACKAGE_MANAGER_MARKER = "dcc-mcp-cli.package-manager.json"

#: Name of the manifest the release script stages next to the archive.
PAYLOAD_METADATA = "payload.json"

#: Package manager recorded in the marker. Read by the CLI's update command.
PACKAGE_MANAGER_NAME = "pypi"

_PAYLOAD_SCHEMA_VERSION = 1
_COPY_CHUNK_BYTES = 1024 * 1024
_EXECUTABLE_MODE = 0o755
#: Python console-script launchers start with a shebang. A native binary
#: never does, which is how a reinstalled launcher is told apart from the
#: binary it replaced.
_LAUNCHER_MAGIC = b"#!"


class PayloadError(RuntimeError):
    """Raised when the wheel does not carry a usable CLI payload."""


def payload_dir() -> Path:
    """Return the directory holding the bundled CLI archive.

    Returns:
        Absolute path of ``dcc_mcp_cli/_payload`` inside the installation.

    Raises:
        PayloadError: if the payload directory is missing.

    """
    directory = Path(__file__).resolve().parent / "_payload"
    if not directory.is_dir():
        raise PayloadError(
            f"{directory} is missing; this wheel was built without a dcc-mcp-cli payload. "
            "Build it with scripts/release/build_cli_wrapper_wheel.py."
        )
    return directory


def load_payload() -> dict:
    """Read and validate the bundled payload manifest.

    Returns:
        The parsed ``payload.json`` mapping.

    Raises:
        PayloadError: if the manifest or its archive is missing or malformed.

    """
    directory = payload_dir()
    metadata_path = directory / PAYLOAD_METADATA
    try:
        metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
    except OSError as exc:
        raise PayloadError(f"cannot read {metadata_path}: {exc}") from exc
    except ValueError as exc:
        raise PayloadError(f"{metadata_path} is not valid JSON: {exc}") from exc

    if not isinstance(metadata, dict):
        raise PayloadError(f"{metadata_path} must contain a JSON object")
    if int(metadata.get("schema_version", 0)) != _PAYLOAD_SCHEMA_VERSION:
        raise PayloadError(
            f"{metadata_path} declares schema_version {metadata.get('schema_version')!r}, "
            f"expected {_PAYLOAD_SCHEMA_VERSION}"
        )
    for key in ("version", "platform", "archive", "member"):
        value = metadata.get(key)
        if not isinstance(value, str) or not value:
            raise PayloadError(f"{metadata_path} is missing a non-empty {key!r}")
    if not (directory / metadata["archive"]).is_file():
        raise PayloadError(f"payload archive {metadata['archive']!r} is missing from {directory}")
    return metadata


def _binary_names() -> tuple:
    """Return candidate file names for the unpacked binary, best first.

    POSIX may reuse ``dcc-mcp-cli`` itself: replacing the console script with
    the real binary puts the native executable directly on ``PATH``. Windows
    cannot — the pip-generated ``dcc-mcp-cli.exe`` launcher is the running
    process image and cannot be replaced while it is mapped — so the unpacked
    binary always takes a distinct name there.
    """
    suffix = ".exe" if os.name == "nt" else ""
    if os.name == "nt":
        return (f"dcc-mcp-cli-bin{suffix}",)
    return (f"dcc-mcp-cli{suffix}", f"dcc-mcp-cli-bin{suffix}")


def _user_fallback_dir() -> Path:
    """Return a per-user directory used when the scripts dir is not writable."""
    if os.name == "nt":
        appdata = os.environ.get("LOCALAPPDATA")
        base = Path(appdata) if appdata else Path.home() / "AppData" / "Local"
        return base / "dcc-mcp-cli" / "bin"
    return Path.home() / ".local" / "share" / "dcc-mcp-cli" / "bin"


def candidate_dirs() -> list:
    """Return directories the binary may live in, most preferred first.

    ``DCC_MCP_CLI_BIN_DIR`` wins so operators can pin the location (read-only
    environments, container images). Otherwise the interpreter's scripts
    directory comes first, then the user-local fallback for prefixes the
    current user cannot write to.
    """
    candidates = []
    override = os.environ.get("DCC_MCP_CLI_BIN_DIR")
    if override:
        candidates.append(Path(override).expanduser())
    scripts = sysconfig.get_path("scripts")
    if scripts:
        candidates.append(Path(scripts))
    candidates.append(Path(sys.prefix) / ("Scripts" if os.name == "nt" else "bin"))
    candidates.append(_user_fallback_dir())

    ordered = []
    for candidate in candidates:
        resolved = Path(os.path.normpath(str(candidate)))
        if resolved not in ordered:
            ordered.append(resolved)
    return ordered


def marker_path(directory: Path) -> Path:
    """Return the package-manager marker path for one directory.

    Args:
        directory: Directory holding (or about to hold) the CLI binary.

    Returns:
        Path of the marker file inside ``directory``.

    """
    return directory / PACKAGE_MANAGER_MARKER


def _read_marker(directory: Path) -> dict | None:
    """Return the package-manager marker in ``directory``, if it parses."""
    path = marker_path(directory)
    try:
        marker = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return None
    return marker if isinstance(marker, dict) else None


def _marker_matches(marker: dict, payload: dict) -> bool:
    """Report whether ``marker`` was written for this exact payload version."""
    return marker.get("version") == payload.get("version") and marker.get("platform") == payload.get("platform")


def _is_launcher(path: Path) -> bool:
    """Report whether ``path`` looks like a Python console-script launcher.

    Reinstalling the *same* wheel version rewrites the pip-generated console
    script over the unpacked binary. The marker is not part of the
    distribution RECORD, so it survives that reinstall and would otherwise
    vouch for a file that is a launcher again: :func:`_execute` would then
    ``execv`` it, re-enter ``main()`` and loop forever without output.
    """
    try:
        with path.open("rb") as stream:
            return stream.read(len(_LAUNCHER_MAGIC)) == _LAUNCHER_MAGIC
    except OSError:
        return True


def _binary_is_intact(path: Path, marker: dict) -> bool:
    """Report whether ``path`` still holds the binary ``marker`` describes.

    Args:
        path: Candidate binary sitting next to the marker.
        marker: Parsed package-manager marker.

    Returns:
        ``True`` when the file is neither a console-script launcher nor a
        differently sized file than the one that was unpacked.

    """
    if _is_launcher(path):
        return False
    recorded = marker.get("binary_size")
    if not isinstance(recorded, int) or isinstance(recorded, bool):
        # Marker written before the fingerprint existed: fall back to the
        # launcher check alone so an older installation is not orphaned.
        return True
    try:
        return path.stat().st_size == recorded
    except OSError:
        return False


def _find_existing(payload: dict) -> Path | None:
    """Return an already unpacked binary matching ``payload``, if any."""
    for directory in candidate_dirs():
        marker = _read_marker(directory)
        if marker is None or not _marker_matches(marker, payload):
            continue
        for name in _binary_names():
            candidate = directory / name
            if candidate.is_file() and _binary_is_intact(candidate, marker):
                return candidate
    return None


def _extract(archive: Path, member: str, destination: Path) -> None:
    """Unpack one archive member to ``destination`` atomically.

    Args:
        archive: Release zip holding the CLI binary.
        member: Archive member to unpack.
        destination: Final path of the executable.

    Raises:
        PayloadError: if the archive does not contain ``member``.

    """
    destination.parent.mkdir(parents=True, exist_ok=True)
    handle, temporary_name = tempfile.mkstemp(
        prefix=destination.name + ".",
        suffix=".tmp",
        dir=str(destination.parent),
    )
    os.close(handle)
    temporary = Path(temporary_name)
    try:
        with zipfile.ZipFile(archive) as archive_file:
            try:
                source = archive_file.open(member)
            except KeyError as exc:
                raise PayloadError(f"{archive.name} does not contain {member!r}") from exc
            with source, temporary.open("wb") as target:
                shutil.copyfileobj(source, target, length=_COPY_CHUNK_BYTES)
        temporary.chmod(_EXECUTABLE_MODE)
        temporary.replace(destination)
    except BaseException:
        if temporary.exists():
            with contextlib.suppress(OSError):  # best effort cleanup
                temporary.unlink()
        raise


def _write_marker(directory: Path, payload: dict, binary_name: str) -> None:
    """Record that a package manager owns the binary in ``directory``.

    Args:
        directory: Directory holding the unpacked binary.
        payload: Payload manifest the binary was unpacked from.
        binary_name: File name of the unpacked binary.

    """
    marker = {
        "schema_version": _PAYLOAD_SCHEMA_VERSION,
        "distribution": payload.get("distribution", "dcc-mcp-cli"),
        "manager": PACKAGE_MANAGER_NAME,
        "version": payload["version"],
        "platform": payload["platform"],
        "binary": binary_name,
    }
    # Fingerprint the unpacked binary so a reinstall that restores the pip
    # console script cannot be mistaken for a healthy installation.
    with contextlib.suppress(OSError):
        marker["binary_size"] = (directory / binary_name).stat().st_size
    directory.mkdir(parents=True, exist_ok=True)
    temporary = directory / (PACKAGE_MANAGER_MARKER + ".tmp")
    temporary.write_text(json.dumps(marker, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    temporary.replace(marker_path(directory))


def _archive_digest(archive: Path, expected: str | None) -> None:
    """Verify the staged archive against the digest recorded at build time.

    Args:
        archive: Release zip to verify.
        expected: Lowercase hex SHA-256 recorded in ``payload.json``.

    Raises:
        PayloadError: if the digest is missing or does not match.

    """
    if not expected:
        raise PayloadError(f"{archive.name} has no archive_sha256 in {PAYLOAD_METADATA}; rebuild the wrapper wheel")
    digest = hashlib.sha256()
    with archive.open("rb") as stream:
        for chunk in iter(lambda: stream.read(_COPY_CHUNK_BYTES), b""):
            digest.update(chunk)
    if digest.hexdigest() != expected:
        raise PayloadError(f"{archive.name} sha256 mismatch: expected {expected}, got {digest.hexdigest()}")


def install_binary(payload: dict) -> Path:
    """Unpack the bundled archive into the first writable candidate directory.

    Args:
        payload: Parsed payload manifest.

    Returns:
        Path of the unpacked executable.

    Raises:
        PayloadError: if no candidate directory accepts the binary.

    """
    archive = payload_dir() / payload["archive"]
    _archive_digest(archive, payload.get("archive_sha256"))

    failures = []
    for directory in candidate_dirs():
        for name in _binary_names():
            destination = directory / name
            try:
                _extract(archive, payload["member"], destination)
            except (OSError, PayloadError) as exc:
                failures.append(f"{destination}: {exc}")
                continue
            try:
                _write_marker(directory, payload, name)
            except OSError as exc:  # pragma: no cover - rare, directory just worked
                failures.append(f"{marker_path(directory)}: {exc}")
                continue
            print(
                f"info: unpacked dcc-mcp-cli {payload['version']} ({payload['platform']}) to {destination}",
                file=sys.stderr,
            )
            return destination

    raise PayloadError(
        "could not unpack the dcc-mcp-cli binary into any candidate directory:\n  " + "\n  ".join(failures)
    )


def resolve_binary() -> Path:
    """Return the path of the CLI binary, unpacking it on first use.

    Returns:
        Path of the executable to run.

    Raises:
        PayloadError: if the payload is unusable or cannot be unpacked.

    """
    payload = load_payload()
    existing = _find_existing(payload)
    return existing if existing is not None else install_binary(payload)


def binary_path() -> Path:
    """Return the filesystem path of the bundled ``dcc-mcp-cli`` binary.

    Resolution order:

    1. ``DCC_MCP_CLI_BIN`` env var — operator override to an existing binary.
    2. An already unpacked binary whose marker matches this wheel's payload.
    3. The bundled archive unpacked into the environment's scripts directory.

    Returns:
        Path of the executable.

    Raises:
        PayloadError: if no binary can be resolved.

    """
    override = os.environ.get("DCC_MCP_CLI_BIN")
    if override:
        candidate = Path(override).expanduser()
        if candidate.is_file():
            return candidate
    return resolve_binary()


def _execute(binary: Path, argv: list) -> int:
    """Run the binary and return its exit code.

    POSIX replaces the current process so signals, exit codes and TTY state
    behave exactly like a native invocation. Windows cannot replace a running
    image, so it runs the binary as a child process instead.

    Args:
        binary: Executable to run.
        argv: Arguments to pass through.

    Returns:
        The process exit code (Windows only; POSIX never returns).

    """
    sys.stdout.flush()
    sys.stderr.flush()
    command = [str(binary)]
    command.extend(argv)
    if os.name == "nt":
        try:
            return int(subprocess.run(command).returncode)
        except KeyboardInterrupt:
            return 130
    os.execv(str(binary), command)
    return 1  # pragma: no cover - os.execv does not return


def main(argv: list | None = None) -> int:
    """Unpack (if needed) and run the bundled ``dcc-mcp-cli`` binary.

    Args:
        argv: Arguments forwarded to the CLI. Defaults to ``sys.argv[1:]``.

    Returns:
        The CLI's exit code.

    """
    arguments = list(sys.argv[1:] if argv is None else argv)
    try:
        binary = binary_path()
    except PayloadError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1
    return _execute(binary, arguments)
