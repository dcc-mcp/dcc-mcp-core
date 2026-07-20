#!/usr/bin/env python3
"""Retag and validate dcc-mcp-server binary wheels."""

from __future__ import annotations

import argparse
from pathlib import Path
import subprocess
import sys
import zipfile

EXTENSION_SUFFIXES = (".pyd", ".abi3.so")


def _github_error(message: str) -> None:
    print(f"::error::{message}", file=sys.stderr)


def _find_wheels(wheel_dir: Path, pattern: str = "*.whl") -> list[Path]:
    wheels = sorted(wheel_dir.glob(pattern))
    if not wheels:
        _github_error(f"No dcc-mcp-server wheel was built under {wheel_dir}")
        raise SystemExit(1)
    return wheels


def _python_extensions(wheel: Path) -> list[str]:
    with zipfile.ZipFile(wheel) as zf:
        return [
            name
            for name in zf.namelist()
            if name.lower().endswith(EXTENSION_SUFFIXES)
            or (".cpython-" in name.lower() and name.lower().endswith(".so"))
        ]


def _assert_no_python_extensions(wheels: list[Path]) -> None:
    for wheel in wheels:
        extensions = _python_extensions(wheel)
        if extensions:
            _github_error(f"{wheel.name} contains CPython extension files and cannot be tagged py3-none: {extensions}")
            raise SystemExit(1)


def _retag(wheel_dir: Path) -> None:
    wheels = _find_wheels(wheel_dir, "dcc_mcp_server-*.whl")
    _assert_no_python_extensions(wheels)

    try:
        import wheel  # noqa: F401
    except ModuleNotFoundError:
        _github_error(
            "The Python 'wheel' package is required to retag server binary wheels. "
            "Install it with `python -m pip install wheel>=0.46`."
        )
        raise SystemExit(1) from None

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
                *[str(wheel) for wheel in wheels],
            ],
            check=True,
        )
    except subprocess.CalledProcessError as exc:
        _github_error(f"Failed to retag server binary wheels: {exc}")
        raise SystemExit(exc.returncode) from exc


def _read_dist_info(zf: zipfile.ZipFile, suffix: str) -> str:
    try:
        name = next(name for name in zf.namelist() if name.endswith(suffix))
    except StopIteration:
        raise ValueError(f"missing {suffix}") from None
    return zf.read(name).decode("utf-8")


def _validate(wheel_dir: Path) -> None:
    wheels = _find_wheels(wheel_dir)

    for wheel in wheels:
        with zipfile.ZipFile(wheel) as zf:
            names = zf.namelist()
            wheel_metadata = _read_dist_info(zf, ".dist-info/WHEEL")
            metadata = _read_dist_info(zf, ".dist-info/METADATA")

        if "Requires-Python: >=3.7" not in metadata:
            _github_error(
                f"{wheel.name} must declare Requires-Python: >=3.7 so Maya 2022 / Python 3.7 can install dcc-mcp-server"
            )
            raise SystemExit(1)

        if "-py3-none-" not in wheel.name or "Tag: py3-none-" not in wheel_metadata:
            _github_error(
                f"{wheel.name} must use py3-none platform tags because "
                "dcc-mcp-server ships a standalone binary, not a CPython extension"
            )
            raise SystemExit(1)

        if "manylinux_2_39" in wheel.name:
            _github_error(
                f"{wheel.name} was built against the GitHub runner glibc. "
                "Build Linux server wheels in manylinux2014 so Maya 2022 / "
                "Python 3.7 can resolve dcc-mcp-server."
            )
            raise SystemExit(1)

        if "manylinux" in wheel.name and not ("manylinux_2_17" in wheel.name or "manylinux2014" in wheel.name):
            _github_error(
                f"{wheel.name} must target manylinux2014 / manylinux_2_17 for older DCC-hosted Python environments."
            )
            raise SystemExit(1)

        removed_capture_helpers = [name for name in names if name.endswith("dcc-mcp-capture-helper.exe")]
        if removed_capture_helpers:
            _github_error(f"{wheel.name} contains removed capture helper: {removed_capture_helpers}")
            raise SystemExit(1)

        ui_control_hosts = [name for name in names if name.endswith(".data/scripts/dcc-mcp-ui-control-host.exe")]
        if "win_amd64" in wheel.name and len(ui_control_hosts) != 1:
            _github_error(
                f"{wheel.name} must contain exactly one dcc-mcp-ui-control-host.exe, found {ui_control_hosts}"
            )
            raise SystemExit(1)
        if "win_amd64" not in wheel.name and ui_control_hosts:
            _github_error(f"{wheel.name} must not contain the Windows UI Control host: {ui_control_hosts}")
            raise SystemExit(1)

        print(f"{wheel.name}: Python 3.7+ metadata OK")


def main() -> int:
    """Run the server wheel tag helper."""
    parser = argparse.ArgumentParser()
    parser.add_argument("command", choices=["retag", "validate"])
    parser.add_argument(
        "--wheel-dir",
        type=Path,
        default=Path("pkg/dcc-mcp-server-bin/wheels"),
    )
    args = parser.parse_args()

    if args.command == "retag":
        _retag(args.wheel_dir)
    else:
        _validate(args.wheel_dir)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
