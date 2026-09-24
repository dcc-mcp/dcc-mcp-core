#!/usr/bin/env python3
"""Retag and validate ``dcc-mcp-cli`` wrapper wheels.

hatchling builds these wheels as pure Python (``py3-none-any``), but each one
carries an archive for a single platform. This helper stamps the platform tag
recorded in the wheel's ``_payload/payload.json`` and then asserts the wheel
still declares the metadata the project's Python LTS contract requires.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import subprocess
import sys
import zipfile

EXPECTED_REQUIRES_PYTHON = ">=3.7"
FORBIDDEN_SUFFIXES = (".pyd", ".abi3.so")


def _github_error(message: str) -> None:
    print(f"::error::{message}", file=sys.stderr)


def _find_wheels(wheel_dir: Path) -> list:
    wheels = sorted(wheel_dir.glob("dcc_mcp_cli-*.whl"))
    if not wheels:
        _github_error(f"No dcc-mcp-cli wrapper wheel was built under {wheel_dir}")
        raise SystemExit(1)
    return wheels


def _read_payload(wheel: Path) -> dict:
    """Read the staged payload manifest from inside a wheel.

    Args:
        wheel: Wrapper wheel to inspect.

    Returns:
        Parsed ``payload.json`` mapping.

    Raises:
        SystemExit: if the manifest is absent or invalid.

    """
    with zipfile.ZipFile(wheel) as archive:
        names = [name for name in archive.namelist() if name.endswith("_payload/payload.json")]
        if len(names) != 1:
            _github_error(f"{wheel.name} must carry exactly one _payload/payload.json, found {len(names)}")
            raise SystemExit(1)
        payload = json.loads(archive.read(names[0]).decode("utf-8"))
    if not payload.get("wheel_platform_tag"):
        _github_error(f"{wheel.name} payload is missing wheel_platform_tag")
        raise SystemExit(1)
    return payload


def _retag(wheel_dir: Path) -> None:
    """Stamp each wheel with the platform tag recorded in its payload."""
    try:
        import wheel
    except ModuleNotFoundError:
        _github_error(
            "The Python 'wheel' package is required to retag dcc-mcp-cli wheels. "
            "Install it with `python -m pip install wheel>=0.46`."
        )
        raise SystemExit(1) from None

    for wheel in _find_wheels(wheel_dir):
        payload = _read_payload(wheel)
        platform_tag = payload["wheel_platform_tag"]
        try:
            subprocess.run(
                [
                    sys.executable,
                    "-m",
                    "wheel",
                    "tags",
                    "--remove",
                    "--python-tag=py3",
                    "--abi-tag=none",
                    f"--platform-tag={platform_tag}",
                    str(wheel),
                ],
                check=True,
            )
        except subprocess.CalledProcessError as exc:
            _github_error(f"Failed to retag {wheel.name}: {exc}")
            raise SystemExit(exc.returncode) from exc
        print(f"{wheel.name}: tagged py3-none-{platform_tag}")


def _read_dist_info(archive: zipfile.ZipFile, suffix: str) -> str:
    try:
        name = next(name for name in archive.namelist() if name.endswith(suffix))
    except StopIteration:
        raise ValueError(f"missing {suffix}") from None
    return archive.read(name).decode("utf-8")


def _validate(wheel_dir: Path, version: str | None) -> None:
    """Assert every wheel declares the contract metadata and its payload."""
    for wheel in _find_wheels(wheel_dir):
        with zipfile.ZipFile(wheel) as archive:
            names = archive.namelist()
            wheel_metadata = _read_dist_info(archive, ".dist-info/WHEEL")
            metadata = _read_dist_info(archive, ".dist-info/METADATA")
            entry_points = _read_dist_info(archive, ".dist-info/entry_points.txt")

        if f"Requires-Python: {EXPECTED_REQUIRES_PYTHON}" not in metadata:
            _github_error(
                f"{wheel.name} must declare Requires-Python: {EXPECTED_REQUIRES_PYTHON} "
                "so Maya 2022 / Python 3.7 can install dcc-mcp-cli"
            )
            raise SystemExit(1)

        if "dcc-mcp-cli = dcc_mcp_cli:main" not in entry_points:
            _github_error(f"{wheel.name} must expose the dcc-mcp-cli console script")
            raise SystemExit(1)

        for name in names:
            if name.lower().endswith(FORBIDDEN_SUFFIXES):
                _github_error(f"{wheel.name} must not ship CPython extension modules (found {name})")
                raise SystemExit(1)

        payload = _read_payload(wheel)
        expected_tag = payload["wheel_platform_tag"]
        if f"-py3-none-{expected_tag}.whl" not in wheel.name or f"Tag: py3-none-{expected_tag}" not in wheel_metadata:
            _github_error(
                f"{wheel.name} must be tagged py3-none-{expected_tag} because the payload archive is platform-specific"
            )
            raise SystemExit(1)

        if "manylinux" in wheel.name and not any(
            marker in wheel.name for marker in ("manylinux_2_17", "manylinux2014")
        ):
            print(f"::warning::{wheel.name} targets a newer manylinux than the project's 2.17 baseline")

        if version is not None and payload["version"] != version:
            _github_error(f"{wheel.name} payload version is {payload['version']!r}, expected {version!r}")
            raise SystemExit(1)

        archive_member = next(
            (name for name in names if name.endswith(f"_payload/{payload['archive']}")),
            None,
        )
        if archive_member is None:
            _github_error(f"{wheel.name} is missing the payload archive {payload['archive']!r}")
            raise SystemExit(1)

        print(f"{wheel.name}: {expected_tag} payload {payload['version']} OK")


def main() -> int:
    """Run the wrapper wheel tag helper."""
    parser = argparse.ArgumentParser()
    parser.add_argument("command", choices=["retag", "validate"])
    parser.add_argument("--wheel-dir", type=Path, default=Path("dist-cli"))
    parser.add_argument("--version", default=None, help="expected release version, without the v prefix")
    args = parser.parse_args()

    if args.command == "retag":
        _retag(args.wheel_dir)
    else:
        _validate(args.wheel_dir, args.version)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
