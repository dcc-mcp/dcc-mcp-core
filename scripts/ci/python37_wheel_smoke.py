"""Run current zero-backport or verified immutable historical wheel smokes."""

from __future__ import annotations

import argparse
from email.parser import Parser
from pathlib import Path
import re
import subprocess
import sys
import zipfile

try:
    from .python_support_contract import load_contract
    from .smoke_zero_typing_extensions import _single_wheel
except ImportError:  # pragma: no cover - direct script execution
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    from python_support_contract import load_contract
    from smoke_zero_typing_extensions import _single_wheel


def historical_release(checkout_ref: str, version: str, head: str, tag: str, cutoff: str) -> bool:
    """Allow declared dependencies only on a matching pre-cutover release tag."""
    match = re.fullmatch(r"(?:refs/tags/)?v(\d+\.\d+\.\d+)", checkout_ref)
    if match is None or tuple(map(int, match[1].split("."))) >= tuple(map(int, cutoff.split("."))):
        return False
    if version != match[1] or not re.fullmatch(r"[0-9a-f]{40}", head) or head != tag:
        raise ValueError("historical checkout tag, commit and wheel version must match")
    return True


def smoke_wheel(wheel: Path, profile: str, historical: bool, has_contract: bool = True) -> None:
    """Install the wheel with its applicable dependency policy, then exercise it."""
    scripts = Path(__file__).resolve().parent
    if not historical:
        # Never infer exemption from source text or the presence of a test file.
        subprocess.run(
            [sys.executable, str(scripts / "smoke_zero_typing_extensions.py"), "--wheel", str(wheel)], check=True
        )
    install = [sys.executable, "-m", "pip", "install", "--force-reinstall"]
    if not historical:
        install.append("--no-deps")
    # For historical tags pip prepares only dependencies declared by that wheel.
    # No backport is bundled, injected into metadata, or installed for current code.
    subprocess.run([*install, str(wheel)], check=True)
    if historical and not has_contract:
        subprocess.run([sys.executable, "-I", "-c", "import dcc_mcp_core"], check=True)
    else:
        subprocess.run(
            [sys.executable, "-I", str(scripts / "smoke_python37_runtime.py"), "--profile", profile], check=True
        )


def main() -> int:
    """Bind the selected tag and installed wheel to the workflow-owned policy."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--wheel", required=True)
    parser.add_argument("--profile", choices=("lite_py37", "native_py37"), required=True)
    parser.add_argument("--checkout-ref", default="")
    args = parser.parse_args()
    checkout_ref = args.checkout_ref
    if checkout_ref.startswith("refs/tags/"):
        checkout_ref = checkout_ref[len("refs/tags/") :]
    wheel = _single_wheel(args.wheel)
    with zipfile.ZipFile(str(wheel)) as archive:
        names = [name for name in archive.namelist() if name.endswith(".dist-info/METADATA")]
        if len(names) != 1:
            raise ValueError("wheel must contain exactly one METADATA file")
        version = Parser().parsestr(archive.read(names[0]).decode("utf-8"))["Version"]
    rules = load_contract()["distributions"]["dcc-mcp-core"]["forbidden_runtime_dependencies"]
    cutoff = next(rule["from_version"] for rule in rules if rule["name"] == "typing-extensions")
    historical = False
    has_contract = True
    if re.fullmatch(r"v\d+\.\d+\.\d+", checkout_ref):
        head = subprocess.check_output(["git", "rev-parse", "HEAD"], universal_newlines=True).strip()
        tag = subprocess.check_output(
            ["git", "rev-parse", "--verify", "refs/tags/" + checkout_ref + "^{commit}"],
            universal_newlines=True,
        ).strip()
        historical = historical_release(checkout_ref, version, head, tag, cutoff)
        if historical:
            # Query the verified immutable tree, not a removable local file.
            has_contract = (
                subprocess.run(
                    ["git", "cat-file", "-e", tag + ":compatibility/python.json"],
                    capture_output=True,
                ).returncode
                == 0
            )
    print(
        "Python 3.7 wheel policy: " + ("historical declared dependencies" if historical else "zero backport"),
        flush=True,
    )
    smoke_wheel(wheel, args.profile, historical, has_contract)
    return 0


if __name__ == "__main__":
    sys.exit(main())
