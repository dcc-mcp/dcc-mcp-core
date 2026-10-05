"""Version provenance + startup drift self-check for ``dcc-mcp-core``.

Why this module exists
----------------------
Inside one host, ``dcc-mcp-core``'s version can be read from two places and
they do **not** always agree:

* ``importlib.metadata.version("dcc-mcp-core")`` — the *installed
  distribution* metadata (``.dist-info`` / ``.egg-info``).  This is what
  ``pip`` believes is installed, and it is what
  ``dcc_mcp_core.__version__`` resolves to.
* ``dcc_mcp_core._core.__version__`` — the version compiled into the native
  PyO3 extension (``env!("CARGO_PKG_VERSION")`` in ``src/lib.rs``).  This is
  the artifact that actually executes, and it is the value
  :func:`dcc_mcp_core._version_util.package_version` prefers.

Both are generated from one release-please version at build time
(``pyproject.toml`` → wheel ``METADATA``; ``Cargo.toml`` → the compiled
extension), so a correct install prints the same number twice.  A *partially*
upgraded install does not: the reported case had a ``.dist-info`` saying
``0.19.3`` while the loaded ``_core`` extension said ``0.19.2``, so the
startup log line advertised a version nobody could reproduce.

Policy implemented here
-----------------------
1. **Single source at release time.**  release-please bumps
   ``.release-please-manifest.json``, ``pyproject.toml``, ``Cargo.toml`` and
   ``python/dcc_mcp_core/_core.pyi`` in one commit (see
   ``release-please-config.json`` → ``extra-files``).
   ``tests/test_version_single_source.py`` fails CI when those files ever
   drift apart.
2. **The executing artifact wins at runtime.**  ``_core.__version__`` is the
   code that runs, so it is what the startup log advertises and what a bug
   report should quote.
3. **Drift is loud, never fatal.**  :func:`run_version_self_check` runs once
   during :class:`~dcc_mcp_core.server_base.DccServerBase` construction, logs
   both answers with the ``.dist-info`` path, and emits a ``warning`` naming
   which side is stale when they disagree.  It never raises — a version
   mismatch must not stop an artist from working.

Python 3.7 support (Maya 2022)
------------------------------
:mod:`importlib.metadata` is 3.8+ and the ``importlib_metadata`` backport is
not a declared dependency (adding one for a diagnostics-only feature would
make every Maya 2022 install depend on a network-resolved wheel).  When
neither is importable, :func:`distribution_version` falls back to a pure
stdlib scan of ``sys.path`` for ``<package>-*.dist-info/METADATA`` (or the
``PKG-INFO`` of an ``.egg-info``, including setuptools' single-file form) and
reads the ``Version:`` field.  That keeps the drift check working on the py3.7
hosts where it matters most — those are the ones that still carry stale
installs from several releases ago.

Opt out with ``DCC_MCP_CORE_VERSION_CHECK=0``.
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
from dataclasses import dataclass
import logging
from pathlib import Path
import sys
from typing import Any
from typing import Dict
from typing import List
from typing import Optional
from typing import Tuple

# Import local modules
from dcc_mcp_core.constants import ENV_VERSION_CHECK
from dcc_mcp_core.env import env_flag

logger = logging.getLogger(__name__)

#: Distribution name of this package (as ``pip`` knows it).
PACKAGE = "dcc-mcp-core"
#: Dotted module name of the compiled PyO3 extension.
NATIVE_MODULE = "dcc_mcp_core._core"

#: Both answers exist and agree.
STATUS_CONSISTENT = "consistent"
#: Both answers exist but disagree — partial upgrade or stale artifact on ``sys.path``.
STATUS_MISMATCH = "mismatch"
#: No installed-distribution metadata for the package (source checkout, vendored module zip).
STATUS_NOT_INSTALLED = "not-installed"
#: Not enough information to answer (no metadata API, import failure, no ``__version__``).
STATUS_UNKNOWN = "unknown"

#: The installed distribution metadata is older than the executing artifact.
STALE_DISTRIBUTION = "distribution"
#: The executing native extension is older than the installed distribution metadata.
STALE_RUNTIME = "runtime"
#: Both answers exist and disagree, but neither can be ordered.
STALE_UNKNOWN = "unknown"


def _metadata_module() -> Optional[Any]:
    """Return an ``importlib.metadata``-compatible module, or ``None``.

    ``importlib.metadata`` landed in Python 3.8; Maya 2022 still runs 3.7,
    where only the ``importlib_metadata`` backport provides the same API.
    """
    try:
        from importlib import metadata as importlib_metadata
    except ImportError:  # pragma: no cover - Python 3.7 only
        try:
            import importlib_metadata  # type: ignore[no-redef]
        except ImportError:
            return None
    return importlib_metadata


def _normalize_version(raw: Any) -> str:
    """Normalise *raw* so ``0.19.2`` and ``0.19.02`` compare equal.

    Falls back to a lower-cased string when :mod:`packaging` is unavailable
    or the value is not PEP 440 (``"unknown"``, ``"0.19.16+local"`` …).
    """
    text = "" if raw is None else str(raw).strip()
    if not text:
        return ""
    try:
        from packaging.version import Version

        return str(Version(text))
    except Exception:
        # InvalidVersion / ImportError / TypeError — non-PEP 440 values such as
        # ``0.0.0-dev`` must still compare deterministically instead of raising.
        return text.lower()


def _metadata_field(metadata_text: str, field: str) -> Optional[str]:
    """Return the first ``Field: value`` line matching *field* (case-insensitive)."""
    prefix = field.lower() + ":"
    for line in metadata_text.splitlines():
        if line.lower().startswith(prefix):
            value = line[len(prefix) :].strip()
            if value:
                return value
    return None


def _version_from_distribution_name(distribution_path: str) -> str:
    """Extract the version segment of a ``<package>-<version>.dist-info`` name.

    Only used to *order* candidates inside one directory; the reported version
    always comes from the metadata file itself.
    """
    name = Path(distribution_path).name
    for suffix in (".dist-info", ".egg-info"):
        if name.lower().endswith(suffix):
            name = name[: -len(suffix)]
            break
    else:
        return ""
    # ``<package>-<version>``: PEP 440 versions never contain a dash, so the
    # last dash-separated segment is the version.
    return name.rsplit("-", 1)[-1] if "-" in name else ""


def _version_sort_key(version_text: str) -> Tuple[Any, ...]:
    """Return a sort key that orders PEP 440 versions numerically.

    ``0.19.30`` must rank above ``0.19.2``; ``sorted()`` over the raw
    ``.dist-info`` directory names gets that the wrong way round.  Falls back
    to the lower-cased text when :mod:`packaging` is unavailable or the value
    is not a valid version, so non-PEP 440 names still order deterministically
    instead of raising.
    """
    text = _normalize_version(version_text)
    if not text:
        return (0, None, "")
    try:
        from packaging.version import Version

        return (1, Version(text), "")
    except Exception:
        # InvalidVersion / ImportError — fall back to a stable text ordering.
        return (0, None, text)


def _candidate_sort_key(candidate: Tuple[str, str]) -> Tuple[Any, ...]:
    """Sort key ranking one scan candidate by version, newest first.

    The metadata file is authoritative; its ``.dist-info`` name is only the
    fallback for candidates whose version cannot be read.
    """
    version = _version_from_metadata_file(candidate[0]) or _version_from_distribution_name(candidate[1])
    return _version_sort_key(version)


def _scan_distribution_dirs(package: str) -> List[Tuple[str, str]]:
    """Find ``<package>-*.dist-info`` / ``.egg-info`` distributions on ``sys.path``.

    Pure-stdlib stand-in for :mod:`importlib.metadata`, used on Python 3.7
    (Maya 2022) where neither the stdlib module nor the
    ``importlib_metadata`` backport is guaranteed to exist.  Recognises the
    ``<package>-<version>.dist-info`` / ``.egg-info`` layouts, plus the
    unversioned ``<package>.egg-info`` that setuptools writes for editable
    installs.  Returns ``(metadata_file, distribution_path)`` pairs in
    ``sys.path`` order; where one directory holds several distributions the
    highest PEP 440 version comes first.
    """
    normalised = package.replace("-", "_")
    prefixes = (normalised + "-", package + "-")
    suffixes = (".dist-info", ".egg-info")
    # setuptools names the editable-install artifact ``<package>.egg-info``,
    # with no version segment for the prefix check above to match on.
    bare_names = (normalised + ".egg-info", package.lower() + ".egg-info")
    found: List[Tuple[str, str]] = []

    for entry in sys.path:
        if not entry:
            continue
        try:
            names = sorted(Path(entry).iterdir())
        except OSError:
            # Unreadable / missing sys.path entry — nothing to scan there.
            continue
        candidates: List[Tuple[str, str]] = []
        for child in names:
            lowered = child.name.lower()
            if not lowered.endswith(suffixes):
                continue
            if not lowered.startswith(prefixes) and lowered not in bare_names:
                continue
            if child.is_file():
                # setuptools allows a single-file ``.egg-info`` that *is* the
                # PKG-INFO payload, with no directory to look inside of.
                candidates.append((str(child), str(child)))
                continue
            metadata_file = child / "METADATA"
            if not metadata_file.is_file():
                metadata_file = child / "PKG-INFO"
                if not metadata_file.is_file():
                    continue
            candidates.append((str(metadata_file), str(child)))
        # Several distributions in one directory is itself the failure mode
        # this check exists to surface: the newest one is the best answer, and
        # lexicographic ordering would rank ``0.19.2`` above ``0.19.30``.
        candidates.sort(key=_candidate_sort_key, reverse=True)
        found.extend(candidates)
    return found


def _version_from_metadata_file(metadata_file: str) -> Optional[str]:
    try:
        with Path(metadata_file).open(encoding="utf-8", errors="replace") as handle:
            content = handle.read()
    except OSError:
        return None
    return _metadata_field(content, "Version")


def _distribution_path_from_api(metadata: Any, package: str) -> Optional[str]:
    """Locate *package* through the metadata API, at ``.dist-info`` granularity."""
    try:
        distribution = metadata.distribution(package)
    except Exception:
        # PackageNotFoundError and friends.
        return None
    if distribution is None:
        return None
    # ``Distribution._path`` is the ``.dist-info`` directory — the artifact an
    # operator actually has to delete.  locate_file("") resolves to the
    # ``site-packages`` root instead, so it is only a fallback for backports
    # that do not expose ``_path``.
    try:
        path = getattr(distribution, "_path", None)
    except Exception:
        path = None
    if path:
        return str(path)
    try:
        located = distribution.locate_file("")
    except Exception:
        return None
    return str(located) if located else None


def _resolve_distribution(package: str) -> Tuple[Optional[str], Optional[str]]:
    """Resolve the installed distribution once, as ``(version, path)``.

    Both halves always describe the **same** artifact.  Resolving them
    independently lets the two answers come apart — the version from one
    ``.dist-info`` and the path from another — which would tell the operator
    to delete a distribution the report never quoted.

    Resolution order:

    1. :mod:`importlib.metadata` (Python 3.8+) or the ``importlib_metadata``
       backport, when importable.
    2. A pure-stdlib scan of ``sys.path`` for ``<package>-*.dist-info``, so
       Python 3.7 / Maya 2022 hosts are still covered.

    A candidate that yields no version is skipped in both passes alike, so a
    malformed ``.dist-info`` on ``sys.path`` cannot supply a path on its own.
    Returns ``(None, None)`` when nothing usable is found.
    """
    metadata = _metadata_module()
    if metadata is not None:
        try:
            value = metadata.version(package)
        except Exception:
            # PackageNotFoundError and friends.
            value = None
        # A malformed METADATA without a ``Version:`` field can answer with an
        # empty value; ``str()`` would turn that into the literal ``"None"``
        # and poison the report, so fall through to the scan instead.
        if value:
            return str(value), _distribution_path_from_api(metadata, package)
        # Fall through: a partially-installed distribution can raise even when
        # a .dist-info directory is present, and the scan can still read it.

    for metadata_file, distribution_path in _scan_distribution_dirs(package):
        version = _version_from_metadata_file(metadata_file)
        if version:
            return version, distribution_path
    return None, None


def distribution_version(package: str = PACKAGE) -> Optional[str]:
    """Return the installed-distribution version of *package*.

    See :func:`_resolve_distribution` for the resolution order.

    Returns ``None`` when no distribution metadata can be found at all.
    """
    return _resolve_distribution(package)[0]


def distribution_location(package: str = PACKAGE) -> Optional[str]:
    """Return where the installed distribution of *package* lives on disk.

    Used in log lines so an operator can delete the stale ``.dist-info``
    without guessing which site-packages is responsible.

    Both resolution paths answer at the same granularity: the ``.dist-info`` /
    ``.egg-info`` artifact itself.  The returned path always belongs to the
    same artifact that :func:`distribution_version` quoted, because a report
    naming one version and another distribution's directory would send the
    operator to delete the wrong install.

    The one exception is a metadata backport that does not expose ``_path``:
    ``locate_file("")`` then answers with the site-packages root that contains
    the artifact, which is coarser than ideal but still names the right
    install.
    """
    return _resolve_distribution(package)[1]


def native_version(load: bool = False) -> Optional[str]:
    """Return the version compiled into the PyO3 extension, or ``None``.

    This is the version of the artifact that actually executes: ``src/lib.rs``
    registers ``__version__`` from ``env!("CARGO_PKG_VERSION")``, so it is
    frozen when the wheel was built rather than read from disk at runtime.

    Parameters
    ----------
    load:
        Import the native extension when it is not in ``sys.modules`` yet.
        Callers that must stay import-light (installer/uninstaller paths) leave
        this disabled; :mod:`~dcc_mcp_core.server_base` already imports
        ``_core``, so the module is normally present by then.

    """
    core: Any = sys.modules.get(NATIVE_MODULE)
    if core is None and load:
        try:
            import importlib

            core = importlib.import_module(NATIVE_MODULE)
        except Exception:
            # No wheel, no disk, nothing to report.
            core = None
    version = getattr(core, "__version__", None) if core is not None else None
    return str(version) if version else None


def native_origin() -> Optional[str]:
    """Return ``__file__`` of the loaded native extension (``None`` when absent)."""
    core: Any = sys.modules.get(NATIVE_MODULE)
    if core is None:
        return None
    try:
        origin = getattr(core, "__file__", None)
    except Exception:
        return None
    return str(origin) if origin else None


def _stale_side(runtime_version: str, installed: str) -> str:
    """Return which side of a mismatch is older.

    Both values are compared through :func:`_normalize_version`; the lower one
    is the stale artifact.  Returns :data:`STALE_UNKNOWN` when the two cannot
    be ordered (``"0.0.0-dev"``, local versions, non-PEP 440 strings) — naming
    a side we cannot compute would be worse than admitting we cannot tell.

    A tie after normalisation (``0.19.2`` vs ``0.19.02``) is not drift, so it
    is reported as :data:`STALE_UNKNOWN` rather than picking a side.
    """
    left = _normalize_version(runtime_version)
    right = _normalize_version(installed)
    if not left or not right or left == right:
        return STALE_UNKNOWN
    try:
        from packaging.version import Version
    except Exception:
        # No packaging available — plain text ordering is the best we can do.
        return STALE_RUNTIME if left < right else STALE_DISTRIBUTION
    try:
        return STALE_RUNTIME if Version(left) < Version(right) else STALE_DISTRIBUTION
    except Exception:
        # InvalidVersion on either side.
        return STALE_UNKNOWN


@dataclass
class VersionReport:
    """Version provenance for this package.

    Attributes
    ----------
    package:
        Distribution name (``dcc-mcp-core``).
    runtime_version:
        ``__version__`` compiled into the loaded native extension — the
        artifact actually executing.  ``None`` when the extension is absent
        (pure-Python sidecar / lite-fallback mode).
    distribution_version:
        ``Version:`` recorded in the installed distribution metadata.
    status:
        One of :data:`STATUS_CONSISTENT`, :data:`STATUS_MISMATCH`,
        :data:`STATUS_NOT_INSTALLED`, :data:`STATUS_UNKNOWN`.
    module_path:
        ``__file__`` of the loaded native extension.
    distribution_path:
        Location of the distribution metadata on disk.
    stale_side:
        :data:`STALE_RUNTIME`, :data:`STALE_DISTRIBUTION` or
        :data:`STALE_UNKNOWN`; empty unless ``status`` is
        :data:`STATUS_MISMATCH`.
    detail:
        Human-readable explanation, empty when the report is consistent.

    """

    package: str
    runtime_version: Optional[str]
    distribution_version: Optional[str]
    status: str
    module_path: Optional[str] = None
    distribution_path: Optional[str] = None
    stale_side: str = ""
    detail: str = ""

    @property
    def consistent(self) -> bool:
        """``True`` only when both answers exist and agree."""
        return self.status == STATUS_CONSISTENT

    @property
    def drift(self) -> bool:
        """``True`` when both answers exist and disagree."""
        return self.status == STATUS_MISMATCH

    @property
    def authoritative_version(self) -> Optional[str]:
        """Version to quote in bug reports — the executing artifact wins.

        Falls back to the distribution metadata when the native extension is
        not loaded (sidecar / lite-fallback mode), and to ``None`` when
        neither answer exists.
        """
        return self.runtime_version or self.distribution_version

    def to_dict(self) -> Dict[str, Any]:
        """Return a JSON-serialisable snapshot of this report."""
        return {
            "package": self.package,
            "status": self.status,
            "consistent": self.consistent,
            "drift": self.drift,
            "runtime_version": self.runtime_version,
            "distribution_version": self.distribution_version,
            "authoritative_version": self.authoritative_version,
            "stale_side": self.stale_side,
            "module_path": self.module_path,
            "distribution_path": self.distribution_path,
            "detail": self.detail,
        }


def build_report(
    *,
    module_path: Optional[str] = None,
    runtime_version: Optional[str] = None,
    installed_version: Optional[str] = None,
    installed_path: Optional[str] = None,
) -> VersionReport:
    """Compare the executing native extension with the installed distribution.

    Every input is optional so tests (and diagnostics callers that already
    resolved one side) can inject answers without touching ``sys.path`` or
    importing the native extension.  Omitted values are resolved from the
    live environment.
    """
    runtime_version = native_version() if runtime_version is None else str(runtime_version)
    if module_path is None:
        module_path = native_origin()
    if installed_version is None:
        installed_version = distribution_version()
    if installed_path is None:
        installed_path = distribution_location() if installed_version else None

    if runtime_version is None:
        return VersionReport(
            package=PACKAGE,
            runtime_version=None,
            distribution_version=installed_version,
            status=STATUS_UNKNOWN,
            module_path=module_path,
            distribution_path=installed_path,
            detail=f"{NATIVE_MODULE} does not expose __version__ (native extension not loaded)",
        )

    if installed_version is None:
        if _metadata_module() is None:
            detail = (
                "no distribution metadata source available: neither importlib.metadata nor the "
                f"importlib_metadata backport is importable, and no {PACKAGE}-*.dist-info or .egg-info "
                "was found on sys.path"
            )
            status = STATUS_UNKNOWN
        else:
            detail = "no installed distribution metadata found (source checkout or vendored module)"
            status = STATUS_NOT_INSTALLED
        return VersionReport(
            package=PACKAGE,
            runtime_version=runtime_version,
            distribution_version=None,
            status=status,
            module_path=module_path,
            distribution_path=None,
            detail=detail,
        )

    if _normalize_version(runtime_version) == _normalize_version(installed_version):
        return VersionReport(
            package=PACKAGE,
            runtime_version=runtime_version,
            distribution_version=installed_version,
            status=STATUS_CONSISTENT,
            module_path=module_path,
            distribution_path=installed_path,
        )

    stale_side = _stale_side(runtime_version, installed_version)
    if stale_side == STALE_RUNTIME:
        stale_detail = f"the loaded native extension ({runtime_version}) is older than the installed distribution"
        remedy = f"reinstall {PACKAGE} so the extension is replaced"
    elif stale_side == STALE_DISTRIBUTION:
        stale_detail = (
            f"the installed distribution metadata ({installed_version}) is older than the loaded native extension"
        )
        remedy = f"upgrade {PACKAGE} so the metadata matches the extension"
    else:
        stale_detail = "the two answers cannot be ordered"
        remedy = f"reinstall {PACKAGE} so both answers come from one build"

    return VersionReport(
        package=PACKAGE,
        runtime_version=runtime_version,
        distribution_version=installed_version,
        status=STATUS_MISMATCH,
        module_path=module_path,
        distribution_path=installed_path,
        stale_side=stale_side,
        detail=(
            f"installed distribution metadata reports {installed_version} but the loaded native "
            f"extension reports {runtime_version} — {stale_detail}; {remedy}"
        ),
    )


def version_report() -> Dict[str, Any]:
    """Return a JSON-serialisable version provenance payload (no logging).

    Shape::

        {
          "package": "dcc-mcp-core",
          "status": "consistent" | "mismatch" | "not-installed" | "unknown",
          "consistent": bool,
          "drift": bool,
          "runtime_version": str | None,
          "distribution_version": str | None,
          "authoritative_version": str | None,
          "stale_side": "runtime" | "distribution" | "unknown",
          "module_path": str | None,
          "distribution_path": str | None,
          "detail": str,
        }

    ``consistent`` is intentionally strict: a report that could not be decided
    (``not-installed`` for a source checkout, ``unknown`` when no metadata can
    be read) makes it ``False``.  Consumers that only care about real drift —
    "two answers exist and disagree" — must read ``drift`` instead.
    """
    return build_report().to_dict()


def log_report(report: VersionReport, target_logger: Optional[logging.Logger] = None) -> None:
    """Log one report: ``info`` when healthy, ``warning`` on drift."""
    log = target_logger if target_logger is not None else logger
    version = report.authoritative_version or "unknown"

    if report.drift:
        log.warning(
            "dcc-mcp-core version drift — %s (running: %s, distribution: %s; extension: %s; distribution metadata: %s)",
            report.detail,
            report.runtime_version or "unknown",
            report.distribution_version or "unknown",
            report.module_path or "unknown",
            report.distribution_path or "unknown",
        )
        return

    if report.consistent:
        log.info(
            "dcc-mcp-core %s (extension: %s; distribution: %s)",
            version,
            report.module_path or "unknown",
            report.distribution_path or "unknown",
        )
        return

    log.debug(
        "dcc-mcp-core %s — %s (extension: %s; distribution: %s; status: %s)",
        version,
        report.detail,
        report.module_path or "unknown",
        report.distribution_path or "unknown",
        report.status,
    )


def version_check_enabled() -> bool:
    """Return whether the startup self-check should run.

    Opt out with ``DCC_MCP_CORE_VERSION_CHECK=0``.  Defaults to enabled: the
    whole point of the check is that a silent drift is worse than one extra
    metadata lookup per process start.
    """
    return env_flag(ENV_VERSION_CHECK, default=True, truthy=("1", "true"))


def run_version_self_check(target_logger: Optional[logging.Logger] = None) -> Dict[str, Any]:
    """Run the startup version self-check: log both answers, warn on drift.

    Never raises.  Returns the same payload as :func:`version_report` so
    callers (tests, diagnostics endpoints, instance metadata) can reuse it.
    """
    log = target_logger if target_logger is not None else logger
    try:
        report = build_report()
    except Exception as exc:
        # Diagnostics must never block startup.
        log.debug("dcc-mcp-core version self-check failed: %s", exc)
        return {}
    log_report(report, log)
    return report.to_dict()


def resolve_startup_version(fallback: str) -> str:
    """Return the version the startup log should advertise.

    Thin wrapper so :mod:`~dcc_mcp_core.server_base` does not have to know the
    precedence rules: the executing native extension wins, the installed
    distribution metadata is next, and *fallback* is used only when neither
    answer exists (source checkout with no native wheel loaded).  Never raises
    and never returns an empty string.
    """
    try:
        report = build_report()
    except Exception:
        # Diagnostics must never block startup.
        return fallback
    return report.authoritative_version or fallback
