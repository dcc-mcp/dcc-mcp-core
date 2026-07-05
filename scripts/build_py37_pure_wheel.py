"""Build the py37-lite ``py3-none-any`` wheel without a ``_core`` extension.

Maturin packages a platform wheel whenever ``Cargo.toml`` lists ``cdylib`` in
``crate-type``, even when the PyO3 module is absent. For Maya 2022 / Python 3.7
we need a pure-Python wheel, so this helper temporarily restricts the root
crate to ``rlib`` only for the duration of ``maturin build``.
"""

from __future__ import annotations

from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
CARGO_TOML = ROOT / "Cargo.toml"
CARGO_BACKUP = ROOT / "Cargo.toml.py37.bak"
CRATE_TYPE_FROM = 'crate-type = ["cdylib", "rlib"]'
CRATE_TYPE_TO = 'crate-type = ["rlib"]'


def main() -> int:
    """Patch ``Cargo.toml``, invoke maturin, and restore the manifest."""
    text = CARGO_TOML.read_text(encoding="utf-8")
    if CRATE_TYPE_FROM not in text:
        sys.stderr.write(f"build_py37_pure_wheel: expected {CRATE_TYPE_FROM!r} in Cargo.toml\n")
        return 1

    shutil.copy2(CARGO_TOML, CARGO_BACKUP)
    try:
        CARGO_TOML.write_text(text.replace(CRATE_TYPE_FROM, CRATE_TYPE_TO), encoding="utf-8")
        cmd = [
            "maturin",
            "build",
            "--release",
            "--out",
            "dist",
            "--no-default-features",
            "-F",
            "py37-lite",
        ]
        subprocess.check_call(cmd, cwd=str(ROOT))
    finally:
        shutil.move(str(CARGO_BACKUP), str(CARGO_TOML))
    return 0


if __name__ == "__main__":
    sys.exit(main())
