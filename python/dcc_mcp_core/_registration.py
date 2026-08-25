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
import warnings


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


def _run_adapter_extension(context: RegistrationContext, name: str) -> bool:
    """Run a legacy adapter phase extension when a subclass still defines it.

    ``DccServerBase`` no longer duplicates the standard phase implementations.
    Existing adapters may keep an override during the compatibility window;
    new adapters should supply a custom :class:`RegistrationPhase` instead.
    """
    extension = getattr(context.server, name, None)
    if extension is None:
        return False
    warnings.warn(
        f"{type(context.server).__name__}.{name} is a legacy registration hook; "
        "supply a custom RegistrationPhase instead",
        DeprecationWarning,
        stacklevel=3,
    )
    extension(context)
    return True


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
        if _run_adapter_extension(context, "_register_core_builtin_actions"):
            return
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
        if _run_adapter_extension(context, "_run_strict_skill_scan_phase"):
            return
        if hasattr(context.server, "_run_strict_skill_scan_if_enabled"):
            context.server._run_strict_skill_scan_if_enabled(
                context.strict_scan,
                context.extra_skill_paths,
                context.include_bundled,
            )


class MetadataDrivenToolsPhase(RegistrationPhase):
    """Register ``recipes__*`` and ``skill_refs__*`` tools."""

    name = "metadata_driven_tools"

    def run(self, context: RegistrationContext) -> None:
        if _run_adapter_extension(context, "_register_metadata_driven_tools"):
            return
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
        if _run_adapter_extension(context, "_register_introspect_tools"):
            return
        try:
            from dcc_mcp_core.introspect import register_introspect_tools
        except ImportError:
            return
        register_introspect_tools(context.server._server, dcc_name=context.server._dcc_name)


class FeedbackToolPhase(RegistrationPhase):
    """Register the shared ``dcc_feedback__report`` gateway forwarder."""

    name = "feedback_tool"

    def run(self, context: RegistrationContext) -> None:
        try:
            from dcc_mcp_core._server.finding_context import finding_context_for_server
            from dcc_mcp_core.feedback import register_feedback_tool
        except ImportError:
            return
        server = context.server
        config = getattr(server, "_config", None)
        gateway_port = int(getattr(config, "gateway_port", 0) or 0)

        def instance_id_provider() -> str | None:
            return server.instance_id

        def finding_context_provider():
            return finding_context_for_server(server)

        register_feedback_tool(
            server._server,
            dcc_name=server._dcc_name,
            store=server.feedback_store,
            gateway_port=gateway_port,
            instance_id_provider=instance_id_provider,
            finding_context_provider=finding_context_provider,
        )


class QtUiInspectorPhase(RegistrationPhase):
    """Register the shared ``qt_ui_inspector__*`` tools."""

    name = "qt_ui_inspector"

    def run(self, context: RegistrationContext) -> None:
        _run_adapter_extension(context, "_register_qt_ui_inspector")


class CapabilityManifestPhase(RegistrationPhase):
    """Register the ``dcc_capability_manifest`` MCP tool."""

    name = "capability_manifest"

    def run(self, context: RegistrationContext) -> None:
        _run_adapter_extension(context, "_register_capability_manifest_tool")


class ProjectToolsPhase(RegistrationPhase):
    """Register the four ``project_*`` MCP tools."""

    name = "project_tools"

    def run(self, context: RegistrationContext) -> None:
        _run_adapter_extension(context, "_attach_project_tools")


class ResourcesPhase(RegistrationPhase):
    """Publish ``scene://current`` + dynamic resource producers."""

    name = "resources"

    def run(self, context: RegistrationContext) -> None:
        _run_adapter_extension(context, "_attach_resources")


class SkillCatalogReadyPhase(RegistrationPhase):
    """Signal that the skill catalog has been populated (readiness gate)."""

    name = "skill_catalog_ready"

    def run(self, context: RegistrationContext) -> None:
        if _run_adapter_extension(context, "_mark_skill_catalog_ready"):
            return
        if hasattr(context.server, "_readiness") and hasattr(context.server._readiness, "mark_skill_catalog_ready"):
            context.server._readiness.mark_skill_catalog_ready()


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
