"""Fail when packaging, CI, or documentation drifts from Python LTS policy."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import sys
from typing import Any
from typing import Mapping

try:
    from .python_support_contract import ContractError
    from .python_support_contract import load_contract
    from .python_support_contract import minimum_python_spec
    from .python_support_contract import native_ci_matrix
    from .python_support_contract import repository_root
except ImportError:  # pragma: no cover - direct script execution
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    from python_support_contract import ContractError
    from python_support_contract import load_contract
    from python_support_contract import minimum_python_spec
    from python_support_contract import native_ci_matrix
    from python_support_contract import repository_root


def expected_fragments(contract: Mapping[str, Any]) -> dict[str, list[str]]:
    """Return required projections of the canonical contract into repo files."""
    support = contract["support"]
    build = contract["build"]
    native = build["native_py37"]
    abi3 = build["abi3"]
    toolchain = contract["test_toolchain"]

    classifiers = [f'"Programming Language :: Python :: {version}"' for version in support["tested_versions"]]
    full_matrix_versions = [
        f'"{version}"' for version in support["tested_versions"] if version != support["minimum_python"]
    ]

    return {
        "pyproject.toml": [
            f'requires-python = "{minimum_python_spec(dict(contract))}"',
            f'python_version = "{support["minimum_python"]}"',
            f'target-version = "py{support["minimum_python"].replace(".", "")}"',
            f"pytest=={toolchain['pytest']},<8.0; python_version<'3.8'",
            f"pytest-xdist=={toolchain['pytest_xdist']}; python_version<'3.8'",
            f"typing-extensions=={toolchain['typing_extensions']}; python_version<'3.8'",
            *classifiers,
        ],
        "pkg/dcc-mcp-server-bin/pyproject.toml": [
            f'requires-python = "{minimum_python_spec(dict(contract))}"',
            *classifiers,
        ],
        "CONTRIBUTING.md": [
            "Python 3.7-3.14",
            "native cp37 wheels on Linux/Windows",
        ],
        "Cargo.toml": [
            f'pyo3 = {{ version = "{build["pyo3_series"]}"',
            f'{abi3["feature"]} = ["pyo3/{abi3["feature"]}"]',
        ],
        "Cargo.lock": [
            f'name = "pyo3"\nversion = "{build["pyo3_series"]}.',
        ],
        "justfile": [
            "WHEEL_FEATURES_PY37 :=",
            "build-py37 *EXTRA:",
            "check-python-support:",
        ],
        ".github/workflows/ci.yml": [
            "python37-contract:",
            "python37-native:",
            "python37-gate:",
            "fromJSON(needs.python37-contract.outputs.native-matrix)",
            "install_python37_test_toolchain.py",
            f'python-version: "{native["python"]}"',
        ],
        ".github/workflows/build-wheels.yml": [
            "linux-py37:",
            "windows-py37:",
            "check_python_wheel.py --profile lite_py37",
        ],
        ".github/workflows/release.yml": [
            "uses: ./.github/workflows/build-wheels.yml",
            "needs.build-wheels.result == 'success'",
        ],
        ".github/workflows/python-matrix-full.yml": full_matrix_versions,
        ".github/actions/build-wheel/action.yml": [
            "check_python_wheel.py",
            "smoke_python37_runtime.py --profile native_py37",
        ],
    }


def collect_projection_errors(root: Path, contract: Mapping[str, Any]) -> list[str]:
    """Collect all contract drift errors without failing at the first one."""
    errors: list[str] = []
    for relative, fragments in expected_fragments(contract).items():
        path = root / relative
        try:
            text = path.read_text(encoding="utf-8")
        except OSError as exc:
            errors.append(f"{relative}: cannot read file: {exc}")
            continue
        for fragment in fragments:
            if fragment not in text:
                errors.append(f"{relative}: missing contract projection {fragment!r}")

    expiry_files = (
        "AGENTS.md",
        "docs/guide/agents-reference.md",
        "docs/guide/maya2022-support.md",
        "docs/zh/guide/maya2022-support.md",
        "docs/guide/py37-lite-architecture.md",
        ".github/workflows/ci.yml",
        "skills/dcc-mcp-creator/SKILL.md",
        "skills/dcc-mcp-skills-creator/SKILL.md",
        "skills/marketplace-publish-extension/SKILL.md",
        "CONTRIBUTING.md",
    )
    for relative in expiry_files:
        text = (root / relative).read_text(encoding="utf-8")
        if "2026-12-31" in text:
            errors.append(f"{relative}: calendar-based Python 3.7 expiry contradicts LTS policy")

    release_jobs = {
        "linux-x86_64": "linux-py37",
        "windows-x86_64": "windows-py37",
    }
    build_workflow = (root / ".github/workflows/build-wheels.yml").read_text(encoding="utf-8")
    for row in contract["build"]["native_py37"]["pr_matrix"]:
        job_name = release_jobs[row["platform"]]
        marker = f"  {job_name}:\n"
        start = build_workflow.find(marker)
        if start < 0:
            errors.append(f".github/workflows/build-wheels.yml: missing {job_name} job")
            continue
        next_job = build_workflow.find("\n  ", start + len(marker))
        while next_job >= 0:
            line = build_workflow[next_job + 1 :].split("\n", 1)[0]
            if line.startswith("  ") and not line.startswith("    ") and line.endswith(":"):
                break
            next_job = build_workflow.find("\n  ", next_job + 3)
        block = build_workflow[start : next_job if next_job >= 0 else len(build_workflow)]
        for fragment in (
            f"runs-on: {row['runner']}",
            f"target: {row['target']}",
            "python-version: '3.7'",
        ):
            if fragment not in block:
                errors.append(f".github/workflows/build-wheels.yml: {job_name} missing {fragment!r}")
    return errors


def main(argv: list[str] | None = None) -> int:
    """Validate the contract and every critical projection used by CI/release."""
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--github-output",
        type=Path,
        help="append the generated native matrix to this GitHub Actions output file",
    )
    args = parser.parse_args(argv)
    root = repository_root()
    try:
        contract = load_contract(root)
    except ContractError as exc:
        sys.stderr.write(f"python-support-contract: {exc}\n")
        return 1

    errors = collect_projection_errors(root, contract)
    if errors:
        for error in errors:
            sys.stderr.write(f"python-support-contract: {error}\n")
        return 1

    if args.github_output is not None:
        matrix = json.dumps(native_ci_matrix(contract), separators=(",", ":"), sort_keys=True)
        with args.github_output.open("a", encoding="utf-8") as stream:
            stream.write(f"native-matrix={matrix}\n")

    minimum = contract["support"]["minimum_python"]
    maximum = contract["support"]["maximum_tested_python"]
    sys.stdout.write(f"python-support-contract: OK (Python {minimum}-{maximum}, native py37 on Linux + Windows)\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
