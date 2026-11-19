"""Shared capacity-contract helpers for JSON bridges (issue #2821).

This slice defines reusable declaration parsing, clamping, byte-budget, and
error-data helpers. It does not install a WebSocket limit, change a handshake,
or change adapter behavior.
"""

from __future__ import annotations

from collections.abc import Mapping
from dataclasses import dataclass
import json
from typing import Any
from typing import Dict
from typing import Optional
from typing import Union

CAPACITY_CAPABILITY_KEY = "dcc-mcp.capacity.v1"
CAPACITY_SCHEMA_VERSION = 1
LEGACY_RECEIVE_BYTES = 65535
MIN_RECEIVE_BYTES = 256
MAX_RECEIVE_BYTES = 1048576
MAX_U64 = 18446744073709551615

OUTCOME_NOT_DISPATCHED = "not_dispatched"
OUTCOME_UNKNOWN = "unknown"
VALID_OUTCOMES = (OUTCOME_NOT_DISPATCHED, OUTCOME_UNKNOWN)
ERROR_CODE_CAPACITY = "capacity_error"
COMPACT_ERROR_REQUIRED_FIELDS = ("code", "message", "data", "outcome")
COMPACT_ERROR_OPTIONAL_FIELDS = ("actual_bytes",)

DIRECTION_REQUEST = "request"
DIRECTION_RESPONSE = "response"
VALID_DIRECTIONS = (DIRECTION_REQUEST, DIRECTION_RESPONSE)

ERROR_INVALID_CAPACITY_VALUE = "invalid_capacity_value"
ERROR_CONFLICTING_CAPACITY_DECLARATION = "conflicting_capacity_declaration"
ERROR_UNSUPPORTED_CAPACITY_VERSION = "unsupported_capacity_version"

LEGACY_ASSUMED = "legacy_assumed"
DECLARED = "declared"


class BridgeCapacityContract:
    """Namespace for version-1 bridge capacity contract constants."""

    VERSION = CAPACITY_SCHEMA_VERSION
    CAPABILITY_KEY = CAPACITY_CAPABILITY_KEY
    LEGACY_RECEIVE_BYTES = LEGACY_RECEIVE_BYTES
    MAX_U64 = MAX_U64


class CapacityContractError(ValueError):
    """A capacity declaration violates the version-1 contract."""

    def __init__(self, code: str, message: str) -> None:
        super().__init__(message)
        self.code = code


@dataclass(frozen=True)
class DirectionalReceive:
    """Receive capacity for one logical direction of a connection."""

    incoming: int
    outgoing: int

    def __post_init__(self) -> None:
        object.__setattr__(self, "incoming", _validate_capacity(self.incoming))
        object.__setattr__(self, "outgoing", _validate_capacity(self.outgoing))

    def to_dict(self) -> Dict[str, int]:
        return {"incoming": self.incoming, "outgoing": self.outgoing}


@dataclass(frozen=True)
class CapacityPolicy:
    """Local bounds that a peer declaration cannot exceed."""

    min_bytes: int = MIN_RECEIVE_BYTES
    max_bytes: int = MAX_RECEIVE_BYTES

    def __post_init__(self) -> None:
        _validate_policy_bound(self.min_bytes, "min_bytes")
        _validate_policy_bound(self.max_bytes, "max_bytes")
        if self.max_bytes < self.min_bytes:
            raise CapacityContractError(
                ERROR_INVALID_CAPACITY_VALUE,
                "capacity policy requires 1 <= min_bytes <= max_bytes",
            )

    def clamp(self, value: int) -> int:
        return min(max(_validate_capacity(value), self.min_bytes), self.max_bytes)


@dataclass(frozen=True)
class DirectionalCapacityState:
    """Effective receive capacity and how it was obtained."""

    receive: DirectionalReceive
    negotiation: str
    declared_version: Optional[int] = None


@dataclass(frozen=True)
class ByteBudget:
    """Result of checking complete serialized bytes against a limit."""

    actual_bytes: int
    max_bytes: int
    fits: bool

    def to_dict(self) -> Dict[str, Any]:
        return {
            "actual_bytes": self.actual_bytes,
            "max_bytes": self.max_bytes,
            "fits": self.fits,
        }


def _validate_capacity(value: Any) -> int:
    # bool is an int subclass; contract values must be genuine integers.
    if isinstance(value, bool) or not isinstance(value, int):
        raise CapacityContractError(
            ERROR_INVALID_CAPACITY_VALUE,
            "capacity values must be positive integers",
        )
    if value <= 0:
        raise CapacityContractError(
            ERROR_INVALID_CAPACITY_VALUE,
            "capacity values must be greater than zero",
        )
    if value > MAX_U64:
        raise CapacityContractError(
            ERROR_INVALID_CAPACITY_VALUE,
            "capacity values must be no greater than 2**64 - 1",
        )
    return value


def _validate_policy_bound(value: Any, name: str) -> int:
    validated = _validate_capacity(value)
    if validated == value:
        return validated
    raise CapacityContractError(
        ERROR_INVALID_CAPACITY_VALUE,
        f"capacity policy {name} must be a genuine integer",
    )


def validate_final_bytes(serialized: Union[str, bytes], max_bytes: int) -> ByteBudget:
    """Check the exact serialized content that the caller will send."""
    payload = serialized.encode("utf-8") if isinstance(serialized, str) else serialized
    if not isinstance(payload, bytes):
        raise TypeError("serialized must be str or bytes")
    limit = _validate_capacity(max_bytes)
    actual = len(payload)
    return ByteBudget(actual, limit, actual <= limit)


def serialized_json_bytes(message: Any) -> bytes:
    """Serialize a JSON-compatible message once for budget testing."""
    return json.dumps(message, ensure_ascii=False, separators=(",", ":")).encode("utf-8")


def _iter_error_strings(value: Any):
    """Yield every decoded string carried by a compact error payload."""
    if isinstance(value, str):
        yield value
    elif isinstance(value, Mapping):
        for key, nested in value.items():
            yield from _iter_error_strings(key)
            yield from _iter_error_strings(nested)
    elif isinstance(value, (list, tuple)):
        for nested in value:
            yield from _iter_error_strings(nested)


def make_capacity_error_data(
    direction: str,
    max_bytes: int,
    outcome: str = OUTCOME_UNKNOWN,
    actual_bytes: Optional[int] = None,
) -> Dict[str, Any]:
    """Build stable JSON-RPC error data without echoing request content."""
    if direction not in VALID_DIRECTIONS:
        raise CapacityContractError(ERROR_INVALID_CAPACITY_VALUE, "direction must be request or response")
    if outcome not in VALID_OUTCOMES:
        raise CapacityContractError(ERROR_INVALID_CAPACITY_VALUE, "outcome must be not_dispatched or unknown")
    limit = _validate_capacity(max_bytes)
    validated_actual: Optional[int] = None
    if actual_bytes is not None:
        validated_actual = _validate_capacity(actual_bytes)
    if outcome == OUTCOME_NOT_DISPATCHED and (
        direction != DIRECTION_REQUEST or validated_actual is None or validated_actual <= limit
    ):
        raise CapacityContractError(
            ERROR_INVALID_CAPACITY_VALUE,
            "not_dispatched requires a request whose actual_bytes exceeds max_bytes",
        )
    data: Dict[str, Any] = {
        "direction": direction,
        "max_bytes": limit,
        "outcome": outcome,
    }
    if validated_actual is not None:
        data["actual_bytes"] = validated_actual
    return data


def make_compact_capacity_error(
    direction: str,
    max_bytes: int,
    error_budget_bytes: int,
    correlation_id: str,
    outcome: str = OUTCOME_UNKNOWN,
    actual_bytes: Optional[int] = None,
    message: Optional[str] = None,
) -> Dict[str, Any]:
    """Build a self-bounded structured error that never echoes correlation IDs."""
    error_data = make_capacity_error_data(direction, max_bytes, outcome, actual_bytes)
    text = message or (
        "Request exceeded the byte budget."
        if outcome == OUTCOME_NOT_DISPATCHED
        else "Result outcome is unknown; inspect state before retrying."
    )
    payload: Dict[str, Any] = {
        "code": ERROR_CODE_CAPACITY,
        "message": text,
        "data": error_data,
        "outcome": outcome,
    }
    if "actual_bytes" in error_data:
        payload["actual_bytes"] = error_data["actual_bytes"]
    serialized = json.dumps(payload, ensure_ascii=False, separators=(",", ":"))
    budget = validate_final_bytes(serialized, error_budget_bytes)
    if not budget.fits:
        raise CapacityContractError(
            ERROR_INVALID_CAPACITY_VALUE,
            "compact capacity error exceeds error_budget_bytes",
        )
    if correlation_id and (
        correlation_id in serialized or any(correlation_id in text for text in _iter_error_strings(payload))
    ):
        raise CapacityContractError(
            ERROR_INVALID_CAPACITY_VALUE,
            "compact capacity error must not contain correlation_id",
        )
    return payload


def parse_capacity_error_data(data: Any) -> Dict[str, Any]:
    """Validate structured capacity error data from a JSON-RPC error object."""
    if not isinstance(data, Mapping):
        raise CapacityContractError(
            ERROR_INVALID_CAPACITY_VALUE,
            "capacity error data must be an object",
        )
    direction = data.get("direction")
    outcome = data.get("outcome")
    if direction not in VALID_DIRECTIONS:
        raise CapacityContractError(ERROR_INVALID_CAPACITY_VALUE, "direction must be request or response")
    if outcome not in VALID_OUTCOMES:
        raise CapacityContractError(ERROR_INVALID_CAPACITY_VALUE, "outcome must be not_dispatched or unknown")
    if "max_bytes" not in data:
        raise CapacityContractError(ERROR_INVALID_CAPACITY_VALUE, "max_bytes is required")
    return make_capacity_error_data(
        direction,
        data["max_bytes"],
        outcome,
        data.get("actual_bytes"),
    )


def parse_capacity_declaration(
    capabilities: Optional[Mapping[str, Any]],
    policy: Optional[CapacityPolicy] = None,
) -> DirectionalCapacityState:
    """Parse a version-1 declaration from a bridge capabilities map."""
    effective_policy = policy or CapacityPolicy()
    if not capabilities or CAPACITY_CAPABILITY_KEY not in capabilities:
        receive = DirectionalReceive(
            effective_policy.clamp(LEGACY_RECEIVE_BYTES), effective_policy.clamp(LEGACY_RECEIVE_BYTES)
        )
        return DirectionalCapacityState(receive, LEGACY_ASSUMED)
    declaration = capabilities[CAPACITY_CAPABILITY_KEY]
    if not isinstance(declaration, Mapping):
        raise CapacityContractError(ERROR_INVALID_CAPACITY_VALUE, "capacity declaration must be an object")
    version = declaration.get("version", CAPACITY_SCHEMA_VERSION)
    if isinstance(version, bool) or not isinstance(version, int) or version != CAPACITY_SCHEMA_VERSION:
        raise CapacityContractError(ERROR_UNSUPPORTED_CAPACITY_VERSION, "unsupported version")
    flat = "receive_max_bytes" in declaration
    nested = "receive" in declaration
    if flat == nested:
        raise CapacityContractError(
            ERROR_CONFLICTING_CAPACITY_DECLARATION, "capacity declaration must use one receive shape"
        )
    if flat:
        value = effective_policy.clamp(declaration["receive_max_bytes"])
        receive = DirectionalReceive(value, value)
    else:
        directional = declaration["receive"]
        if not isinstance(directional, Mapping):
            raise CapacityContractError(ERROR_INVALID_CAPACITY_VALUE, "receive declaration must be an object")
        incoming = directional.get("incoming", LEGACY_RECEIVE_BYTES)
        outgoing = directional.get("outgoing", LEGACY_RECEIVE_BYTES)
        receive = DirectionalReceive(effective_policy.clamp(incoming), effective_policy.clamp(outgoing))
    return DirectionalCapacityState(receive, DECLARED, CAPACITY_SCHEMA_VERSION)
