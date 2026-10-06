#!/usr/bin/env python3
"""Fail when a package-environment requirement is wider than the declared one.

An adapter declares the core version it supports twice. ``pip`` reads the
Python distribution metadata (``Requires-Dist: dcc-mcp-core>=0.19.3,<0.19.5``);
a studio's package-environment resolver reads a second, hand-written
requirement (``requires = ["dcc_mcp_core-0"]``). The second one drifts, and
because it drifts *wider*, the resolver happily selects a core release the
adapter excluded on PyPI. The environment then imports cleanly and fails
several modules deep with an error that never mentions the version, which is
exactly the failure this gate exists to prevent.

Two independent checks, both read-only:

``contract``
    The committed ``compatibility/adapter-core-requirement.json`` must parse,
    must keep the Python distribution metadata as the source of truth, and
    every ``known_violations`` row must agree with what the shared parser
    derives from the same inputs. A row that lists drift codes the parser no
    longer recomputes is stale and fails.

``--adapter-root PATH``
    Scan an adapter checkout: read the core requirement from its
    ``pyproject.toml`` dependencies and the requirement its ``package.py``
    declares, then compare. Violations are errors — adapters run this in their
    own CI so a newly unbounded requirement fails the release that introduces
    it. A requirement that cannot be parsed is an error, never an exemption.

Exit codes: 0 = clean, 1 = at least one error-severity finding, 2 = usage or
IO failure.

Stdlib only, Python 3.7+ (organisation red line: keep 3.7 compatible).

Usage:
    python scripts/ci/check_adapter_core_requirement.py
    python scripts/ci/check_adapter_core_requirement.py --adapter-root ../dcc-mcp-maya
    python scripts/ci/check_adapter_core_requirement.py --github
    python scripts/ci/check_adapter_core_requirement.py --json
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import sys
from typing import Any

try:
    from .adapter_core_requirement_contract import CONTRACT_RELATIVE_PATH
    from .adapter_core_requirement_contract import ContractError
    from .adapter_core_requirement_contract import core_requirement_module
    from .adapter_core_requirement_contract import load_contract
    from .adapter_core_requirement_contract import repository_root
except ImportError:  # pragma: no cover - direct script execution
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    from adapter_core_requirement_contract import CONTRACT_RELATIVE_PATH
    from adapter_core_requirement_contract import ContractError
    from adapter_core_requirement_contract import core_requirement_module
    from adapter_core_requirement_contract import load_contract
    from adapter_core_requirement_contract import repository_root


_PACKAGE_PY_MAX_DEPTH = 3
_REQUIRES_DIST_RE = re.compile(r"""^\s*["']([A-Za-z0-9._\-]+)\s*(.*?)["']\s*,?\s*$""")


class Finding:
    """One reported problem, with the severity the contract assigns it."""

    def __init__(self, severity: str, scope: str, code: str, message: str) -> None:
        """Store one finding."""
        self.severity = severity
        self.scope = scope
        self.code = code
        self.message = message

    def as_dict(self) -> dict[str, str]:
        """Return the finding as a JSON-serialisable mapping."""
        return {"severity": self.severity, "scope": self.scope, "code": self.code, "message": self.message}

    def render(self, github: bool = False) -> str:
        """Return the finding as one line, optionally as a GitHub annotation."""
        text = f"{self.scope}: {self.message}"
        if github and self.severity in {"error", "warning"}:
            return f"::{self.severity}::{text}"
        return f"{self.severity.upper()} {self.code}: {text}"


def _project_dependencies(pyproject: Path) -> list[str]:
    """Return the ``[project].dependencies`` entries of *pyproject*."""
    try:
        import tomllib
    except ModuleNotFoundError:  # pragma: no cover - exercised by the Python 3.7 lane
        try:
            import tomli as tomllib  # type: ignore[no-redef]
        except ImportError:
            return _project_dependencies_from_text(pyproject)
    try:
        document = tomllib.loads(pyproject.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return _project_dependencies_from_text(pyproject)
    project = document.get("project") or {}
    dependencies = project.get("dependencies")
    if not isinstance(dependencies, list):
        return []
    return [str(entry) for entry in dependencies]


def _project_dependencies_from_text(pyproject: Path) -> list[str]:
    """Return ``[project].dependencies`` entries without a TOML parser.

    Python 3.7 has neither :mod:`tomllib` nor a guaranteed :mod:`tomli`, and an
    adapter checkout is not worth a hard dependency. The fallback scans only
    the ``dependencies = [ ... ]`` array that follows a ``[project]`` header,
    so a ``[build-system] requires`` array can never be mistaken for it.
    """
    try:
        lines = pyproject.read_text(encoding="utf-8").splitlines()
    except OSError:
        return []
    entries: list[str] = []
    in_project = False
    in_dependencies = False
    for raw in lines:
        line = raw.strip()
        if line.startswith("[") and line.endswith("]"):
            in_project = line == "[project]"
            in_dependencies = False
            continue
        if not in_project:
            continue
        if in_dependencies:
            if line.startswith("]"):
                in_dependencies = False
                continue
            match = _REQUIRES_DIST_RE.match(line)
            if match is not None:
                entries.append(f"{match.group(1)} {match.group(2)}".strip())
            continue
        if re.match(r"^dependencies\s*=\s*\[", line):
            in_dependencies = True
            if line.endswith("]"):
                in_dependencies = False
    return entries


def _package_py_requires(package_py: Path) -> list[str] | None:
    """Return the ``requires`` string literals of a Rez ``package.py``.

    The file is parsed with :mod:`ast` and never executed, so an adapter
    checkout cannot run arbitrary code just by being scanned. Returns ``None``
    when ``requires`` is absent or is not a literal list of strings — the
    caller reports that as an error rather than assuming an exemption.
    """
    import ast

    try:
        tree = ast.parse(package_py.read_text(encoding="utf-8"), filename=str(package_py))
    except (OSError, SyntaxError):
        return None
    for node in tree.body:
        if not isinstance(node, ast.Assign):
            continue
        targets = [target for target in node.targets if isinstance(target, ast.Name)]
        if not any(target.id == "requires" for target in targets):
            continue
        value = node.value
        if not isinstance(value, (ast.List, ast.Tuple)):
            return None
        entries = []
        for element in value.elts:
            if not isinstance(element, ast.Constant) or not isinstance(element.value, str):
                return None
            entries.append(element.value)
        return entries
    return None


def _find_package_py(root: Path) -> Path | None:
    """Return the shallowest ``package.py`` under *root*, if any."""
    candidates = sorted(
        path
        for path in root.rglob("package.py")
        if len(path.relative_to(root).parts) <= _PACKAGE_PY_MAX_DEPTH
    )
    return candidates[0] if candidates else None


def _core_requirement_from_dependencies(dependencies: list[str], core_names: set[str]) -> str | None:
    """Return the core requirement declared in *dependencies*."""
    for entry in dependencies:
        name = re.split(r"[\s\[<>=!~;]", entry.strip(), maxsplit=1)[0].strip()
        if name.replace("-", "_").lower() in core_names:
            return entry.strip()
    return None


def _core_requirement_from_package(requires: list[str], core_names: set[str]) -> str | None:
    """Return the core requirement declared in a ``package.py`` ``requires`` list."""
    for entry in requires:
        name = entry.split("-", 1)[0].strip()
        if name.replace("-", "_").lower() in core_names:
            return entry.strip()
    return None


def check_adapter_root(root: Path, module: Any, contract: dict[str, Any]) -> list[Finding]:
    """Compare an adapter checkout's two core declarations."""
    package = str(contract["core_package"])
    import_name = str(contract["core_import_name"])
    # Both spellings address one package; requirements are matched on the
    # underscore form so ``dcc-mcp-core`` and ``dcc_mcp_core`` compare equal.
    core_names = {package.replace("-", "_").lower(), import_name.replace("-", "_").lower()}

    scope = root.name or str(root)
    pyproject = root / "pyproject.toml"
    if not pyproject.is_file():
        return [Finding("error", scope, "missing_pyproject", "no pyproject.toml to read a declared requirement from")]

    dependencies = _project_dependencies(pyproject)
    declared_text = _core_requirement_from_dependencies(dependencies, core_names)
    if declared_text is None:
        return [
            Finding(
                "error",
                scope,
                "missing_declared_requirement",
                f"{pyproject.name} declares no {package} requirement in [project].dependencies",
            )
        ]

    findings: list[Finding] = []
    try:
        declared = module.requirement_from_pep440(declared_text, package=package)
    except ValueError as exc:
        return [Finding("error", scope, "unparseable", str(exc))]

    if module.open_ceiling(declared):
        findings.append(
            Finding(
                "warning",
                scope,
                "open_ceiling",
                f"declared ceiling '{declared.upper}' is the 1.0.0 placeholder; bound it to the next minor above the "
                "highest core release this adapter was verified against.",
            )
        )

    package_py = _find_package_py(root)
    if package_py is None:
        return findings
    requires = _package_py_requires(package_py)
    if requires is None:
        findings.append(
            Finding(
                "error",
                scope,
                "unparseable",
                f"{package_py.name} has no literal 'requires' list to read a package-environment requirement from",
            )
        )
        return findings

    observed_text = _core_requirement_from_package(requires, core_names)
    if observed_text is None:
        findings.append(
            Finding(
                "error",
                scope,
                "missing_package_environment_requirement",
                f"{package_py.name} does not require {import_name} at all, so the resolver picks any version it likes",
            )
        )
        return findings

    try:
        observed = module.requirement_from_package_environment(observed_text, package=import_name)
    except ValueError as exc:
        findings.append(Finding("error", scope, "unparseable", str(exc)))
        return findings

    try:
        expected = declared.to_package_environment()
    except ValueError as exc:
        findings.append(Finding("error", scope, "unparseable", str(exc)))
        return findings

    for code in module.compare_requirements(declared, observed):
        findings.append(
            Finding(
                "error",
                scope,
                code,
                f"{pyproject.name} declares '{declared_text}', but {package_py.name} declares "
                f"'{observed_text}'. Derive it instead: '{expected}'.",
            )
        )
    return findings


def check_contract(contract: dict[str, Any]) -> list[Finding]:
    """Report the adapters whose package-environment requirement is still open."""
    findings: list[Finding] = []
    for entry in contract.get("known_violations", []):
        if entry.get("status") != "open":
            continue
        scope = "{}-{}".format(entry.get("adapter"), entry.get("adapter_version"))
        codes = entry.get("codes") or ["open_ceiling"]
        findings.append(
            Finding(
                "warning",
                scope,
                ",".join(str(code) for code in codes),
                "package-environment requirement '{}' is wider than the declared '{}'; expected '{}'.".format(
                    entry.get("package_environment_requirement"),
                    entry.get("python_distribution_requirement"),
                    entry.get("expected_package_environment_requirement"),
                ),
            )
        )
    return findings


def build_parser() -> argparse.ArgumentParser:
    """Return the command-line parser for this gate."""
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument(
        "--adapter-root",
        action="append",
        default=[],
        type=Path,
        help="Adapter checkout to compare against the contract (repeatable).",
    )
    parser.add_argument("--root", type=Path, default=None, help="Repository root holding the contract.")
    parser.add_argument("--github", action="store_true", help="Emit GitHub workflow annotations.")
    parser.add_argument("--json", action="store_true", help="Emit findings as JSON.")
    return parser


def main(argv: list[str] | None = None) -> int:
    """Run the gate and return the process exit code."""
    args = build_parser().parse_args(argv)
    root = args.root or repository_root()

    try:
        contract = load_contract(root)
    except ContractError as exc:
        print(f"adapter-core-requirement contract is invalid: {exc}", file=sys.stderr)
        return 1

    module = core_requirement_module()
    findings = check_contract(contract)
    for adapter_root in args.adapter_root:
        if not adapter_root.is_dir():
            findings.append(Finding("error", str(adapter_root), "missing_root", "not a directory"))
            continue
        findings.extend(check_adapter_root(adapter_root, module, contract))

    if args.json:
        print(
            json.dumps(
                {
                    "contract": str(root / CONTRACT_RELATIVE_PATH),
                    "findings": [finding.as_dict() for finding in findings],
                },
                indent=2,
            )
        )
    else:
        for finding in findings:
            print(finding.render(github=args.github))
        errors = sum(1 for finding in findings if finding.severity == "error")
        warnings = sum(1 for finding in findings if finding.severity == "warning")
        print(
            "adapter-core-requirement: contract valid, {} error(s), {} warning(s), {} tracked violation(s)".format(
                errors, warnings, len(contract.get("known_violations", []))
            )
        )

    return 1 if any(finding.severity == "error" for finding in findings) else 0


if __name__ == "__main__":
    raise SystemExit(main())
