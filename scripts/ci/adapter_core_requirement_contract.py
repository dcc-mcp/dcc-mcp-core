"""Shared loader and invariants for the adapter-to-core requirement contract.

The contract in ``compatibility/adapter-core-requirement.json`` is the
machine-readable half of :mod:`dcc_mcp_core.version_compat.core_requirement`: it records the
rules the gate applies and the adapters whose package-environment requirement
is known to be wider than the range their PyPI metadata declares.

Keeping the loader separate from the gate mirrors
``scripts/ci/python_support_contract.py``, so tests can assert the invariants
without going through argument parsing.
"""

from __future__ import annotations

import json
from pathlib import Path
import sys
from typing import Any

CONTRACT_RELATIVE_PATH = Path("compatibility") / "adapter-core-requirement.json"


class ContractError(ValueError):
    """Raised when the adapter-to-core requirement contract is malformed."""


def repository_root() -> Path:
    """Return the repository root for scripts executed from any directory."""
    return Path(__file__).resolve().parents[2]


def core_requirement_module() -> Any:
    """Return :mod:`dcc_mcp_core.version_compat.core_requirement`.

    Imported normally first, because an adapter running this gate in its own CI
    has ``dcc-mcp-core`` installed as a dependency. When the import fails — a
    bare core checkout with no install — the module is loaded from the source
    file instead, so the gate never depends on a build step.
    """
    import importlib.util

    try:
        import dcc_mcp_core.version_compat.core_requirement as module

        return module
    except ImportError:
        pass

    root = repository_root()
    source_root = root / "python"
    source = source_root / "dcc_mcp_core" / "version_compat" / "core_requirement.py"
    if not source.is_file():
        raise ContractError(
            f"dcc_mcp_core.version_compat.core_requirement is not installed and {source} is missing"
        )
    if str(source_root) not in sys.path:
        sys.path.insert(0, str(source_root))
    # The failed import above cached the *package* from wherever it was found,
    # and a child import later resolves against that package's ``__path__`` —
    # an installed wheel would shadow the checkout this gate has to validate.
    # Drop the cached entries that were loaded from somewhere else first.
    for name in list(sys.modules):
        if name != "dcc_mcp_core" and not name.startswith("dcc_mcp_core."):
            continue
        origin = str(getattr(sys.modules[name], "__file__", "") or "")
        if origin and not origin.startswith(str(source_root)):
            del sys.modules[name]
    spec = importlib.util.spec_from_file_location("_dcc_mcp_core_requirement_for_ci", source)
    if spec is None or spec.loader is None:  # pragma: no cover - defensive
        raise ContractError(f"cannot load {source}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def load_contract(root: Path | None = None) -> dict[str, Any]:
    """Load and validate the machine-readable requirement contract."""
    repo_root = Path(root) if root is not None else repository_root()
    path = repo_root / CONTRACT_RELATIVE_PATH
    try:
        contract = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError) as exc:
        raise ContractError(f"cannot load {path}: {exc}") from exc
    validate_contract(contract)
    return contract


def validate_contract(contract: dict[str, Any]) -> None:
    """Validate internal relationships before consumers use the contract."""
    if contract.get("schema_version") != 1:
        raise ContractError("schema_version must be 1")
    if contract.get("contract_id") != "adapter-core-requirement":
        raise ContractError("contract_id must be adapter-core-requirement")

    try:
        policy = contract["policy"]
        syntax = contract["syntax"]
        checks = contract["checks"]
        violations = contract["known_violations"]
        package = contract["core_package"]
        import_name = contract["core_import_name"]
    except (KeyError, TypeError) as exc:
        raise ContractError(f"missing required contract field: {exc}") from exc

    if import_name != package.replace("-", "_"):
        raise ContractError("core_import_name must be the underscore form of core_package")
    if policy.get("source_of_truth") != "python_distribution_metadata":
        raise ContractError("the Python distribution metadata must remain the source of truth")
    if policy.get("upper_bound_required") is not True:
        raise ContractError("policy.upper_bound_required must be true")
    if policy.get("upper_bound_exclusive") is not True:
        raise ContractError("policy.upper_bound_exclusive must be true")
    if policy.get("package_environment_derived") is not True:
        raise ContractError("the package-environment requirement must be derived, never hand-written")

    open_ceiling = policy.get("open_ceiling") or {}
    if not str(open_ceiling.get("value", "")).startswith("1."):
        raise ContractError("policy.open_ceiling.value must name the 1.0.0 placeholder")
    if open_ceiling.get("severity") not in {"warning", "error"}:
        raise ContractError("policy.open_ceiling.severity must be warning or error")

    for name in ("python_distribution", "package_environment"):
        if not (syntax.get(name) or {}).get("grammar"):
            raise ContractError(f"syntax.{name} must declare a grammar")

    severities = {"warning", "error"}
    for name, check in checks.items():
        if check.get("severity") not in severities:
            raise ContractError(f"checks.{name} must declare a warning or error severity")

    module = core_requirement_module()
    seen: set[tuple[str, str]] = set()
    for entry in violations:
        _validate_violation(entry, module, seen)


def _validate_violation(entry: Any, module: Any, seen: set[tuple[str, str]]) -> None:
    """Validate one ``known_violations`` row against the shared parser."""
    if not isinstance(entry, dict):
        raise ContractError("known_violations entries must be objects")
    try:
        adapter = entry["adapter"]
        version = entry["adapter_version"]
        declared = entry["python_distribution_requirement"]
        observed = entry["package_environment_requirement"]
        expected = entry["expected_package_environment_requirement"]
        codes = entry["codes"]
    except KeyError as exc:
        raise ContractError(f"known_violations entry is missing {exc!s}") from exc

    key = (str(adapter), str(version))
    if key in seen:
        raise ContractError(f"duplicate known_violations entry for {adapter}-{version}")
    seen.add(key)

    if entry.get("status") not in {"open", "fixed"}:
        raise ContractError(f"known_violations[{key}].status must be open or fixed")

    declared_requirement = module.requirement_from_pep440(str(declared))
    derived = declared_requirement.to_package_environment()
    if derived != expected:
        raise ContractError(
            f"{adapter}-{version}: expected_package_environment_requirement is {expected!r} "
            f"but the parser derives {derived!r}"
        )
    observed_requirement = module.requirement_from_package_environment(str(observed))
    recomputed = module.compare_requirements(declared_requirement, observed_requirement)
    if list(codes) != recomputed:
        raise ContractError(
            f"{adapter}-{version}: codes {codes!r} do not match the recomputed drift {recomputed!r}"
        )
