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
        if hasattr(context.server, "_register_core_builtin_actions"):
            context.server._register_core_builtin_actions(context)
        else:
            context.server.register_builtin_actions(
                extra_skill_paths=context.extra_skill_paths,
                include_bundled=context.include_bundled,
                minimal_mode=context.minimal_mode,
            )


class StrictSkillScanPhase(RegistrationPhase):
    """Run strict skill validation when ``strict_scan`` is enabled."""

    name = "strict_skill_scan"
    fatal_exceptions = (ValueError,)

    def run(self, context: RegistrationContext) -> None:
        if hasattr(context.server, "_run_strict_skill_scan_phase"):
            context.server._run_strict_skill_scan_phase(context)
        elif hasattr(context.server, "_run_strict_skill_scan_if_enabled"):
            context.server._run_strict_skill_scan_if_enabled(
                context.strict_scan,
                context.extra_skill_paths,
                context.include_bundled,
            )


class MetadataDrivenToolsPhase(RegistrationPhase):
    """Register ``recipes__*`` and ``skill_refs__*`` tools."""

    name = "metadata_driven_tools"

    def run(self, context: RegistrationContext) -> None:
        if hasattr(context.server, "_register_metadata_driven_tools"):
            context.server._register_metadata_driven_tools(context)
        else:
            try:
                from dcc_mcp_core.metadata_registration import register_metadata_driven_tools
            except ImportError:
                return
            paths = context.server.collect_skill_search_paths(
                extra_paths=context.extra_skill_paths,
                include_bundled=context.include_bundled,
                filter_existing=True,
            )
            register_metadata_driven_tools(
                context.server._server,
                dcc_name=context.server._dcc_name,
                extra_paths=paths,
            )


class IntrospectToolsPhase(RegistrationPhase):
    """Register the four ``dcc_introspect__*`` MCP tools."""

    name = "introspect_tools"

    def run(self, context: RegistrationContext) -> None:
        if hasattr(context.server, "_register_introspect_tools"):
            context.server._register_introspect_tools(context)
        else:
            try:
                from dcc_mcp_core.introspect import register_introspect_tools
            except ImportError:
                return
            register_introspect_tools(context.server._server, dcc_name=context.server._dcc_name)


class FeedbackToolPhase(RegistrationPhase):
    """Register the ``dcc_feedback__report`` MCP tool."""

    name = "feedback_tool"

    def run(self, context: RegistrationContext) -> None:
        if hasattr(context.server, "_register_feedback_tool"):
            context.server._register_feedback_tool(context)
        else:
            try:
                from dcc_mcp_core.feedback import register_feedback_tool
            except ImportError:
                return
            register_feedback_tool(context.server._server, dcc_name=context.server._dcc_name)


class QtUiInspectorPhase(RegistrationPhase):
    """Register the shared ``qt_ui_inspector__*`` tools."""

    name = "qt_ui_inspector"

    def run(self, context: RegistrationContext) -> None:
        if hasattr(context.server, "_register_qt_ui_inspector"):
            context.server._register_qt_ui_inspector(context)


class CapabilityManifestPhase(RegistrationPhase):
    """Register the ``dcc_capability_manifest`` MCP tool."""

    name = "capability_manifest"

    def run(self, context: RegistrationContext) -> None:
        if hasattr(context.server, "_register_capability_manifest_tool"):
            context.server._register_capability_manifest_tool(context)


class ProjectToolsPhase(RegistrationPhase):
    """Register the four ``project_*`` MCP tools."""

    name = "project_tools"

    def run(self, context: RegistrationContext) -> None:
        if hasattr(context.server, "_attach_project_tools"):
            context.server._attach_project_tools(context)


class ResourcesPhase(RegistrationPhase):
    """Publish ``scene://current`` + dynamic resource producers."""

    name = "resources"

    def run(self, context: RegistrationContext) -> None:
        if hasattr(context.server, "_attach_resources"):
            context.server._attach_resources(context)


class SkillCatalogReadyPhase(RegistrationPhase):
    """Signal that the skill catalog has been populated (readiness gate)."""

    name = "skill_catalog_ready"

    def run(self, context: RegistrationContext) -> None:
        if hasattr(context.server, "_readiness") and hasattr(context.server._readiness, "mark_skill_catalog_ready"):
            context.server._readiness.mark_skill_catalog_ready()
        elif hasattr(context.server, "_mark_skill_catalog_ready"):
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
