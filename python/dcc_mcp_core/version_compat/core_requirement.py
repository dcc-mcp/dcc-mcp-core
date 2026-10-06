"""Adapter to core version requirement contract.

Why this module exists
----------------------
An adapter declares the core version it supports **twice**, and only one of
the two declarations is enforced by a resolver:

* the Python distribution metadata (``Requires-Dist: dcc-mcp-core>=0.19.3,<0.19.5``),
  which ``pip`` honours when the adapter is installed from PyPI;
* the package-environment requirement (Rez: ``requires = ["dcc_mcp_core-0"]``
  in the adapter's ``package.py``), which the studio's environment resolver
  honours when it assembles the DCC runtime.

The second declaration is hand-maintained and, in practice, drifts to "any
0.x": 35 of 51 published adapter versions carried no upper bound at all. A
resolver that sees ``dcc_mcp_core-0`` is free to pick a core the adapter's own
PyPI metadata explicitly excludes — and it does. The resulting environment
imports cleanly at the top level and only fails deep inside
``dcc_mcp_core.dcc_server``, so the operator sees a circular-import
``ImportError`` instead of "this adapter needs core <0.19.5".

This module makes the two declarations comparable and gives adapters one
canonical way to derive the second from the first:

    >>> from dcc_mcp_core.core_requirement import requirement_from_pep440
    >>> requirement = requirement_from_pep440("dcc-mcp-core>=0.19.3,<0.19.5")
    >>> requirement.to_package_environment()
    'dcc_mcp_core-0.19.3..0.19.5'

Policy implemented here
-----------------------
1. **The Python distribution declaration is the source of truth.** Bounds are
   read from the adapter's own ``Requires-Dist`` metadata; the
   package-environment requirement is derived from it, never the reverse.
2. **An upper bound is mandatory.** :func:`check_requirement` reports a
   requirement with no upper bound even when the installed core happens to
   satisfy it — an unbounded request is a resolver bug waiting for the next
   core release.
3. **The upper bound is exclusive.** ``<0.19.5`` renders as the ``..0.19.5``
   range token. An inclusive upper bound (``<=0.19.5``) has no equivalent
   package-environment token, so :meth:`Requirement.to_package_environment`
   rejects it instead of silently widening the range.
4. **Drift is detected, not tolerated.** :func:`enforce_core_compatibility`
   raises :class:`CoreRequirementError` naming the adapter, its declared
   range, the running core version, and the exact remedy. Adapters call it
   while importing so the failure lands on the adapter's front door rather
   than on a deep core import.

The machine-readable half of the contract — the survey of adapters whose
package-environment requirement is missing its upper bound, plus the rules the
CI gate applies — lives in ``compatibility/adapter-core-requirement.json`` and
is enforced by ``scripts/ci/check_adapter_core_requirement.py``.

Python 3.7 support (Maya 2022)
------------------------------
Stdlib only. Version comparison prefers :mod:`packaging.version` when the host
provides it and falls back to a numeric release-segment comparison otherwise,
so the module imports inside a Maya 2022 interpreter with no extra wheels.
Distribution metadata is read through :mod:`importlib.metadata` where
available and through a ``sys.path`` scan otherwise, mirroring
:mod:`dcc_mcp_core._version_check`.
"""

from __future__ import annotations

# Import future modules
# Import built-in modules
import logging
import os
from pathlib import Path
import re

# Import local modules
from dcc_mcp_core import constants
from dcc_mcp_core._version_check import _metadata_module
from dcc_mcp_core._version_check import _scan_distribution_dirs

logger = logging.getLogger(__name__)

#: PyPI distribution name of the core package.
CORE_PACKAGE = "dcc-mcp-core"
#: Import name of the core package, used by package-environment requirements.
CORE_IMPORT_NAME = "dcc_mcp_core"
#: Set to ``0`` to downgrade :func:`enforce_core_compatibility` to a warning.
#:
#: The name itself is owned by :mod:`dcc_mcp_core.constants`, which is the only
#: place a runtime environment variable may be spelled out.
ENV_ENFORCE = constants.ENV_REQUIREMENT_ENFORCE

_SPEC_RE = re.compile(r"^(==|!=|~=|>=|<=|>|<)\s*([0-9][0-9A-Za-z.*+\-!]*)")
_VERSION_PREFIX_RE = re.compile(r"^(\d+(?:\.\d+)*)(.*)$")
_PRE_MARKERS = (
    ("dev", -3),
    ("alpha", -2),
    ("a", -2),
    ("beta", -1),
    ("b", -1),
    ("c", -1),
    ("pre", -1),
    ("preview", -1),
    ("rc", -1),
)
_POST_MARKERS = (("post", 1), ("rev", 1), ("r", 1))


class CoreRequirementError(RuntimeError):
    """Raised when the running core violates an adapter's declared requirement."""


def _packaging_version() -> object:
    """Return :class:`packaging.version.Version`, or ``None`` when unavailable.

    ``packaging`` is not a declared runtime dependency — adding one for a
    diagnostics-only feature would make every Maya 2022 install depend on a
    network-resolved wheel — so the import is best-effort and cached.
    """
    try:
        from packaging.version import Version
    except Exception:  # pragma: no cover - ImportError or a broken install
        return None
    return Version


def _release_segments(text: str) -> tuple[int, ...]:
    """Return the numeric release segments of *text* as a fixed-width tuple."""
    match = _VERSION_PREFIX_RE.match(text.strip().lstrip("vV"))
    if match is None:
        return (0,)
    segments = [int(part) for part in match.group(1).split(".") if part.isdigit()]
    if not segments:
        return (0,)
    while len(segments) < 4:
        segments.append(0)
    return tuple(segments[:4])


def _pre_marker_rank(letter: str) -> int:
    """Return the ordering rank of a PEP 440 pre-release *letter*."""
    for marker, rank in _PRE_MARKERS:
        if marker == letter:
            return rank
    return -1


def _marker_rank(rest: str) -> tuple[int, int]:
    """Return a pre/post-release rank for the non-numeric tail *rest*."""
    text = rest.lower().replace(".", "").replace("_", "").replace("-", "")
    for marker, rank in _PRE_MARKERS:
        index = text.find(marker)
        if index >= 0:
            digits = re.search(r"\d+", text[index + len(marker) :])
            return (rank, int(digits.group()) if digits else 0)
    for marker, rank in _POST_MARKERS:
        index = text.find(marker)
        if index >= 0:
            digits = re.search(r"\d+", text[index + len(marker) :])
            return (rank, int(digits.group()) if digits else 0)
    return (0, 0)


def _version_key(value: str) -> tuple[tuple[int, ...], tuple[int, int], str]:
    """Return a comparable key for *value*, preferring PEP 440 semantics.

    Every branch returns a ``(release, rank, local)`` triple built from the
    same types, so keys stay comparable even when one host has
    :mod:`packaging` and another does not.
    """
    text = str(value or "").strip().lstrip("vV")
    local = ""
    if "+" in text:
        text, local = text.split("+", 1)
    version_cls = _packaging_version()
    if version_cls is not None:
        try:
            parsed = version_cls(text)
            rank = (0, 0)
            if parsed.is_devrelease:
                rank = (-3, int(parsed.dev or 0))
            elif parsed.is_prerelease and parsed.pre is not None:
                letter = str(parsed.pre[0]).lower()
                rank = (_pre_marker_rank(letter), int(parsed.pre[1] or 0))
            elif parsed.is_postrelease:
                rank = (1, int(parsed.post or 0))
            return (tuple(parsed.release), rank, str(parsed.local or "").lower())
        except Exception:  # pragma: no cover - InvalidVersion falls through
            pass
    match = _VERSION_PREFIX_RE.match(text)
    rest = match.group(2) if match is not None else text
    return (_release_segments(text), _marker_rank(rest), local.lower())


def compare_versions(left: str, right: str) -> int:
    """Return ``-1``, ``0`` or ``1`` ordering *left* against *right*."""
    left_key = _version_key(left)
    right_key = _version_key(right)
    if left_key < right_key:
        return -1
    if left_key > right_key:
        return 1
    return 0


def _canonical(version_text: str) -> str:
    """Return *version_text* trimmed to the dotted-numeric release prefix."""
    match = _VERSION_PREFIX_RE.match(str(version_text).strip().lstrip("vV"))
    if match is None:
        return str(version_text).strip()
    return match.group(1)


def package_environment_name(package: str) -> str:
    """Return the package-environment spelling of a distribution *package*.

    Resolvers address packages by import name, so ``dcc-mcp-core`` renders as
    ``dcc_mcp_core`` regardless of which spelling the requirement was parsed
    from.
    """
    return str(package).strip().replace("-", "_")


def _prefix_range(version_text: str) -> tuple[str, str]:
    """Return the ``(lower, upper)`` pair implied by a partial version prefix.

    ``0`` widens to ``>=0.0.0,<1.0.0`` and ``0.20`` to ``>=0.20.0,<0.21.0``,
    matching how a package-environment resolver reads a truncated version.
    """
    cleaned = str(version_text).strip().rstrip("*").rstrip(".")
    segments = [part for part in cleaned.split(".") if part.isdigit()]
    if not segments:
        raise ValueError(f"cannot derive a version range from {version_text!r}")
    numbers = [int(part) for part in segments]
    lower = list(numbers)
    while len(lower) < 3:
        lower.append(0)
    upper = list(numbers[:-1])
    upper.append(numbers[-1] + 1)
    while len(upper) < 3:
        upper.append(0)
    return (".".join(str(part) for part in lower), ".".join(str(part) for part in upper))


def _compatible_release_range(version_text: str) -> tuple[str, str]:
    """Return the ``(lower, upper)`` pair implied by a ``~=`` specifier."""
    cleaned = str(version_text).strip().rstrip("*").rstrip(".")
    segments = [part for part in cleaned.split(".") if part.isdigit()]
    if len(segments) < 2:
        raise ValueError(f"'~=' requires at least a major.minor version, got {version_text!r}")
    lower = [int(part) for part in segments]
    while len(lower) < 3:
        lower.append(0)
    upper = [int(segments[0]), int(segments[1]) + 1, 0]
    return (".".join(str(part) for part in lower), ".".join(str(part) for part in upper))


class Requirement:
    """Inclusive/exclusive bounds an adapter declares against core.

    ``lower``/``upper`` are plain version strings; the ``*_inclusive`` flags
    record whether the bound itself is part of the range. ``raw`` keeps the
    text the requirement was parsed from so error messages can quote it.
    """

    __slots__ = ("lower", "lower_inclusive", "package", "raw", "upper", "upper_inclusive")

    def __init__(
        self,
        package: str = CORE_IMPORT_NAME,
        lower: str | None = None,
        upper: str | None = None,
        lower_inclusive: bool = True,
        upper_inclusive: bool = False,
        raw: str = "",
    ) -> None:
        """Store one core requirement.

        The package is normalised to its import spelling so the two syntaxes
        produce requirements that compare equal: ``dcc-mcp-core>=0.19.3`` and
        ``dcc_mcp_core-0.19.3..`` describe the same contract.
        """
        self.package = package_environment_name(package)
        self.lower = lower
        self.upper = upper
        self.lower_inclusive = lower_inclusive
        self.upper_inclusive = upper_inclusive
        self.raw = raw

    # -- rendering ---------------------------------------------------------

    def is_bounded(self) -> bool:
        """Return ``True`` when the requirement constrains core from above."""
        return bool(self.upper)

    def contains(self, version: str) -> bool:
        """Return ``True`` when *version* satisfies both bounds."""
        if self.lower:
            order = compare_versions(version, self.lower)
            if order < 0 or (order == 0 and not self.lower_inclusive):
                return False
        if self.upper:
            order = compare_versions(version, self.upper)
            if order > 0 or (order == 0 and not self.upper_inclusive):
                return False
        return True

    def to_pep440(self) -> str:
        """Return the requirement as a PEP 440 specifier set."""
        name = self.package.replace("_", "-")
        clauses = []
        if self.lower:
            clauses.append(f">={self.lower}" if self.lower_inclusive else f">{self.lower}")
        if self.upper:
            clauses.append(f"<={self.upper}" if self.upper_inclusive else f"<{self.upper}")
        if not clauses:
            return name
        return "{}{}".format(name, ",".join(clauses))

    def to_package_environment(self) -> str:
        """Return the requirement as a package-environment range token.

        Renders ``>=0.19.3,<0.19.5`` as ``dcc_mcp_core-0.19.3..0.19.5``. An
        inclusive upper bound has no equivalent token, so it is rejected
        rather than silently widened — the calling adapter has to restate the
        bound as ``<X.Y.Z`` first.
        """
        if self.upper and self.upper_inclusive:
            raise ValueError(
                "cannot render an inclusive upper bound as a package-environment "
                f"requirement; restate {self.raw or self.to_pep440()!r} as '<{self.upper}'."
            )
        name = package_environment_name(self.package)
        if self.lower and self.upper:
            return f"{name}-{self.lower}..{self.upper}"
        if self.lower:
            return f"{name}-{self.lower}+"
        if self.upper:
            return f"{name}-..{self.upper}"
        return name

    def __eq__(self, other: object) -> bool:
        """Return ``True`` when two requirements describe the same bounds."""
        if not isinstance(other, Requirement):
            return NotImplemented
        return (
            self.package == other.package
            and self.lower == other.lower
            and self.upper == other.upper
            and self.lower_inclusive == other.lower_inclusive
            and self.upper_inclusive == other.upper_inclusive
        )

    def __hash__(self) -> int:
        """Return a hash consistent with :meth:`__eq__`."""
        return hash(
            (self.package, self.lower, self.upper, self.lower_inclusive, self.upper_inclusive),
        )

    def __repr__(self) -> str:
        """Return a debugging representation."""
        return f"Requirement({self.to_pep440()!r})"


def requirement_from_pep440(specifier: str, package: str = CORE_PACKAGE) -> Requirement:
    """Parse a PEP 440 core requirement into :class:`Requirement`.

    Accepts a bare specifier set (``">=0.19.3,<0.19.5"``) or a full
    requirement string (``"dcc-mcp-core>=0.19.3,<0.19.5"``). ``!=`` clauses
    are ignored: they narrow a range without bounding it in either direction.
    """
    text = str(specifier or "").strip()
    if not text:
        raise ValueError("empty core requirement")
    body = text
    normalized_package = package.replace("_", "-").lower()
    head_match = re.match(r"^([A-Za-z0-9._\-]+)\s*(?=[<>=!~])", body)
    if head_match is not None and head_match.group(1).replace("_", "-").lower() == normalized_package:
        body = body[head_match.end() :]

    requirement = Requirement(package=package, raw=text)
    for clause in body.split(","):
        clause = clause.strip()
        if not clause:
            continue
        match = _SPEC_RE.match(clause)
        if match is None:
            raise ValueError(f"unsupported core requirement clause: {clause!r}")
        operator, version = match.group(1), match.group(2)
        if operator == "!=":
            continue
        if operator == ">=":
            if requirement.lower is None or compare_versions(version, requirement.lower) > 0:
                requirement.lower, requirement.lower_inclusive = _canonical(version), True
        elif operator == ">":
            if requirement.lower is None or compare_versions(version, requirement.lower) >= 0:
                requirement.lower, requirement.lower_inclusive = _canonical(version), False
        elif operator == "<=":
            if requirement.upper is None or compare_versions(version, requirement.upper) < 0:
                requirement.upper, requirement.upper_inclusive = _canonical(version), True
        elif operator == "<":
            if requirement.upper is None or compare_versions(version, requirement.upper) <= 0:
                requirement.upper, requirement.upper_inclusive = _canonical(version), False
        elif operator == "==":
            if version.rstrip("*").endswith(".") or "*" in version:
                lower, upper = _prefix_range(version)
                requirement.lower, requirement.lower_inclusive = lower, True
                requirement.upper, requirement.upper_inclusive = upper, False
            else:
                canonical = _canonical(version)
                requirement.lower, requirement.lower_inclusive = canonical, True
                requirement.upper, requirement.upper_inclusive = canonical, True
        elif operator == "~=":
            lower, upper = _compatible_release_range(version)
            requirement.lower, requirement.lower_inclusive = lower, True
            requirement.upper, requirement.upper_inclusive = upper, False
    return requirement


def requirement_from_package_environment(token: str, package: str = CORE_IMPORT_NAME) -> Requirement:
    """Parse a package-environment requirement token into :class:`Requirement`.

    Recognises the four shapes an adapter can write::

        dcc_mcp_core              any version
        dcc_mcp_core-0            any 0.x  (prefix floor)
        dcc_mcp_core-0.19.3+      at least 0.19.3
        dcc_mcp_core-0.19.3..0.19.5   at least 0.19.3, below 0.19.5
    """
    text = str(token or "").strip()
    if not text:
        raise ValueError("empty package-environment requirement")
    name, _, range_text = text.partition("-")
    name = name.strip()
    if not name:
        raise ValueError(f"package-environment requirement is missing a package name: {token!r}")
    requirement = Requirement(package=name, raw=text)
    if not range_text:
        return requirement

    if range_text.endswith("+"):
        requirement.lower, requirement.lower_inclusive = _canonical(range_text[:-1]), True
        return requirement
    if ".." in range_text:
        lower_text, _, upper_text = range_text.partition("..")
        if lower_text:
            requirement.lower, requirement.lower_inclusive = _canonical(lower_text), True
        if upper_text:
            requirement.upper, requirement.upper_inclusive = _canonical(upper_text), False
        return requirement
    lower, upper = _prefix_range(range_text)
    requirement.lower, requirement.lower_inclusive = lower, True
    requirement.upper, requirement.upper_inclusive = upper, False
    return requirement


def requirement_from_text(value: str) -> Requirement:
    """Parse a requirement written in either syntax.

    A value containing a PEP 440 comparison operator is read as Python
    distribution metadata; anything else is read as a package-environment
    token.
    """
    text = str(value or "").strip()
    if re.search(r"[<>=!~]=?", text):
        return requirement_from_pep440(text)
    return requirement_from_package_environment(text)


def _requires_from_api(package: str) -> list[str] | None:
    """Return the ``Requires-Dist`` entries of *package* via importlib.metadata."""
    metadata = _metadata_module()
    if metadata is None:
        return None
    try:
        requires = metadata.requires(package)
    except Exception:
        return None
    if not requires:
        return None
    return [str(entry) for entry in requires]


def _requires_from_path_scan(package: str) -> list[str]:
    """Return the ``Requires-Dist`` entries of *package* by scanning ``sys.path``.

    Python 3.7 (Maya 2022) has no :mod:`importlib.metadata` and the backport is
    not a declared dependency, so the metadata file is read directly — the same
    fallback :mod:`dcc_mcp_core._version_check` uses to read ``Version:``.

    The scan walks every ``sys.path`` entry. That is cheap once in a release
    gate and not cheap enough for a server start, so
    :func:`requirement_for_distribution` only reaches it when
    ``allow_path_scan`` is enabled.
    """
    entries: list[str] = []
    for metadata_file, _distribution_path in _scan_distribution_dirs(package):
        try:
            with Path(metadata_file).open(encoding="utf-8", errors="replace") as handle:
                for line in handle:
                    if line.lower().startswith("requires-dist:"):
                        value = line.split(":", 1)[1].strip()
                        if value:
                            entries.append(value)
        except OSError:
            continue
        if entries:
            break
    return entries


def distribution_requires(package: str, allow_path_scan: bool = True) -> list[str]:
    """Return the declared ``Requires-Dist`` entries of *package*.

    Returns an empty list when the distribution is not installed or its
    metadata cannot be read — callers treat "unknown" as "nothing to check"
    rather than as a violation. Set *allow_path_scan* to ``False`` to skip the
    ``sys.path`` fallback and only use :mod:`importlib.metadata`.
    """
    requires = _requires_from_api(package)
    if requires is None and allow_path_scan:
        requires = _requires_from_path_scan(package)
    if not requires:
        return []
    # ``Requires-Dist`` entries for optional extras cannot bound a plain install.
    return [entry for entry in requires if "extra ==" not in entry]


def requirement_for_distribution(
    package: str,
    core_package: str = CORE_PACKAGE,
    allow_path_scan: bool = True,
) -> Requirement | None:
    """Return the core requirement declared by the distribution *package*.

    Accepts either spelling of either name (``dcc-mcp-maya`` /
    ``dcc_mcp_maya``). Returns ``None`` when the distribution is absent or
    does not declare a core dependency at all.
    """
    candidates = {package, package.replace("_", "-"), package.replace("-", "_")}
    core_names = {
        core_package,
        core_package.replace("_", "-"),
        core_package.replace("-", "_"),
    }
    for candidate in sorted(candidates):
        for entry in distribution_requires(candidate, allow_path_scan=allow_path_scan):
            name = re.split(r"[\s\[<>=!~;]", entry, maxsplit=1)[0].strip()
            if name.replace("_", "-").lower() in {value.replace("_", "-").lower() for value in core_names}:
                return requirement_from_pep440(entry, package=name)
    return None


def runtime_core_version() -> str:
    """Return the version of the executing core, or ``""`` when unknown."""
    try:
        from dcc_mcp_core._version_check import distribution_version
        from dcc_mcp_core._version_check import native_version
    except Exception:  # pragma: no cover - defensive: same package
        return ""
    # The compiled extension is the artifact that executes, so it wins; the
    # installed distribution metadata is the fallback for a pure-Python wheel.
    return native_version(load=True) or distribution_version() or ""


def check_requirement(
    requirement: Requirement,
    core_version: str | None = None,
) -> list[str]:
    """Return the contract violations in *requirement* against *core_version*.

    Two violations are reported:

    ``unbounded``
        The requirement has no upper bound. Reported even when the running
        core satisfies the lower bound: an unbounded request is what let a
        resolver pair ``dcc-mcp-maya 0.9.4`` with core ``0.20.28``.

    ``out_of_range``
        The running core falls outside the declared range.
    """
    violations: list[str] = []
    if not requirement.is_bounded():
        violations.append("unbounded")
    if core_version and not requirement.contains(core_version):
        violations.append("out_of_range")
    return violations


def compare_requirements(declared: Requirement, observed: Requirement) -> list[str]:
    """Return the ways *observed* is wider than the *declared* requirement.

    ``declared`` is the requirement from the adapter's Python distribution
    metadata (what ``pip`` enforces); ``observed`` is what the
    package-environment resolver sees. A resolver only ever needs a range at
    least as wide as the declared one, so every problem reported here is a
    range the resolver may pick a core release from that ``pip`` would have
    rejected:

    ``missing_upper_bound``
        The observed requirement has no upper bound at all.

    ``upper_bound_too_high``
        The observed ceiling sits above the declared one (``<1.0.0`` against a
        declared ``<0.19.5``, for example).

    ``lower_bound_too_low``
        The observed floor sits below the declared one.
    """
    problems: list[str] = []
    if declared.lower:
        lower_order = -1 if observed.lower is None else compare_versions(observed.lower, declared.lower)
        narrower_floor = lower_order == 0 and declared.lower_inclusive and not observed.lower_inclusive
        if lower_order < 0 or narrower_floor:
            problems.append("lower_bound_too_low")
    if observed.upper is None:
        problems.append("missing_upper_bound")
    elif declared.upper is not None:
        upper_order = compare_versions(observed.upper, declared.upper)
        inclusive_ceiling = upper_order == 0 and observed.upper_inclusive and not declared.upper_inclusive
        if upper_order > 0 or inclusive_ceiling:
            problems.append("upper_bound_too_high")
    return problems


def open_ceiling(requirement: Requirement) -> bool:
    """Return ``True`` when *requirement*'s ceiling is the open ``1.0.0`` guard.

    Core is still ``0.x``, so ``<1.0.0`` excludes nothing that exists. It is a
    place-holder adapters copy from the compatibility matrix, not a version
    they verified against.
    """
    return requirement.upper is not None and _canonical(requirement.upper).startswith("1.")


def describe_requirement_problem(
    requirement: Requirement,
    core_version: str,
    adapter: str = "",
) -> str:
    """Return one actionable sentence for a failing requirement check."""
    subject = adapter or requirement.package
    declared = requirement.raw or requirement.to_pep440()
    if not requirement.is_bounded():
        return f"{subject} declares '{declared}', which has no upper bound"
    if core_version:
        return f"{subject} declares '{declared}', but core {core_version} is installed"
    return f"{subject} declares '{declared}'"


def enforce_core_compatibility(
    requirement: Requirement | str | None = None,
    adapter: str | None = None,
    core_version: str | None = None,
) -> Requirement | None:
    """Fail fast when the running core is outside an adapter's declared range.

    *requirement* accepts a :class:`Requirement`, a PEP 440 specifier set, or a
    package-environment token. When it is ``None``, the requirement is read
    from the ``Requires-Dist`` metadata of *adapter* (a distribution name such
    as ``dcc-mcp-maya`` or an import name such as ``dcc_mcp_maya``).

    Returns the resolved requirement, or ``None`` when no requirement could be
    determined — an unknown requirement is not a violation.

    Set ``DCC_MCP_CORE_REQUIREMENT_ENFORCE=0`` to downgrade the failure to a
    warning. Adapters that must keep a broken pairing running can opt out, but
    the default is to raise :class:`CoreRequirementError`, because the
    alternative is a circular-import ``ImportError`` raised several modules
    deeper with no mention of the version that caused it.
    """
    resolved: Requirement | None
    if isinstance(requirement, Requirement):
        resolved = requirement
    elif isinstance(requirement, str) and requirement.strip():
        resolved = requirement_from_text(requirement)
    elif adapter:
        resolved = requirement_for_distribution(adapter)
    else:
        resolved = None
    if resolved is None:
        return None

    running = core_version or runtime_core_version()
    violations = check_requirement(resolved, running)
    if not violations:
        return resolved

    message = describe_requirement_problem(resolved, running, adapter or "")
    remedy = f"Install a core release matching '{resolved.to_pep440()}'"
    if "unbounded" in violations:
        remedy += f", and restate the package-environment requirement as '{_unbounded_remedy(resolved)}'"
    if os.environ.get(ENV_ENFORCE, "1").strip().lower() in {"0", "false", "no", "off"}:
        logger.warning("%s %s. Continuing because %s=0.", message, remedy, ENV_ENFORCE)
        return resolved
    raise CoreRequirementError(f"{message} {remedy}")


def _unbounded_remedy(requirement: Requirement) -> str:
    """Return the package-environment token a bounded requirement would use."""
    bounded = Requirement(
        package=requirement.package,
        lower=requirement.lower,
        upper=requirement.upper or "1.0.0",
        lower_inclusive=requirement.lower_inclusive,
        upper_inclusive=False,
        raw=requirement.raw,
    )
    return bounded.to_package_environment()


def package_environment_requirement_for(package: str) -> str | None:
    """Return the package-environment token implied by *package*'s metadata.

    Adapters call this from their release tooling so the ``package.py``
    requirement is generated from the PyPI declaration instead of being typed
    by hand. Returns ``None`` when the distribution declares no core
    dependency.
    """
    requirement = requirement_for_distribution(package)
    if requirement is None:
        return None
    try:
        return requirement.to_package_environment()
    except ValueError:
        return None
