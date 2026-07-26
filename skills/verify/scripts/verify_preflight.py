"""verify_preflight — Run all verification checks in one call."""
from __future__ import annotations

from typing import Any

from .verify_capability import verify_capability
from .verify_environment import verify_environment
from .verify_gateway import verify_gateway
from .verify_instance import verify_instance


def verify_preflight(
    capability_query: str,
    dcc_type: str | None = None,
    instance_id: str | None = None,
) -> dict[str, Any]:
    """Run comprehensive preflight check.

    Args:
        capability_query: The capability you need.
        dcc_type: Target DCC type.
        instance_id: Specific instance.

    Returns:
        Comprehensive preflight report.
    """
    instance_result = verify_instance(
        dcc_type=dcc_type,
        instance_id=instance_id,
    )

    capability_result = verify_capability(
        query=capability_query,
        dcc_type=dcc_type,
        instance_id=instance_id,
    )

    env_result = verify_environment(
        instance_id=instance_id,
        dcc_type=dcc_type,
    )

    gateway_result = verify_gateway()

    # Determine if all clear
    all_clear = (
        instance_result.get("ready", False)
        and capability_result.get("available", False)
        and env_result.get("compatible", True)
        and gateway_result.get("reachable", True)
    )

    blockers: list[str] = []
    if not instance_result.get("ready"):
        blockers.append("No dispatch-ready instance available.")
    if not capability_result.get("available"):
        blockers.append(f"Capability '{capability_query}' not found.")
    if not env_result.get("compatible"):
        blockers.append("Environment has error-level compatibility issues.")
    if not gateway_result.get("reachable"):
        blockers.append("Gateway is unreachable.")

    return {
        "success": True,
        "all_clear": all_clear,
        "instance": instance_result,
        "capability": capability_result,
        "environment": env_result,
        "gateway": gateway_result,
        "ready_to_proceed": all_clear,
        "blockers": blockers,
    }
