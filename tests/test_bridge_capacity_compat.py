"""Compatibility tests for the bridge capacity import paths (issue #2821)."""

from __future__ import annotations

import dcc_mcp_core
from dcc_mcp_core import BridgeCapacityContract
from dcc_mcp_core import bridge_capacity
from dcc_mcp_core.wire import capacity


def test_root_lazy_export_uses_canonical_namespace_class():
    assert dcc_mcp_core.BridgeCapacityContract is BridgeCapacityContract
    assert BridgeCapacityContract is capacity.BridgeCapacityContract
    assert bridge_capacity.BridgeCapacityContract is capacity.BridgeCapacityContract


def test_legacy_module_reexports_canonical_symbols_and_values():
    assert bridge_capacity.CAPACITY_CAPABILITY_KEY == capacity.CAPACITY_CAPABILITY_KEY
    assert bridge_capacity.MAX_RECEIVE_BYTES == capacity.MAX_RECEIVE_BYTES
    assert bridge_capacity.CapacityContractError is capacity.CapacityContractError
    assert bridge_capacity.CapacityPolicy is capacity.CapacityPolicy
    assert bridge_capacity.make_compact_capacity_error is capacity.make_compact_capacity_error


def test_legacy_module_shares_mutable_state_with_canonical_module():
    policy = bridge_capacity.CapacityPolicy(512, 2048)
    declaration = {"dcc-mcp.capacity.v1": {"receive_max_bytes": 4096}}
    via_legacy = bridge_capacity.parse_capacity_declaration(declaration, policy)
    via_canonical = capacity.parse_capacity_declaration(declaration, policy)

    assert via_legacy.receive.to_dict() == via_canonical.receive.to_dict()
