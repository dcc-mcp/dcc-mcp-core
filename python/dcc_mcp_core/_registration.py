"""Registration phase pipeline for DCC MCP builtin-action registration.

Host adapters import the shared base classes and executor from here,
then define their own phase subclasses in a host-specific
``_registration`` module.
"""

from __future__ import annotations

from dataclasses import dataclass
from dataclasses import field
import time
from typing import Any
from typing import Sequence


@dataclass
class RegistrationContext:
    """Input shared by every registration phase."""

    server: Any
    extra_skill_paths: list[str] | None = None
    include_bundled: bool = True
    minimal: bool | None = None
    strict_scan: bool | None = None
    minimal_mode: Any | None = None


@dataclass
class PhaseOutcome:
    """Result for one registration phase."""

    name: str
    success: bool
    elapsed_secs: float
    error: str | None = None


@dataclass
class RegistrationReport:
    """Summary emitted after builtin-action registration completes."""

    outcomes: list[PhaseOutcome] = field(default_factory=list)

    @property
    def success(self) -> bool:
        return all(outcome.success for outcome in self.outcomes)

    @property
    def elapsed_secs(self) -> float:
        return sum(outcome.elapsed_secs for outcome in self.outcomes)


class RegistrationPhase:
    """Base class for one side-effect in DCC builtin registration."""

    name = "registration"
    fatal_exceptions: tuple[type[Exception], ...] = ()

    def run(self, context: RegistrationContext) -> None:
        raise NotImplementedError


def run_registration_phases(phases: Sequence[RegistrationPhase], context: RegistrationContext) -> RegistrationReport:
    report = RegistrationReport()
    for phase in phases:
        started = time.monotonic()
        try:
            phase.run(context)
        except phase.fatal_exceptions as exc:
            report.outcomes.append(
                PhaseOutcome(
                    name=phase.name,
                    success=False,
                    elapsed_secs=time.monotonic() - started,
                    error=str(exc),
                )
            )
            raise
        except Exception as exc:  # phase loop localizes optional integration failures
            report.outcomes.append(
                PhaseOutcome(
                    name=phase.name,
                    success=False,
                    elapsed_secs=time.monotonic() - started,
                    error=str(exc),
                )
            )
        else:
            report.outcomes.append(
                PhaseOutcome(
                    name=phase.name,
                    success=True,
                    elapsed_secs=time.monotonic() - started,
                )
            )
    return report


class CoreBuiltinActionsPhase(RegistrationPhase):
    """Discover skills via the core registration path."""

    name = "core_builtin_actions"

    def run(self, context: RegistrationContext) -> None:
        context.server._register_core_builtin_actions(context)


class StrictSkillScanPhase(RegistrationPhase):
    """Run strict skill validation when ``strict_scan`` is enabled."""

    name = "strict_skill_scan"
    fatal_exceptions = (ValueError,)

    def run(self, context: RegistrationContext) -> None:
        context.server._run_strict_skill_scan_phase(context)


class MetadataDrivenToolsPhase(RegistrationPhase):
    """Register ``recipes__*`` and ``skill_refs__*`` tools."""

    name = "metadata_driven_tools"

    def run(self, context: RegistrationContext) -> None:
        context.server._register_metadata_driven_tools(context)


class IntrospectToolsPhase(RegistrationPhase):
    """Register the four ``dcc_introspect__*`` MCP tools."""

    name = "introspect_tools"

    def run(self, context: RegistrationContext) -> None:
        context.server._register_introspect_tools(context)


class FeedbackToolPhase(RegistrationPhase):
    """Register the ``dcc_feedback__report`` MCP tool."""

    name = "feedback_tool"

    def run(self, context: RegistrationContext) -> None:
        context.server._register_feedback_tool(context)


class QtUiInspectorPhase(RegistrationPhase):
    """Register the shared ``qt_ui_inspector__*`` tools."""

    name = "qt_ui_inspector"

    def run(self, context: RegistrationContext) -> None:
        context.server._register_qt_ui_inspector(context)


class CapabilityManifestPhase(RegistrationPhase):
    """Register the ``dcc_capability_manifest`` MCP tool."""

    name = "capability_manifest"

    def run(self, context: RegistrationContext) -> None:
        context.server._register_capability_manifest_tool(context)


class ProjectToolsPhase(RegistrationPhase):
    """Register the four ``project_*`` MCP tools."""

    name = "project_tools"

    def run(self, context: RegistrationContext) -> None:
        context.server._attach_project_tools(context)


class ResourcesPhase(RegistrationPhase):
    """Publish ``scene://current`` + dynamic resource producers."""

    name = "resources"

    def run(self, context: RegistrationContext) -> None:
        context.server._attach_resources(context)


class SkillCatalogReadyPhase(RegistrationPhase):
    """Signal that the skill catalog has been populated (readiness gate)."""

    name = "skill_catalog_ready"

    def run(self, context: RegistrationContext) -> None:
        context.server._mark_skill_catalog_ready(context)


def get_standard_phases() -> list[RegistrationPhase]:
    """Return the ordered list of standard registration phases."""
    return [
        CoreBuiltinActionsPhase(),
        StrictSkillScanPhase(),
        MetadataDrivenToolsPhase(),
        IntrospectToolsPhase(),
        FeedbackToolPhase(),
        QtUiInspectorPhase(),
        CapabilityManifestPhase(),
        ProjectToolsPhase(),
        ResourcesPhase(),
        SkillCatalogReadyPhase(),
    ]
