"""Version-bound contract for adapter dependency declarations on ``dcc-mcp-core``.

Why this module exists
----------------------
``dcc-mcp-core`` ships ``0.MINOR.PATCH`` releases, so **every minor bump may
break an adapter**. Adapters publish a bounded range on PyPI
(``dcc-mcp-core>=0.19.3,<0.19.5``), but the same adapter has historically been
requested from package environments as ``dcc_mcp_core-0`` — an "any 0.x"
request with no usable floor and a ceiling five minor lines away. Resolvers
happily pick such a core, the top-level imports still succeed, and the failure
only surfaces deep inside server startup.

This module turns that observation into a checkable contract:

1. A declaration must carry a lower **and** an upper bound
   (:data:`REQUIRE_LOWER_BOUND` / :data:`REQUIRE_UPPER_BOUND`).
2. The upper bound must not admit more than :data:`MAX_MINOR_LINES` minor
   line(s) — one by default.
3. When only a minimum core version is known, :func:`derive_requirement`
   produces the canonical bounded range (``0.19.3`` → ``>=0.19.3,<0.20.0``).
4. :func:`check_runtime` compares a declaration against the core that is
   actually executing, so an out-of-range combination is reported where an
   operator can act on it instead of at an unrelated import site.
5. :func:`compare_declarations` catches the reported failure directly: a
   package-environment request that admits versions the packaging metadata
   excludes.

The machine-readable copy of the policy lives in
``compatibility/core-bounds.json``; ``tests/test_core_bounds.py`` keeps this
module, that file, and the Rust implementation in sync.

Declaration forms
-----------------
:func:`parse_requirement` accepts both forms an adapter publishes:

* **Packaging requirements** — a PEP 440 specifier set, optionally prefixed
  with the distribution name and suffixed with a marker:
  ``dcc-mcp-core>=0.19.3,<0.19.5``, ``dcc_mcp_core~=0.20.14``,
  ``>=0.19.3,<0.19.5; python_version >= "3.8"``.
* **Package-environment requests** — ``<name>-<range>`` where the range is a
  version prefix (``dcc_mcp_core-0``, ``dcc_mcp_core-0.20``) or a ``..`` pair
  (``dcc_mcp_core-0.19.3..0.20.0``; lower inclusive, upper exclusive). A bare
  name (``dcc_mcp_core``) is unbounded.

Python 3.7 support (Maya 2022)
------------------------------
Stdlib-only and import-light. Reading an installed distribution's metadata
prefers :mod:`importlib.metadata` (3.8+) or the ``importlib_metadata``
backport, and falls back to a stdlib scan of ``sys.path`` for
``<package>-*.dist-info/METADATA`` so the check still runs on the py3.7 hosts
that carry the oldest installs.
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
from pathlib import Path
import re
import sys
from typing import Dict
from typing import List
from typing import NamedTuple
from typing import Optional
from typing import Sequence
from typing import Tuple

__all__ = [
    "ALIGNED",
    "CODE_DESCRIPTIONS",
    "CODE_INVERTED_RANGE",
    "CODE_MISSING_LOWER_BOUND",
    "CODE_MISSING_UPPER_BOUND",
    "CODE_UNPARSABLE_DECLARATION",
    "CODE_UNSUPPORTED_SPECIFIER",
    "CODE_UPPER_BOUND_TOO_WIDE",
    "CORE_DISTRIBUTION",
    "CORE_IMPORT_NAME",
    "DECLARATION_UNUSABLE",
    "ENVIRONMENT_WIDER",
    "MAX_MINOR_LINES",
    "PACKAGING_WIDER",
    "REQUIRE_LOWER_BOUND",
    "REQUIRE_UPPER_BOUND",
    "VERDICT_CORE_NEWER_THAN_DECLARED",
    "VERDICT_CORE_OLDER_THAN_DECLARED",
    "VERDICT_DECLARATION_UNUSABLE",
    "VERDICT_SUPPORTED",
    "VERDICT_UNKNOWN_CORE_VERSION",
    "CoreRequirement",
    "CoreVersion",
    "check_runtime",
    "compare_declarations",
    "derive_requirement",
    "evaluate",
    "installed_core_requirement",
    "parse_requirement",
]

CORE_DISTRIBUTION = "dcc-mcp-core"
CORE_IMPORT_NAME = "dcc_mcp_core"

# Policy mirrors `compatibility/core-bounds.json` -> adapter_requirement_policy.
REQUIRE_LOWER_BOUND = True
REQUIRE_UPPER_BOUND = True
MAX_MINOR_LINES = 1

CODE_MISSING_LOWER_BOUND = "missing_lower_bound"
CODE_MISSING_UPPER_BOUND = "missing_upper_bound"
CODE_UPPER_BOUND_TOO_WIDE = "upper_bound_too_wide"
CODE_UNSUPPORTED_SPECIFIER = "unsupported_specifier"
CODE_UNPARSABLE_DECLARATION = "unparsable_declaration"
CODE_INVERTED_RANGE = "inverted_range"

CODE_DESCRIPTIONS: Dict[str, str] = {
    CODE_MISSING_LOWER_BOUND: "declares no lower bound, so any older core release resolves",
    CODE_MISSING_UPPER_BOUND: "declares no upper bound, so any newer core release resolves",
    CODE_UPPER_BOUND_TOO_WIDE: (
        "upper bound admits more than one core minor line, and every minor bump may break an adapter"
    ),
    CODE_UNSUPPORTED_SPECIFIER: "contains a specifier that cannot be interpreted",
    CODE_UNPARSABLE_DECLARATION: "is empty or carries no readable version range",
    CODE_INVERTED_RANGE: "admits no core release at all",
}

VERDICT_SUPPORTED = "supported"
VERDICT_CORE_NEWER_THAN_DECLARED = "core_newer_than_declared"
VERDICT_CORE_OLDER_THAN_DECLARED = "core_older_than_declared"
VERDICT_DECLARATION_UNUSABLE = "declaration_unusable"
VERDICT_UNKNOWN_CORE_VERSION = "unknown_core_version"

ALIGNED = "aligned"
ENVIRONMENT_WIDER = "environment_wider"
PACKAGING_WIDER = "packaging_wider"
DECLARATION_UNUSABLE = "declaration_unusable"

_VERSION_PREFIX = re.compile(r"^(\d+)(?:\.(\d+))?(?:\.(\d+))?")
_OPERATORS = ("===", "==", "!=", "~=", "<=", ">=", "<", ">")
# The version range of a `<name>-<range>` package-environment request, anchored
# at the end so a name that itself contains a dash (`dcc-mcp-core`) keeps it.
_REQUEST_SUFFIX = re.compile(r"-\d[\d.]*(?:\.\.\d[\d.]*)?$")


class CoreVersion(NamedTuple):
    """A ``MAJOR.MINOR.PATCH`` core version."""

    major: int
    minor: int
    patch: int

    @classmethod
    def parse(cls, text: str) -> Optional[CoreVersion]:
        """Parse ``0.19.3``, ``0.20``, ``0``, ``v0.19.3``, or ``0.19.3rc1``.

        Missing components default to ``0``; pre-release and build suffixes are
        dropped. Returns ``None`` for anything that is not numeric.
        """
        candidate = str(text).strip().lstrip("vV")
        candidate = re.split(r"[-+]", candidate, maxsplit=1)[0]
        match = _VERSION_PREFIX.match(candidate)
        if match is None:
            return None
        return cls(
            int(match.group(1)),
            int(match.group(2) or 0),
            int(match.group(3) or 0),
        )

    def next_minor(self) -> CoreVersion:
        """Return the first version of the next minor line (``0.19.3`` → ``0.20.0``)."""
        return CoreVersion(self.major, self.minor + 1, 0)

    def minor_line_limit(self, lines: int = MAX_MINOR_LINES) -> CoreVersion:
        """Return the exclusive upper bound that admits ``lines`` minor lines."""
        return CoreVersion(self.major, self.minor + max(1, int(lines)), 0)

    def minor_line(self) -> str:
        """Return the ``MAJOR.MINOR`` line this version belongs to."""
        return f"{self.major}.{self.minor}"

    def __str__(self) -> str:
        return f"{self.major}.{self.minor}.{self.patch}"


class CoreRequirement:
    """A parsed dependency range on :data:`CORE_DISTRIBUTION`."""

    __slots__ = ("lower", "unsupported", "upper")

    def __init__(
        self,
        lower: Optional[Tuple[CoreVersion, bool]] = None,
        upper: Optional[Tuple[CoreVersion, bool]] = None,
        unsupported: Optional[Sequence[str]] = None,
    ) -> None:
        # A bound is a `(version, exclusive)` pair: `exclusive` is True for
        # `>` / `<` and False for `>=` / `<=`.
        self.lower = lower
        self.upper = upper
        self.unsupported: List[str] = list(unsupported or ())

    def is_unbounded(self) -> bool:
        """Return True when the declaration carried no readable bound at all."""
        return self.lower is None and self.upper is None and not self.unsupported

    def contains(self, version: CoreVersion) -> bool:
        """Return True when ``version`` falls inside the declared range.

        Unknown specifiers are ignored here; call :func:`evaluate` to learn that
        the declaration is unusable.
        """
        if self.lower is not None:
            lower, exclusive = self.lower
            if version < lower or (exclusive and version == lower):
                return False
        if self.upper is not None:
            upper, exclusive = self.upper
            if version > upper or (exclusive and version == upper):
                return False
        return True

    def to_spec(self) -> str:
        """Return the canonical specifier set (``">=0.19.3,<0.20.0"``)."""
        parts: List[str] = []
        if self.lower is not None:
            version, exclusive = self.lower
            parts.append(f">{version}" if exclusive else f">={version}")
        if self.upper is not None:
            version, exclusive = self.upper
            parts.append(f"<{version}" if exclusive else f"<={version}")
        return ",".join(parts) if parts else "*"

    def __str__(self) -> str:
        return self.to_spec()

    def __repr__(self) -> str:  # pragma: no cover - debugging aid
        return f"CoreRequirement({self.to_spec()!r})"

    def __eq__(self, other: object) -> bool:
        if not isinstance(other, CoreRequirement):
            return NotImplemented
        return self.lower == other.lower and self.upper == other.upper and self.unsupported == other.unsupported


def parse_requirement(declaration: str) -> CoreRequirement:
    """Parse a packaging requirement or a package-environment request."""
    text = str(declaration).strip()
    if not text:
        # Callers distinguish an empty declaration from an unreadable one
        # through `evaluate()`; there is no specifier to report here.
        return CoreRequirement()
    if any(operator in text for operator in (">", "<", "=", "!", "~")):
        return _parse_specifier_set(text)
    return _parse_package_environment(text)


def evaluate(
    declaration: str,
    require_lower_bound: bool = REQUIRE_LOWER_BOUND,
    require_upper_bound: bool = REQUIRE_UPPER_BOUND,
    max_minor_lines: int = MAX_MINOR_LINES,
) -> Dict[str, object]:
    """Check one declaration against the version-bound contract.

    The returned dictionary is JSON-serialisable and carries ``ok``,
    ``codes``, ``declaration``, ``lower``, ``upper``, ``suggestion`` and
    ``message``.
    """
    requirement = parse_requirement(declaration)
    codes: List[str] = []

    if not str(declaration).strip():
        codes.append(CODE_UNPARSABLE_DECLARATION)
    if requirement.unsupported:
        codes.append(CODE_UNSUPPORTED_SPECIFIER)

    if requirement.lower is None and requirement.upper is None:
        if require_lower_bound and CODE_UNPARSABLE_DECLARATION not in codes:
            codes.append(CODE_MISSING_LOWER_BOUND)
        if require_upper_bound and CODE_UNPARSABLE_DECLARATION not in codes:
            codes.append(CODE_MISSING_UPPER_BOUND)
    elif requirement.lower is None:
        if require_lower_bound:
            codes.append(CODE_MISSING_LOWER_BOUND)
    elif requirement.upper is None:
        if require_upper_bound:
            codes.append(CODE_MISSING_UPPER_BOUND)
    else:
        lower, _ = requirement.lower
        upper, upper_exclusive = requirement.upper
        if lower > upper or (lower == upper and upper_exclusive):
            codes.append(CODE_INVERTED_RANGE)
        elif not _within_minor_lines(lower, upper, upper_exclusive, max_minor_lines):
            codes.append(CODE_UPPER_BOUND_TOO_WIDE)

    report: Dict[str, object] = {
        "declaration": str(declaration),
        "lower": _format_bound(requirement.lower, True),
        "upper": _format_bound(requirement.upper, False),
        "codes": codes,
        "suggestion": _suggestion(requirement, codes, max_minor_lines),
        "ok": not codes,
    }
    report["message"] = _message(report)
    return report


def derive_requirement(min_core_version: str, max_minor_lines: int = MAX_MINOR_LINES) -> Optional[str]:
    """Return the canonical bounded range for a minimum core version.

    ``derive_requirement("0.19.3")`` is ``">=0.19.3,<0.20.0"``; unreadable input
    returns ``None``.
    """
    lower = CoreVersion.parse(min_core_version)
    if lower is None:
        return None
    return f">={lower},<{lower.minor_line_limit(max_minor_lines)}"


def check_runtime(
    declaration: str,
    running_core: str,
    require_lower_bound: bool = REQUIRE_LOWER_BOUND,
    require_upper_bound: bool = REQUIRE_UPPER_BOUND,
    max_minor_lines: int = MAX_MINOR_LINES,
) -> Dict[str, object]:
    """Compare a declaration against the core version that is actually running.

    Returns a JSON-serialisable dictionary with ``declaration``, ``running``,
    ``verdict``, ``ok`` and the nested ``bound`` report. A running core newer
    than the declared upper bound is reported as
    :data:`VERDICT_CORE_NEWER_THAN_DECLARED` — the combination that fails only
    after the adapter is already imported.
    """
    bound = evaluate(declaration, require_lower_bound, require_upper_bound, max_minor_lines)
    running = CoreVersion.parse(running_core)
    if not bound["ok"]:
        verdict = VERDICT_DECLARATION_UNUSABLE
    elif running is None:
        verdict = VERDICT_UNKNOWN_CORE_VERSION
    else:
        requirement = parse_requirement(declaration)
        if requirement.contains(running):
            verdict = VERDICT_SUPPORTED
        elif requirement.lower is not None and running < requirement.lower[0]:
            verdict = VERDICT_CORE_OLDER_THAN_DECLARED
        else:
            verdict = VERDICT_CORE_NEWER_THAN_DECLARED
    return {
        "declaration": str(declaration),
        "running": str(running) if running is not None else None,
        "verdict": verdict,
        "ok": verdict == VERDICT_SUPPORTED,
        "bound": bound,
    }


def compare_declarations(packaging: str, environment: str) -> Dict[str, object]:
    """Compare the two declarations an adapter publishes for the same dependency.

    This is the check that catches the reported failure: packaging metadata
    pinning ``>=0.19.3,<0.19.5`` while the package environment requests
    ``dcc_mcp_core-0``. Drift is the actionable finding, so it wins over a
    policy violation; only a range that cannot be read at all yields
    :data:`DECLARATION_UNUSABLE`.
    """
    packaging_requirement = parse_requirement(packaging)
    environment_requirement = parse_requirement(environment)
    readable = (
        bool(str(packaging).strip())
        and bool(str(environment).strip())
        and not packaging_requirement.unsupported
        and not environment_requirement.unsupported
    )
    if not readable:
        drift = DECLARATION_UNUSABLE
    else:
        environment_wider = _lower_rank(environment_requirement) < _lower_rank(packaging_requirement) or _upper_rank(
            environment_requirement
        ) > _upper_rank(packaging_requirement)
        packaging_wider = _lower_rank(packaging_requirement) < _lower_rank(environment_requirement) or _upper_rank(
            packaging_requirement
        ) > _upper_rank(environment_requirement)
        if environment_wider:
            drift = ENVIRONMENT_WIDER
        elif packaging_wider:
            drift = PACKAGING_WIDER
        else:
            drift = ALIGNED
    return {
        "packaging": str(packaging),
        "environment": str(environment),
        "drift": drift,
        "ok": drift == ALIGNED,
    }


def installed_core_requirement(distribution: str) -> Optional[str]:
    """Return the ``dcc-mcp-core`` requirement declared by an installed distribution.

    Reads ``Requires-Dist`` metadata for ``distribution`` and returns the first
    requirement that names :data:`CORE_DISTRIBUTION` or
    :data:`CORE_IMPORT_NAME`, or ``None`` when the distribution is missing or
    declares no core dependency (for example inside a Rez-resolved environment
    where the request lives in the package environment instead).
    """
    for requirement in _metadata_requirements(distribution):
        if _names_core(requirement):
            return requirement.strip()
    return None


# ── internals ─────────────────────────────────────────────────────────────────


def _apply_specifier(requirement: CoreRequirement, specifier: str) -> None:
    operator, version_text = _split_operator(specifier)
    version = CoreVersion.parse(version_text)
    if version is None or not operator:
        requirement.unsupported.append(specifier)
        return
    if operator == ">=":
        _raise_lower(requirement, version, False)
    elif operator == ">":
        _raise_lower(requirement, version, True)
    elif operator == "<=":
        _lower_upper(requirement, version, False)
    elif operator == "<":
        _lower_upper(requirement, version, True)
    elif operator in ("==", "==="):
        _raise_lower(requirement, version, False)
        _lower_upper(requirement, version, False)
    elif operator == "~=":
        # PEP 440 compatible release: `~=0.19.3` is `>=0.19.3,<0.20.0`.
        _raise_lower(requirement, version, False)
        _lower_upper(requirement, version.next_minor(), True)
    else:
        requirement.unsupported.append(specifier)


def _parse_specifier_set(text: str) -> CoreRequirement:
    without_marker = text.split(";", 1)[0].replace("(", "").replace(")", "")
    start = 0
    for index, character in enumerate(without_marker):
        if character in "<>=!~":
            start = index
            break
    requirement = CoreRequirement()
    for raw in without_marker[start:].split(","):
        specifier = raw.strip()
        if specifier:
            _apply_specifier(requirement, specifier)
    return requirement


def _parse_package_environment(text: str) -> CoreRequirement:
    request_range = _request_range(text)
    if request_range is None:
        # A bare name such as `dcc_mcp_core` or `dcc-mcp-3dsmax`.
        return CoreRequirement()
    unsupported: List[str] = []
    if ".." in request_range:
        lower_text, upper_text = request_range.split("..", 1)
        lower = _version_or_unsupported(lower_text, unsupported)
        upper = _version_or_unsupported(upper_text, unsupported)
        return CoreRequirement(
            lower=None if lower is None else (lower, False),
            upper=None if upper is None else (upper, True),
            unsupported=unsupported,
        )
    version = _version_or_unsupported(request_range, unsupported)
    if version is None:
        return CoreRequirement(unsupported=unsupported)
    components = request_range.split(".")
    if len(components) >= 3:
        # `dcc_mcp_core-0.19.3` pins exactly one release.
        return CoreRequirement(lower=(version, False), upper=(version, False))
    if len(components) == 2:
        # `dcc_mcp_core-0.20` admits the whole 0.20 line.
        return CoreRequirement(lower=(version, False), upper=(version.next_minor(), True))
    # `dcc_mcp_core-0` admits every 0.x line.
    return CoreRequirement(
        lower=(version, False),
        upper=(CoreVersion(version.major + 1, 0, 0), True),
    )


def _request_range(text: str) -> Optional[str]:
    """Return the version range of a ``<name>-<range>`` request, if version-like."""
    for index in range(len(text) - 1, -1, -1):
        if text[index] != "-":
            continue
        suffix = text[index + 1 :]
        if suffix and all(character.isdigit() or character == "." for character in suffix):
            return suffix
    return None


def _version_or_unsupported(text: str, unsupported: List[str]) -> Optional[CoreVersion]:
    version = CoreVersion.parse(text)
    if version is None:
        unsupported.append(text)
    return version


def _raise_lower(requirement: CoreRequirement, version: CoreVersion, exclusive: bool) -> None:
    if requirement.lower is None or (version, exclusive) > requirement.lower:
        requirement.lower = (version, exclusive)


def _lower_upper(requirement: CoreRequirement, version: CoreVersion, exclusive: bool) -> None:
    # `inclusive` is the looser ceiling, so compare with the flag inverted.
    if requirement.upper is None or (version, not exclusive) < (
        requirement.upper[0],
        not requirement.upper[1],
    ):
        requirement.upper = (version, exclusive)


def _split_operator(specifier: str) -> Tuple[str, str]:
    for operator in _OPERATORS:
        if specifier.startswith(operator):
            return operator, specifier[len(operator) :].strip()
    return "", specifier.strip()


def _within_minor_lines(lower: CoreVersion, upper: CoreVersion, upper_exclusive: bool, max_minor_lines: int) -> bool:
    limit = lower.minor_line_limit(max_minor_lines)
    return upper <= limit if upper_exclusive else upper < limit


def _format_bound(bound: Optional[Tuple[CoreVersion, bool]], is_lower: bool) -> Optional[str]:
    if bound is None:
        return None
    version, exclusive = bound
    if is_lower:
        return f">{version}" if exclusive else f">={version}"
    return f"<{version}" if exclusive else f"<={version}"


def _suggestion(requirement: CoreRequirement, codes: Sequence[str], max_minor_lines: int) -> Optional[str]:
    if not codes or requirement.lower is None:
        return None
    lower = requirement.lower[0]
    if lower == CoreVersion(0, 0, 0):
        # `dcc_mcp_core-0` carries no usable floor, so narrowing it would invent
        # a supported range the adapter never declared.
        return None
    return f">={lower},<{lower.minor_line_limit(max_minor_lines)}"


def _message(report: Dict[str, object]) -> str:
    codes = report["codes"]
    if not codes:
        return "ok"
    declaration = report["declaration"]
    reasons = "; ".join(
        f"declaration '{declaration}' {CODE_DESCRIPTIONS.get(str(code), str(code))}"
        for code in codes  # type: ignore[union-attr]
    )
    suggestion = report["suggestion"]
    if suggestion:
        return f"{reasons}; use '{suggestion}'"
    return reasons


def _lower_rank(requirement: CoreRequirement) -> Tuple[int, CoreVersion, int]:
    """Lower-bound strictness: higher admits fewer older versions."""
    if requirement.lower is None:
        return (0, CoreVersion(0, 0, 0), 0)
    version, exclusive = requirement.lower
    return (1, version, int(exclusive))


def _upper_rank(requirement: CoreRequirement) -> Tuple[int, CoreVersion, int]:
    """Upper-bound strictness: lower admits fewer newer versions."""
    if requirement.upper is None:
        return (1, CoreVersion(0, 0, 0), 0)
    version, exclusive = requirement.upper
    return (0, version, int(not exclusive))


def _names_core(requirement: str) -> bool:
    # A package-environment request such as `dcc_mcp_core-0.20` keeps the
    # version attached to the name, so the separator set cannot include `-`:
    # splitting on it would turn the request into `dcc` and never match. Strip
    # a trailing `-<version-prefix>` range first, then apply the packaging
    # separator set.
    candidate = _REQUEST_SUFFIX.sub("", requirement.strip())
    normalized = re.split(r"[\[\(\)<>=!~;\s]", candidate, maxsplit=1)[0]
    normalized = normalized.lower().replace("_", "-")
    return normalized in (CORE_DISTRIBUTION, CORE_IMPORT_NAME.replace("_", "-"))


def _metadata_requirements(distribution: str) -> List[str]:
    """Return the ``Requires-Dist`` entries of an installed distribution."""
    metadata = _read_metadata(distribution)
    if metadata is None:
        return []
    requirements: List[str] = []
    for line in metadata.splitlines():
        if line.lower().startswith("requires-dist:"):
            requirements.append(line.split(":", 1)[1].strip())
    return requirements


def _read_metadata(distribution: str) -> Optional[str]:
    importlib_metadata = None
    try:
        import importlib.metadata as importlib_metadata
    except ImportError:
        try:
            import importlib_metadata  # type: ignore[no-redef]
        except ImportError:
            importlib_metadata = None  # type: ignore[assignment]
    if importlib_metadata is not None:
        try:
            return str(importlib_metadata.metadata(distribution))
        except Exception:
            pass
    return _scan_dist_info_metadata(distribution)


def _scan_dist_info_metadata(distribution: str) -> Optional[str]:
    """Fallback metadata read for hosts without :mod:`importlib.metadata`."""
    normalized = distribution.lower().replace("_", "-")
    prefix = normalized + "-"
    # Directory names are `<name>-<version>.dist-info`, and `<name>` itself may
    # contain dashes (`dcc-mcp-maya-0.9.4.dist-info`).
    for entry in [*sys.path, str(Path.cwd())]:
        if not entry:
            continue
        root = Path(entry)
        if not root.is_dir():
            continue
        for dist_info in sorted(root.iterdir()):
            name = dist_info.name
            if not name.endswith((".dist-info", ".egg-info")):
                continue
            stem = name.rsplit(".", 1)[0].lower().replace("_", "-")
            if not stem.startswith(prefix) or not stem[len(prefix) :][:1].isdigit():
                continue
            for filename in ("METADATA", "PKG-INFO"):
                candidate = dist_info / filename
                if not candidate.is_file():
                    continue
                try:
                    return candidate.read_text(encoding="utf-8", errors="replace")
                except OSError:
                    continue
    return None
