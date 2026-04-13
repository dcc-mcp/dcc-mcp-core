"""Tests for AuditLog / AuditEntry, ActionValidator, VersionedRegistry, SemVer, VersionConstraint.

Scope
-----
- AuditLog: entries/successes/denials/entries_for_action/to_json/len
- AuditEntry: all fields (action/actor/duration_ms/outcome/outcome_detail/params_json/timestamp_ms)
- ActionValidator: from_schema_json/from_action_registry/validate (happy + error paths)
- VersionedRegistry: register_versioned/resolve/resolve_all/latest_version/versions/keys/remove/total_entries
- SemVer: parse/major/minor/patch/comparisons/matches_constraint
- VersionConstraint: parse/matches/str
"""

from __future__ import annotations

import contextlib
import json

from dcc_mcp_core import ActionRegistry
from dcc_mcp_core import ActionValidator
from dcc_mcp_core import SandboxContext
from dcc_mcp_core import SandboxPolicy
from dcc_mcp_core import SemVer
from dcc_mcp_core import VersionConstraint
from dcc_mcp_core import VersionedRegistry

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _make_ctx_with_audit() -> tuple[SandboxContext, AuditLog]:
    """Create a SandboxContext with allow/deny policy and execute several actions."""
    p = SandboxPolicy()
    p.allow_actions(["move", "scale", "rotate"])
    p.deny_actions(["delete", "nuke"])
    ctx = SandboxContext(p)
    ctx.execute_json("move", '{"x": 1, "y": 0}')
    ctx.execute_json("scale", '{"factor": 2}')
    ctx.execute_json("rotate", '{"angle": 90}')
    with contextlib.suppress(RuntimeError):
        ctx.execute_json("delete", "{}")
    with contextlib.suppress(RuntimeError):
        ctx.execute_json("nuke", "{}")
    return ctx, ctx.audit_log


# ===========================================================================
# AuditLog / AuditEntry
# ===========================================================================


class TestAuditLogBasic:
    """Basic AuditLog behaviour: counts and collections."""

    def test_len_total_entries(self):
        """len(audit_log) returns total number of entries (successes + denials)."""
        _ctx, al = _make_ctx_with_audit()
        assert len(al) == 5  # 3 successes + 2 denials

    def test_entries_returns_all(self):
        """entries() returns every recorded entry in insertion order."""
        _ctx, al = _make_ctx_with_audit()
        entries = al.entries()
        assert len(entries) == 5
        actions = [e.action for e in entries]
        assert actions == ["move", "scale", "rotate", "delete", "nuke"]

    def test_successes_only(self):
        """successes() returns only successful entries."""
        _ctx, al = _make_ctx_with_audit()
        successes = al.successes()
        assert len(successes) == 3
        assert all(e.outcome == "success" for e in successes)

    def test_denials_only(self):
        """denials() returns only denied entries."""
        _ctx, al = _make_ctx_with_audit()
        denials = al.denials()
        assert len(denials) == 2
        assert all(e.outcome == "denied" for e in denials)

    def test_entries_for_action_move(self):
        """entries_for_action filters by action name."""
        _ctx, al = _make_ctx_with_audit()
        entries = al.entries_for_action("move")
        assert len(entries) == 1
        assert entries[0].action == "move"
        assert entries[0].outcome == "success"

    def test_entries_for_action_delete_denied(self):
        """entries_for_action works for denied actions too."""
        _ctx, al = _make_ctx_with_audit()
        entries = al.entries_for_action("delete")
        assert len(entries) == 1
        assert entries[0].action == "delete"
        assert entries[0].outcome == "denied"

    def test_entries_for_nonexistent_action(self):
        """entries_for_action returns empty list for unknown action."""
        _ctx, al = _make_ctx_with_audit()
        entries = al.entries_for_action("no_such_action")
        assert entries == []

    def test_to_json_is_valid_json(self):
        """to_json() returns a parseable JSON string."""
        _ctx, al = _make_ctx_with_audit()
        raw = al.to_json()
        assert isinstance(raw, str)
        data = json.loads(raw)
        assert isinstance(data, list)
        assert len(data) == 5

    def test_to_json_contains_action_names(self):
        """to_json() JSON array contains expected action names."""
        _ctx, al = _make_ctx_with_audit()
        data = json.loads(al.to_json())
        names = [entry.get("action") for entry in data]
        assert "move" in names
        assert "delete" in names

    def test_empty_context_no_entries(self):
        """Fresh context with no executions has empty audit_log."""
        p = SandboxPolicy()
        ctx = SandboxContext(p)
        al = ctx.audit_log
        assert len(al) == 0
        assert al.entries() == []
        assert al.successes() == []
        assert al.denials() == []

    def test_entries_returns_list(self):
        """entries() returns a list, not a generator."""
        _ctx, al = _make_ctx_with_audit()
        result = al.entries()
        assert isinstance(result, list)

    def test_successes_returns_list(self):
        """successes() returns a list."""
        _ctx, al = _make_ctx_with_audit()
        assert isinstance(al.successes(), list)

    def test_denials_returns_list(self):
        """denials() returns a list."""
        _ctx, al = _make_ctx_with_audit()
        assert isinstance(al.denials(), list)

    def test_multiple_calls_same_action(self):
        """Multiple calls to the same action produce multiple entries."""
        p = SandboxPolicy()
        ctx = SandboxContext(p)
        ctx.execute_json("move", '{"x": 1}')
        ctx.execute_json("move", '{"x": 2}')
        ctx.execute_json("move", '{"x": 3}')
        al = ctx.audit_log
        entries = al.entries_for_action("move")
        assert len(entries) == 3

    def test_denied_count_increments_after_each_deny(self):
        """Each denied call adds one denial entry."""
        p = SandboxPolicy()
        p.deny_actions(["forbidden"])
        ctx = SandboxContext(p)
        for _ in range(3):
            with contextlib.suppress(RuntimeError):
                ctx.execute_json("forbidden", "{}")
        al = ctx.audit_log
        assert len(al.denials()) == 3


class TestAuditEntry:
    """Tests for AuditEntry field access."""

    def test_action_field(self):
        """AuditEntry.action returns the action name string."""
        p = SandboxPolicy()
        ctx = SandboxContext(p)
        ctx.execute_json("create_mesh", "{}")
        entry = ctx.audit_log.entries()[0]
        assert entry.action == "create_mesh"

    def test_outcome_success(self):
        """AuditEntry.outcome is 'success' for a successful execution."""
        p = SandboxPolicy()
        ctx = SandboxContext(p)
        ctx.execute_json("render", "{}")
        entry = ctx.audit_log.entries()[0]
        assert entry.outcome == "success"

    def test_outcome_denied(self):
        """AuditEntry.outcome is 'denied' for a denied execution."""
        p = SandboxPolicy()
        p.deny_actions(["rm_rf"])
        ctx = SandboxContext(p)
        with contextlib.suppress(RuntimeError):
            ctx.execute_json("rm_rf", "{}")
        entry = ctx.audit_log.denials()[0]
        assert entry.outcome == "denied"

    def test_duration_ms_non_negative(self):
        """AuditEntry.duration_ms is non-negative integer."""
        p = SandboxPolicy()
        ctx = SandboxContext(p)
        ctx.execute_json("update_mesh", "{}")
        entry = ctx.audit_log.entries()[0]
        assert isinstance(entry.duration_ms, int)
        assert entry.duration_ms >= 0

    def test_timestamp_ms_positive(self):
        """AuditEntry.timestamp_ms is a positive integer (Unix ms)."""
        p = SandboxPolicy()
        ctx = SandboxContext(p)
        ctx.execute_json("refresh", "{}")
        entry = ctx.audit_log.entries()[0]
        assert isinstance(entry.timestamp_ms, int)
        assert entry.timestamp_ms > 0

    def test_actor_field_accessible(self):
        """AuditEntry.actor is accessible (default None when not set)."""
        p = SandboxPolicy()
        ctx = SandboxContext(p)
        ctx.execute_json("ping", "{}")
        entry = ctx.audit_log.entries()[0]
        # actor is None when no actor has been set on the context
        assert entry.actor is None or isinstance(entry.actor, str)

    def test_actor_after_set_actor(self):
        """AuditEntry.actor reflects the actor set on context."""
        p = SandboxPolicy()
        ctx = SandboxContext(p)
        ctx.set_actor("agent-007")
        ctx.execute_json("query_scene", "{}")
        entry = ctx.audit_log.entries()[0]
        assert entry.actor == "agent-007"

    def test_outcome_detail_accessible(self):
        """AuditEntry.outcome_detail is accessible (None or string for success)."""
        p = SandboxPolicy()
        ctx = SandboxContext(p)
        ctx.execute_json("list_objects", "{}")
        entry = ctx.audit_log.entries()[0]
        # outcome_detail may be None for successful executions
        assert entry.outcome_detail is None or isinstance(entry.outcome_detail, str)

    def test_params_json_accessible(self):
        """AuditEntry.params_json is accessible as a string."""
        p = SandboxPolicy()
        ctx = SandboxContext(p)
        ctx.execute_json("move_mesh", '{"x": 10}')
        entry = ctx.audit_log.entries()[0]
        assert isinstance(entry.params_json, str)

    def test_repr_contains_action(self):
        """repr(AuditEntry) contains the action name."""
        p = SandboxPolicy()
        ctx = SandboxContext(p)
        ctx.execute_json("test_action_repr", "{}")
        entry = ctx.audit_log.entries()[0]
        assert "test_action_repr" in repr(entry)


# ===========================================================================
# ActionValidator
# ===========================================================================


class TestActionValidatorFromSchemaJson:
    """Tests for ActionValidator.from_schema_json factory."""

    def test_valid_object_passes(self):
        """Valid JSON matching schema returns (True, [])."""
        schema = '{"type":"object","properties":{"x":{"type":"number"}},"required":["x"]}'
        v = ActionValidator.from_schema_json(schema)
        ok, errors = v.validate('{"x": 42}')
        assert ok is True
        assert errors == []

    def test_missing_required_field(self):
        """Missing required field returns (False, [message])."""
        schema = '{"type":"object","properties":{"name":{"type":"string"}},"required":["name"]}'
        v = ActionValidator.from_schema_json(schema)
        ok, errors = v.validate("{}")
        assert ok is False
        assert len(errors) > 0
        assert any("name" in e for e in errors)

    def test_wrong_type_returns_error(self):
        """Wrong field type returns (False, [message])."""
        schema = '{"type":"object","properties":{"radius":{"type":"number"}},"required":["radius"]}'
        v = ActionValidator.from_schema_json(schema)
        ok, errors = v.validate('{"radius": "not_a_number"}')
        assert ok is False
        assert len(errors) > 0

    def test_extra_field_allowed(self):
        """Extra properties not in schema are tolerated (unless additionalProperties:false)."""
        schema = '{"type":"object","properties":{"x":{"type":"number"}}}'
        v = ActionValidator.from_schema_json(schema)
        ok, _errors = v.validate('{"x": 1, "extra": "field"}')
        assert ok is True

    def test_empty_object_no_required(self):
        """Empty object is valid when no required fields."""
        schema = '{"type":"object"}'
        v = ActionValidator.from_schema_json(schema)
        ok, errors = v.validate("{}")
        assert ok is True
        assert errors == []

    def test_multiple_required_fields(self):
        """Multiple required fields: passing both is valid."""
        schema = '{"type":"object","properties":{"a":{"type":"string"},"b":{"type":"integer"}},"required":["a","b"]}'
        v = ActionValidator.from_schema_json(schema)
        ok, errors = v.validate('{"a": "hello", "b": 7}')
        assert ok is True
        assert errors == []

    def test_multiple_required_one_missing(self):
        """Multiple required fields: missing one returns error."""
        schema = '{"type":"object","properties":{"a":{"type":"string"},"b":{"type":"integer"}},"required":["a","b"]}'
        v = ActionValidator.from_schema_json(schema)
        ok, errors = v.validate('{"a": "hello"}')
        assert ok is False
        assert len(errors) > 0

    def test_nested_object(self):
        """Nested object schema validates correctly."""
        schema = (
            '{"type":"object","properties":{'
            '"transform":{"type":"object","properties":{"x":{"type":"number"}},"required":["x"]}'
            '},"required":["transform"]}'
        )
        v = ActionValidator.from_schema_json(schema)
        ok, _errors = v.validate('{"transform": {"x": 1.0}}')
        assert ok is True

    def test_validate_returns_tuple(self):
        """Validate always returns a 2-tuple."""
        schema = '{"type":"object"}'
        v = ActionValidator.from_schema_json(schema)
        result = v.validate("{}")
        assert isinstance(result, tuple)
        assert len(result) == 2

    def test_errors_are_list_of_strings(self):
        """Error messages are a list of strings."""
        schema = '{"type":"object","properties":{"x":{"type":"number"}},"required":["x"]}'
        v = ActionValidator.from_schema_json(schema)
        _ok, errors = v.validate("{}")
        assert isinstance(errors, list)
        assert all(isinstance(e, str) for e in errors)


class TestActionValidatorFromActionRegistry:
    """Tests for ActionValidator.from_action_registry factory."""

    def _make_registry(self) -> ActionRegistry:
        reg = ActionRegistry()
        schema = (
            '{"type":"object","properties":{"radius":{"type":"number"},"name":{"type":"string"}},"required":["radius"]}'
        )
        reg.register(
            name="create_sphere",
            description="Create a sphere",
            category="geometry",
            dcc="maya",
            input_schema=schema,
        )
        return reg

    def test_valid_params(self):
        """Valid params for registered action pass validation."""
        reg = self._make_registry()
        v = ActionValidator.from_action_registry(reg, "create_sphere")
        ok, errors = v.validate('{"radius": 5.0}')
        assert ok is True
        assert errors == []

    def test_optional_field_included(self):
        """Optional field included is still valid."""
        reg = self._make_registry()
        v = ActionValidator.from_action_registry(reg, "create_sphere")
        ok, errors = v.validate('{"radius": 3.0, "name": "mySphere"}')
        assert ok is True
        assert errors == []

    def test_missing_required_radius(self):
        """Missing required 'radius' returns validation error."""
        reg = self._make_registry()
        v = ActionValidator.from_action_registry(reg, "create_sphere")
        ok, errors = v.validate("{}")
        assert ok is False
        assert any("radius" in e for e in errors)

    def test_wrong_type_for_radius(self):
        """String for numeric 'radius' returns validation error."""
        reg = self._make_registry()
        v = ActionValidator.from_action_registry(reg, "create_sphere")
        ok, errors = v.validate('{"radius": "big"}')
        assert ok is False
        assert len(errors) > 0

    def test_multiple_actions_independent(self):
        """Different validators from same registry are independent."""
        reg = ActionRegistry()
        schema_a = '{"type":"object","properties":{"a":{"type":"number"}},"required":["a"]}'
        schema_b = '{"type":"object","properties":{"b":{"type":"string"}},"required":["b"]}'
        reg.register(name="action_a", description="a", category="test", dcc="maya", input_schema=schema_a)
        reg.register(name="action_b", description="b", category="test", dcc="maya", input_schema=schema_b)
        va = ActionValidator.from_action_registry(reg, "action_a")
        vb = ActionValidator.from_action_registry(reg, "action_b")
        ok_a, _ = va.validate('{"a": 1}')
        ok_b, _ = vb.validate('{"b": "hello"}')
        assert ok_a is True
        assert ok_b is True
        # Cross-validate should fail
        bad_a, _ = va.validate('{"b": "hello"}')
        assert bad_a is False


# ===========================================================================
# SemVer
# ===========================================================================


class TestSemVer:
    """Tests for SemVer parsing and comparison."""

    def test_parse_valid(self):
        """SemVer.parse correctly parses a valid version string."""
        sv = SemVer.parse("3.5.2")
        assert sv.major == 3
        assert sv.minor == 5
        assert sv.patch == 2

    def test_parse_zero_version(self):
        """SemVer.parse handles 0.0.0."""
        sv = SemVer.parse("0.0.0")
        assert sv.major == 0
        assert sv.minor == 0
        assert sv.patch == 0

    def test_str_round_trip(self):
        """str(SemVer) returns the original version string."""
        sv = SemVer.parse("1.2.3")
        assert str(sv) == "1.2.3"

    def test_greater_than(self):
        """Higher version is greater."""
        sv_big = SemVer.parse("2.0.0")
        sv_small = SemVer.parse("1.9.9")
        assert sv_big > sv_small

    def test_less_than(self):
        """Lower version is less."""
        sv_big = SemVer.parse("2.0.0")
        sv_small = SemVer.parse("1.9.9")
        assert sv_small < sv_big

    def test_equal(self):
        """Same version is equal."""
        sv1 = SemVer.parse("1.2.3")
        sv2 = SemVer.parse("1.2.3")
        assert sv1 == sv2

    def test_not_equal(self):
        """Different versions are not equal."""
        sv1 = SemVer.parse("1.2.3")
        sv2 = SemVer.parse("1.2.4")
        assert sv1 != sv2

    def test_patch_difference(self):
        """Minor patch difference is compared correctly."""
        sv1 = SemVer.parse("1.0.0")
        sv2 = SemVer.parse("1.0.1")
        assert sv1 < sv2

    def test_minor_difference(self):
        """Minor version difference is compared correctly."""
        sv1 = SemVer.parse("1.1.0")
        sv2 = SemVer.parse("1.2.0")
        assert sv1 < sv2

    def test_major_dominates(self):
        """Major version dominates comparison."""
        sv1 = SemVer.parse("1.99.99")
        sv2 = SemVer.parse("2.0.0")
        assert sv1 < sv2

    def test_matches_constraint_gte(self):
        """matches_constraint with >= constraint."""
        sv = SemVer.parse("2.0.0")
        vc = VersionConstraint.parse(">=1.5.0")
        assert sv.matches_constraint(vc) is True

    def test_matches_constraint_lt(self):
        """matches_constraint returns False when version is below constraint."""
        sv = SemVer.parse("1.4.0")
        vc = VersionConstraint.parse(">=1.5.0")
        assert sv.matches_constraint(vc) is False

    def test_matches_constraint_exact(self):
        """matches_constraint with exact version."""
        sv = SemVer.parse("1.2.3")
        vc = VersionConstraint.parse("=1.2.3")
        assert sv.matches_constraint(vc) is True

    def test_major_field_type(self):
        """major, minor, patch are int."""
        sv = SemVer.parse("4.5.6")
        assert isinstance(sv.major, int)
        assert isinstance(sv.minor, int)
        assert isinstance(sv.patch, int)


# ===========================================================================
# VersionConstraint
# ===========================================================================


class TestVersionConstraint:
    """Tests for VersionConstraint parsing and matching."""

    def test_parse_gte(self):
        """VersionConstraint.parse with >= works."""
        vc = VersionConstraint.parse(">=1.0.0")
        assert str(vc) == ">=1.0.0"

    def test_parse_caret(self):
        """VersionConstraint.parse with ^ (compatible) works."""
        vc = VersionConstraint.parse("^1.2.0")
        assert str(vc) == "^1.2.0"

    def test_parse_wildcard(self):
        """VersionConstraint.parse with * works."""
        vc = VersionConstraint.parse("*")
        sv = SemVer.parse("999.0.0")
        assert vc.matches(sv) is True

    def test_matches_gte_pass(self):
        """Matching version satisfies >= constraint."""
        vc = VersionConstraint.parse(">=2.0.0")
        sv = SemVer.parse("3.0.0")
        assert vc.matches(sv) is True

    def test_matches_gte_fail(self):
        """Version below >= constraint fails."""
        vc = VersionConstraint.parse(">=2.0.0")
        sv = SemVer.parse("1.9.9")
        assert vc.matches(sv) is False

    def test_matches_caret_same_major(self):
        """^ constraint matches same major, higher minor."""
        vc = VersionConstraint.parse("^1.0.0")
        sv = SemVer.parse("1.5.0")
        assert vc.matches(sv) is True

    def test_matches_caret_different_major(self):
        """^ constraint does not match different major."""
        vc = VersionConstraint.parse("^1.0.0")
        sv = SemVer.parse("2.0.0")
        assert vc.matches(sv) is False

    def test_matches_exact(self):
        """Exact constraint matches only equal version."""
        vc = VersionConstraint.parse("=1.2.3")
        sv_exact = SemVer.parse("1.2.3")
        sv_other = SemVer.parse("1.2.4")
        assert vc.matches(sv_exact) is True
        assert vc.matches(sv_other) is False

    def test_str_preserves_constraint(self):
        """str(VersionConstraint) preserves original constraint string."""
        raw = ">=1.5.0"
        vc = VersionConstraint.parse(raw)
        assert str(vc) == raw


# ===========================================================================
# VersionedRegistry
# ===========================================================================


class TestVersionedRegistryBasic:
    """Basic registration and retrieval."""

    def test_register_and_resolve_latest(self):
        """Registering versions and resolving * returns latest."""
        vr = VersionedRegistry()
        vr.register_versioned("action_x", dcc="maya", version="1.0.0")
        vr.register_versioned("action_x", dcc="maya", version="2.0.0")
        result = vr.resolve("action_x", dcc="maya", constraint=">=1.0.0")
        assert result["version"] == "2.0.0"

    def test_resolve_caret_constraint(self):
        """^ constraint resolves to highest compatible version."""
        vr = VersionedRegistry()
        vr.register_versioned("action_x", dcc="maya", version="1.0.0")
        vr.register_versioned("action_x", dcc="maya", version="1.9.0")
        vr.register_versioned("action_x", dcc="maya", version="2.0.0")
        result = vr.resolve("action_x", dcc="maya", constraint="^1.0.0")
        assert result["version"] == "1.9.0"

    def test_resolve_exact(self):
        """Exact constraint resolves to the specific version."""
        vr = VersionedRegistry()
        vr.register_versioned("action_y", dcc="blender", version="3.0.0")
        vr.register_versioned("action_y", dcc="blender", version="3.5.0")
        result = vr.resolve("action_y", dcc="blender", constraint="=3.0.0")
        assert result["version"] == "3.0.0"

    def test_resolve_result_has_name_field(self):
        """Resolved result dict contains 'name' field."""
        vr = VersionedRegistry()
        vr.register_versioned("my_action", dcc="maya", version="1.0.0")
        result = vr.resolve("my_action", dcc="maya", constraint="*")
        assert result["name"] == "my_action"

    def test_resolve_result_has_dcc_field(self):
        """Resolved result dict contains 'dcc' field."""
        vr = VersionedRegistry()
        vr.register_versioned("action_z", dcc="houdini", version="1.0.0")
        result = vr.resolve("action_z", dcc="houdini", constraint="*")
        assert result["dcc"] == "houdini"

    def test_resolve_all_wildcard(self):
        """resolve_all with * returns all versions sorted ascending."""
        vr = VersionedRegistry()
        vr.register_versioned("multi", dcc="maya", version="1.0.0")
        vr.register_versioned("multi", dcc="maya", version="2.0.0")
        vr.register_versioned("multi", dcc="maya", version="1.5.0")
        results = vr.resolve_all("multi", dcc="maya", constraint="*")
        assert len(results) == 3
        versions = [r["version"] for r in results]
        assert versions == ["1.0.0", "1.5.0", "2.0.0"]

    def test_resolve_all_filtered(self):
        """resolve_all with constraint filters versions."""
        vr = VersionedRegistry()
        vr.register_versioned("action_f", dcc="maya", version="1.0.0")
        vr.register_versioned("action_f", dcc="maya", version="1.5.0")
        vr.register_versioned("action_f", dcc="maya", version="2.0.0")
        results = vr.resolve_all("action_f", dcc="maya", constraint="^1.0.0")
        versions = [r["version"] for r in results]
        assert "1.0.0" in versions
        assert "1.5.0" in versions
        assert "2.0.0" not in versions

    def test_latest_version(self):
        """latest_version returns the highest version string."""
        vr = VersionedRegistry()
        vr.register_versioned("act_lv", dcc="maya", version="1.0.0")
        vr.register_versioned("act_lv", dcc="maya", version="3.0.0")
        vr.register_versioned("act_lv", dcc="maya", version="2.0.0")
        assert vr.latest_version("act_lv", dcc="maya") == "3.0.0"

    def test_versions_sorted(self):
        """versions() returns sorted list of version strings."""
        vr = VersionedRegistry()
        vr.register_versioned("act_v", dcc="maya", version="2.0.0")
        vr.register_versioned("act_v", dcc="maya", version="1.0.0")
        vr.register_versioned("act_v", dcc="maya", version="1.5.0")
        assert vr.versions("act_v", dcc="maya") == ["1.0.0", "1.5.0", "2.0.0"]

    def test_keys_contains_name_dcc_pair(self):
        """keys() returns list of (name, dcc) tuples."""
        vr = VersionedRegistry()
        vr.register_versioned("act_k", dcc="maya", version="1.0.0")
        keys = vr.keys()
        assert ("act_k", "maya") in keys

    def test_keys_multiple_dccs(self):
        """keys() lists entries for multiple DCCs."""
        vr = VersionedRegistry()
        vr.register_versioned("shared", dcc="maya", version="1.0.0")
        vr.register_versioned("shared", dcc="blender", version="1.0.0")
        keys = vr.keys()
        assert ("shared", "maya") in keys
        assert ("shared", "blender") in keys

    def test_total_entries(self):
        """total_entries() counts all registered version entries."""
        vr = VersionedRegistry()
        vr.register_versioned("act_te", dcc="maya", version="1.0.0")
        vr.register_versioned("act_te", dcc="maya", version="2.0.0")
        vr.register_versioned("act_te", dcc="blender", version="1.0.0")
        assert vr.total_entries() == 3


class TestVersionedRegistryRemove:
    """Tests for VersionedRegistry.remove()."""

    def test_remove_by_constraint_returns_count(self):
        """remove() returns the count of removed versions."""
        vr = VersionedRegistry()
        vr.register_versioned("act_r", dcc="maya", version="1.0.0")
        vr.register_versioned("act_r", dcc="maya", version="1.5.0")
        vr.register_versioned("act_r", dcc="maya", version="2.0.0")
        removed = vr.remove("act_r", dcc="maya", constraint="^1.0.0")
        assert removed == 2  # 1.0.0 and 1.5.0 match ^1.0.0

    def test_remove_leaves_non_matching(self):
        """After remove, non-matching versions are still present."""
        vr = VersionedRegistry()
        vr.register_versioned("act_r2", dcc="maya", version="1.0.0")
        vr.register_versioned("act_r2", dcc="maya", version="2.0.0")
        vr.remove("act_r2", dcc="maya", constraint="^1.0.0")
        versions = vr.versions("act_r2", dcc="maya")
        assert versions == ["2.0.0"]

    def test_remove_all_wildcard(self):
        """remove() with * removes all versions."""
        vr = VersionedRegistry()
        vr.register_versioned("act_r3", dcc="maya", version="1.0.0")
        vr.register_versioned("act_r3", dcc="maya", version="2.0.0")
        removed = vr.remove("act_r3", dcc="maya", constraint="*")
        assert removed == 2
        versions = vr.versions("act_r3", dcc="maya")
        assert versions == []

    def test_remove_no_match_returns_zero(self):
        """remove() with no matching constraint returns 0."""
        vr = VersionedRegistry()
        vr.register_versioned("act_r4", dcc="maya", version="3.0.0")
        removed = vr.remove("act_r4", dcc="maya", constraint="^1.0.0")
        assert removed == 0

    def test_remove_only_affects_target_dcc(self):
        """remove() only removes versions for the specified DCC."""
        vr = VersionedRegistry()
        vr.register_versioned("shared_r", dcc="maya", version="1.0.0")
        vr.register_versioned("shared_r", dcc="blender", version="1.0.0")
        vr.remove("shared_r", dcc="maya", constraint="*")
        maya_versions = vr.versions("shared_r", dcc="maya")
        blender_versions = vr.versions("shared_r", dcc="blender")
        assert maya_versions == []
        assert blender_versions == ["1.0.0"]

    def test_remove_exact_version(self):
        """remove() with exact constraint removes only that version."""
        vr = VersionedRegistry()
        vr.register_versioned("act_re", dcc="maya", version="1.0.0")
        vr.register_versioned("act_re", dcc="maya", version="1.1.0")
        vr.register_versioned("act_re", dcc="maya", version="1.2.0")
        removed = vr.remove("act_re", dcc="maya", constraint="=1.1.0")
        assert removed == 1
        versions = vr.versions("act_re", dcc="maya")
        assert "1.1.0" not in versions
        assert "1.0.0" in versions
        assert "1.2.0" in versions


class TestVersionedRegistryMultiDcc:
    """Cross-DCC isolation and multi-version scenarios."""

    def test_different_dccs_independent(self):
        """Versions registered for different DCCs are independent."""
        vr = VersionedRegistry()
        vr.register_versioned("action", dcc="maya", version="2.0.0")
        vr.register_versioned("action", dcc="blender", version="1.0.0")
        maya_result = vr.resolve("action", dcc="maya", constraint="*")
        blender_result = vr.resolve("action", dcc="blender", constraint="*")
        assert maya_result["version"] == "2.0.0"
        assert blender_result["version"] == "1.0.0"

    def test_keys_unique_per_dcc(self):
        """Each (name, dcc) pair appears only once in keys."""
        vr = VersionedRegistry()
        vr.register_versioned("action", dcc="maya", version="1.0.0")
        vr.register_versioned("action", dcc="maya", version="2.0.0")
        vr.register_versioned("action", dcc="blender", version="1.0.0")
        keys = vr.keys()
        assert keys.count(("action", "maya")) == 1
        assert keys.count(("action", "blender")) == 1

    def test_total_entries_across_dccs(self):
        """total_entries() counts all version entries across all DCCs."""
        vr = VersionedRegistry()
        vr.register_versioned("a", dcc="maya", version="1.0.0")
        vr.register_versioned("a", dcc="maya", version="2.0.0")
        vr.register_versioned("a", dcc="blender", version="1.0.0")
        vr.register_versioned("b", dcc="maya", version="1.0.0")
        assert vr.total_entries() == 4

    def test_resolve_all_empty_after_all_removed(self):
        """resolve_all returns empty list after all versions removed."""
        vr = VersionedRegistry()
        vr.register_versioned("emp", dcc="maya", version="1.0.0")
        vr.remove("emp", dcc="maya", constraint="*")
        results = vr.resolve_all("emp", dcc="maya", constraint="*")
        assert results == []
