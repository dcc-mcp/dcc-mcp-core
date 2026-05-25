"""Helpers for registering skill metadata-driven MCP extension tools.

DCC adapters often expose the same optional skill metadata extensions:
recipes, reference docs, and future extension files declared under
``metadata.dcc-mcp``. This module centralises the startup pattern so adapters
can scan skills once, register each optional extension, and inspect a compact
report instead of copying try/import/register wrappers.
"""

from __future__ import annotations

from dataclasses import dataclass
import importlib
import logging
from typing import Any
from typing import Callable
from typing import Dict
from typing import Iterable
from typing import List
from typing import Optional
from typing import Sequence
from typing import Tuple
from typing import Union

logger = logging.getLogger(__name__)


ExtensionCallback = Callable[..., Any]
RegistrationInput = Union["MetadataExtensionRegistration", ExtensionCallback, Tuple[str, ExtensionCallback]]


@dataclass(frozen=True)
class MetadataExtensionRegistration:
    """Description of one optional metadata-driven extension registration.

    Provide either ``callback`` directly or ``module`` + ``attribute`` for a
    lazy import. Lazy imports keep adapter startup resilient when an extension
    remains optional for a host.
    """

    name: str
    callback: Optional[ExtensionCallback] = None
    module: Optional[str] = None
    attribute: Optional[str] = None
    optional: bool = True

    def resolve(self) -> ExtensionCallback:
        """Return the callable registration function for this extension."""
        if self.callback is not None:
            return self.callback
        if not self.module or not self.attribute:
            raise ValueError(f"metadata extension {self.name!r} has no callback or import path")
        module = importlib.import_module(self.module)
        callback = getattr(module, self.attribute)
        if not callable(callback):
            raise TypeError(f"{self.module}.{self.attribute} is not callable")
        return callback


@dataclass(frozen=True)
class MetadataExtensionResult:
    """Outcome for one metadata extension registration attempt."""

    name: str
    status: str
    message: Optional[str] = None


@dataclass(frozen=True)
class MetadataRegistrationReport:
    """Summary returned by :func:`register_metadata_driven_tools`."""

    phase: str
    skills: List[Any]
    skipped: List[Any]
    extensions: List[MetadataExtensionResult]
    scan_error: Optional[str] = None

    @property
    def registered_count(self) -> int:
        """Number of extensions that completed registration."""
        return sum(1 for item in self.extensions if item.status == "registered")

    @property
    def failed_count(self) -> int:
        """Number of extensions that failed during import or registration."""
        return sum(1 for item in self.extensions if item.status == "failed")

    @property
    def skipped_count(self) -> int:
        """Number of optional extensions skipped before registration."""
        return sum(1 for item in self.extensions if item.status == "skipped")

    @property
    def ok(self) -> bool:
        """Whether scanning and all attempted extension registrations avoided hard failures."""
        return self.scan_error is None and self.failed_count == 0

    def to_dict(self) -> Dict[str, Any]:
        """Return a JSON-serialisable representation for logs or diagnostics."""
        return {
            "phase": self.phase,
            "skill_count": len(self.skills),
            "skipped": [str(item) for item in self.skipped],
            "scan_error": self.scan_error,
            "extensions": [
                {"name": item.name, "status": item.status, "message": item.message} for item in self.extensions
            ],
        }


def metadata_extension(
    name: str,
    callback: ExtensionCallback,
    *,
    optional: bool = True,
) -> MetadataExtensionRegistration:
    """Create a direct callback registration descriptor."""
    return MetadataExtensionRegistration(name=name, callback=callback, optional=optional)


def imported_metadata_extension(
    name: str,
    module: str,
    attribute: str,
    *,
    optional: bool = True,
) -> MetadataExtensionRegistration:
    """Create a lazy import registration descriptor."""
    return MetadataExtensionRegistration(
        name=name,
        module=module,
        attribute=attribute,
        optional=optional,
    )


def default_metadata_extension_registrations() -> List[MetadataExtensionRegistration]:
    """Return the built-in optional metadata extension registrations."""
    return [
        imported_metadata_extension(
            "recipes",
            "dcc_mcp_core.recipes",
            "register_recipes_tools",
        ),
        imported_metadata_extension(
            "skill-reference-docs",
            "dcc_mcp_core.skill_reference_docs",
            "register_skill_reference_docs_tools",
        ),
    ]


def _coerce_registration(raw: RegistrationInput) -> MetadataExtensionRegistration:
    if isinstance(raw, MetadataExtensionRegistration):
        return raw
    if isinstance(raw, tuple) and len(raw) == 2:
        name, callback = raw
        if not isinstance(name, str) or not callable(callback):
            raise TypeError("registration tuples must be (name, callable)")
        return metadata_extension(name, callback)
    if callable(raw):
        return metadata_extension(getattr(raw, "__name__", "metadata-extension"), raw)
    raise TypeError(f"unsupported metadata extension registration: {raw!r}")


def _load_skills(
    *,
    dcc_name: str,
    extra_paths: Optional[Iterable[str]],
    scan: Optional[Callable[..., Tuple[List[Any], List[Any]]]],
    log: logging.Logger,
    log_prefix: str,
) -> Tuple[List[Any], List[Any], Optional[str]]:
    scan_fn = scan
    if scan_fn is None:
        from dcc_mcp_core import scan_and_load_lenient

        scan_fn = scan_and_load_lenient
    try:
        skills, skipped = scan_fn(
            extra_paths=list(extra_paths) if extra_paths is not None else None,
            dcc_name=dcc_name,
        )
        return list(skills), list(skipped), None
    except Exception as exc:
        log.warning("%s: skill scan failed: %s", log_prefix, exc)
        return [], [], str(exc)


def register_metadata_driven_tools(
    server: Any,
    *,
    skills: Optional[Sequence[Any]] = None,
    skipped: Optional[Sequence[Any]] = None,
    dcc_name: str = "dcc",
    extra_paths: Optional[Iterable[str]] = None,
    registrations: Optional[Sequence[RegistrationInput]] = None,
    scan: Optional[Callable[..., Tuple[List[Any], List[Any]]]] = None,
    phase: str = "startup",
    log_prefix: str = "register_metadata_driven_tools",
    logger: Optional[logging.Logger] = None,
) -> MetadataRegistrationReport:
    """Register optional skill metadata extension tools on *server*.

    When ``skills`` is omitted the helper calls ``scan_and_load_lenient`` using
    ``extra_paths`` and ``dcc_name``. Each registration callback is invoked as::

        callback(server, skills=loaded_skills, dcc_name=dcc_name)

    Failures are logged and recorded in the returned report so one optional
    extension cannot prevent later extensions from registering.
    """
    log = logger if logger is not None else globals()["logger"]
    if skills is None:
        loaded, skipped_dirs, scan_error = _load_skills(
            dcc_name=dcc_name,
            extra_paths=extra_paths,
            scan=scan,
            log=log,
            log_prefix=log_prefix,
        )
    else:
        loaded = list(skills)
        skipped_dirs = list(skipped or [])
        scan_error = None

    raw_registrations = registrations
    if raw_registrations is None:
        raw_registrations = default_metadata_extension_registrations()

    results: List[MetadataExtensionResult] = []
    for raw in raw_registrations:
        try:
            registration = _coerce_registration(raw)
        except Exception as exc:
            log.warning("%s: invalid registration skipped: %s", log_prefix, exc)
            results.append(MetadataExtensionResult(name="<invalid>", status="skipped", message=str(exc)))
            continue

        try:
            callback = registration.resolve()
        except Exception as exc:
            status = "skipped" if registration.optional else "failed"
            log.warning("%s: %s import failed: %s", log_prefix, registration.name, exc)
            results.append(MetadataExtensionResult(registration.name, status, str(exc)))
            continue

        try:
            callback(server, skills=loaded, dcc_name=dcc_name)
        except Exception as exc:
            log.warning("%s: %s registration failed: %s", log_prefix, registration.name, exc)
            results.append(MetadataExtensionResult(registration.name, "failed", str(exc)))
            continue
        results.append(MetadataExtensionResult(registration.name, "registered"))

    return MetadataRegistrationReport(
        phase=phase,
        skills=loaded,
        skipped=skipped_dirs,
        extensions=results,
        scan_error=scan_error,
    )


__all__ = [
    "MetadataExtensionRegistration",
    "MetadataExtensionResult",
    "MetadataRegistrationReport",
    "default_metadata_extension_registrations",
    "imported_metadata_extension",
    "metadata_extension",
    "register_metadata_driven_tools",
]
