"""Compatibility shim for the bridge capacity contract.

The canonical implementation lives in :mod:`dcc_mcp_core.wire.capacity`.
Import it directly for new code; this module preserves the original v1 import
path and root-level ``BridgeCapacityContract`` export.
"""

from dcc_mcp_core.wire.capacity import *  # noqa: F403
from dcc_mcp_core.wire.capacity import CAPACITY_CAPABILITY_KEY  # noqa: F401
from dcc_mcp_core.wire.capacity import CAPACITY_SCHEMA_VERSION  # noqa: F401
from dcc_mcp_core.wire.capacity import COMPACT_ERROR_OPTIONAL_FIELDS  # noqa: F401
from dcc_mcp_core.wire.capacity import COMPACT_ERROR_REQUIRED_FIELDS  # noqa: F401
from dcc_mcp_core.wire.capacity import LEGACY_RECEIVE_BYTES  # noqa: F401
from dcc_mcp_core.wire.capacity import MAX_RECEIVE_BYTES  # noqa: F401
from dcc_mcp_core.wire.capacity import MIN_RECEIVE_BYTES  # noqa: F401
from dcc_mcp_core.wire.capacity import BridgeCapacityContract  # noqa: F401
from dcc_mcp_core.wire.capacity import ByteBudget  # noqa: F401
from dcc_mcp_core.wire.capacity import CapacityContractError  # noqa: F401
from dcc_mcp_core.wire.capacity import CapacityPolicy  # noqa: F401
from dcc_mcp_core.wire.capacity import DirectionalCapacityState  # noqa: F401
from dcc_mcp_core.wire.capacity import DirectionalReceive  # noqa: F401
from dcc_mcp_core.wire.capacity import make_capacity_error_data  # noqa: F401
from dcc_mcp_core.wire.capacity import make_compact_capacity_error  # noqa: F401
from dcc_mcp_core.wire.capacity import parse_capacity_declaration  # noqa: F401
from dcc_mcp_core.wire.capacity import parse_capacity_error_data  # noqa: F401
from dcc_mcp_core.wire.capacity import serialized_json_bytes  # noqa: F401
from dcc_mcp_core.wire.capacity import validate_final_bytes  # noqa: F401

__all__ = [
    "BridgeCapacityContract",
    "ByteBudget",
    "CapacityContractError",
    "CapacityPolicy",
    "DirectionalCapacityState",
    "DirectionalReceive",
    "make_capacity_error_data",
    "make_compact_capacity_error",
    "parse_capacity_declaration",
    "parse_capacity_error_data",
    "serialized_json_bytes",
    "validate_final_bytes",
]
