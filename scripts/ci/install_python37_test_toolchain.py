"""Install the pinned test toolchain declared by the Python support contract."""

from __future__ import annotations

from pathlib import Path
import subprocess
import sys

try:
    from .python_support_contract import load_contract
    from .python_support_contract import python37_test_requirements
except ImportError:  # pragma: no cover - direct script execution
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    from python_support_contract import load_contract
    from python_support_contract import python37_test_requirements


def main() -> int:
    """Install the canonical Python 3.7 test requirements with this interpreter."""
    requirements = python37_test_requirements(load_contract())
    return subprocess.call([sys.executable, "-m", "pip", "install", *requirements])


if __name__ == "__main__":
    raise SystemExit(main())
