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
