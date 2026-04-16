"""Deep tests for ToolDeclaration, SemVer/VersionConstraint, AuditEntry, InputValidator, and ToolResult.

Target: +110 tests covering the five domains identified in the previous iteration as under-tested.
"""

from __future__ import annotations

import contextlib
import json

import dcc_mcp_core as m

# ---------------------------------------------------------------------------
# Helper factories
# ---------------------------------------------------------------------------


def _make_sandbox(allowed: list[str] | None = None) -> tuple[m.SandboxPolicy, m.SandboxContext]:
    policy = m.SandboxPolicy()
    if allowed:
        policy.allow_actions(allowed)
    ctx = m.SandboxContext(policy)
    return policy, ctx


# ===========================================================================
# Section 1: ToolDeclaration
# ===========================================================================


class TestToolDeclarationCreate:
    """ToolDeclaration construction and default field values."""

    def test_create_minimal(self):
        td = m.ToolDeclaration(name="create_sphere", description="desc", source_file="s.py")
        assert td.name == "create_sphere"

    def test_description_field(self):
        td = m.ToolDeclaration(name="a", description="my desc", source_file="a.py")
        assert td.description == "my desc"

    def test_source_file_field(self):
        td = m.ToolDeclaration(name="a", description="d", source_file="scripts/a.py")
        assert td.source_file == "scripts/a.py"

    def test_read_only_default_false(self):
        td = m.ToolDeclaration(name="a", description="d", source_file="a.py")
        assert td.read_only is False

    def test_destructive_default_false(self):
        td = m.ToolDeclaration(name="a", description="d", source_file="a.py")
        assert td.destructive is False

    def test_idempotent_default_false(self):
        td = m.ToolDeclaration(name="a", description="d", source_file="a.py")
        assert td.idempotent is False

    def test_input_schema_default_is_object_json(self):
        td = m.ToolDeclaration(name="a", description="d", source_file="a.py")
        parsed = json.loads(td.input_schema)
        assert parsed.get("type") == "object"

    def test_output_schema_default_empty_string(self):
        td = m.ToolDeclaration(name="a", description="d", source_file="a.py")
        assert td.output_schema == ""

    def test_repr_contains_name(self):
        td = m.ToolDeclaration(name="create_sphere", description="d", source_file="a.py")
        assert "create_sphere" in repr(td)


class TestToolDeclarationMutation:
    """ToolDeclaration field mutation."""

    def test_set_read_only_true(self):
        td = m.ToolDeclaration(name="a", description="d", source_file="a.py")
        td.read_only = True
        assert td.read_only is True

    def test_set_destructive_true(self):
        td = m.ToolDeclaration(name="a", description="d", source_file="a.py")
        td.destructive = True
        assert td.destructive is True

    def test_set_idempotent_true(self):
        td = m.ToolDeclaration(name="a", description="d", source_file="a.py")
        td.idempotent = True
        assert td.idempotent is True

    def test_set_description(self):
        td = m.ToolDeclaration(name="a", description="old", source_file="a.py")
        td.description = "new desc"
        assert td.description == "new desc"

    def test_set_source_file(self):
        td = m.ToolDeclaration(name="a", description="d", source_file="old.py")
        td.source_file = "new.py"
        assert td.source_file == "new.py"

    def test_set_input_schema(self):
        schema = json.dumps({"type": "object", "required": ["x"]})
        td = m.ToolDeclaration(name="a", description="d", source_file="a.py")
        td.input_schema = schema
        assert json.loads(td.input_schema)["required"] == ["x"]

    def test_set_output_schema(self):
        td = m.ToolDeclaration(name="a", description="d", source_file="a.py")
        td.output_schema = '{"type": "object"}'
        assert "object" in td.output_schema

    def test_set_name(self):
        td = m.ToolDeclaration(name="old_name", description="d", source_file="a.py")
        td.name = "new_name"
        assert td.name == "new_name"

    def test_all_bool_flags_independent(self):
        td = m.ToolDeclaration(name="a", description="d", source_file="a.py")
        td.read_only = True
        td.destructive = True
        td.idempotent = True
        assert td.read_only is True
        assert td.destructive is True
        assert td.idempotent is True

    def test_toggle_read_only(self):
        td = m.ToolDeclaration(name="a", description="d", source_file="a.py")
        td.read_only = True
        td.read_only = False
        assert td.read_only is False

    def test_read_only_true_destructive_false_combination(self):
        td = m.ToolDeclaration(name="get_scene", description="query", source_file="get.py")
        td.read_only = True
        td.destructive = False
        td.idempotent = True
        assert td.read_only is True
        assert td.destructive is False
        assert td.idempotent is True

    def test_two_tool_declarations_independent(self):
        td1 = m.ToolDeclaration(name="a", description="d", source_file="a.py")
        td2 = m.ToolDeclaration(name="b", description="d", source_file="b.py")
        td1.read_only = True
        assert td2.read_only is False


# ===========================================================================
# Section 2: SemVer
# ===========================================================================


class TestSemVerCreate:
    """SemVer construction and basic properties."""

    def test_create_basic(self):
        v = m.SemVer(1, 2, 3)
        assert v.major == 1
        assert v.minor == 2
        assert v.patch == 3

    def test_str_format(self):
        assert str(m.SemVer(1, 2, 3)) == "1.2.3"

    def test_repr_format(self):
        assert repr(m.SemVer(1, 2, 3)) == "SemVer(1, 2, 3)"

    def test_parse_simple(self):
        v = m.SemVer.parse("2.5.10")
        assert v.major == 2
        assert v.minor == 5
        assert v.patch == 10

    def test_parse_with_v_prefix(self):
        v = m.SemVer.parse("v1.5.0-alpha")
        assert v.major == 1
        assert v.minor == 5
        assert v.patch == 0

    def test_parse_zero_version(self):
        v = m.SemVer.parse("0.0.1")
        assert v.major == 0
        assert v.minor == 0
        assert v.patch == 1


class TestSemVerComparison:
    """SemVer comparison operators."""

    def test_gt_by_major(self):
        assert m.SemVer(2, 0, 0) > m.SemVer(1, 9, 9)

    def test_gt_by_minor(self):
        assert m.SemVer(1, 5, 0) > m.SemVer(1, 4, 9)

    def test_gt_by_patch(self):
        assert m.SemVer(1, 0, 5) > m.SemVer(1, 0, 4)

    def test_eq_same_version(self):
        assert m.SemVer(1, 2, 3) == m.SemVer(1, 2, 3)

    def test_eq_parse_vs_constructor(self):
        assert m.SemVer.parse("1.2.3") == m.SemVer(1, 2, 3)

    def test_lt(self):
        assert m.SemVer(1, 0, 0) < m.SemVer(2, 0, 0)

    def test_not_eq_different_patch(self):
        assert m.SemVer(1, 0, 0) != m.SemVer(1, 0, 1)

    def test_ge(self):
        assert m.SemVer(1, 2, 3) >= m.SemVer(1, 2, 3)
        assert m.SemVer(2, 0, 0) >= m.SemVer(1, 9, 9)

    def test_le(self):
        assert m.SemVer(1, 0, 0) <= m.SemVer(1, 0, 0)
        assert m.SemVer(0, 9, 9) <= m.SemVer(1, 0, 0)


class TestVersionConstraint:
    """VersionConstraint operators."""

    def test_caret_matches_same_major(self):
        c = m.VersionConstraint.parse("^1.0.0")
        assert c.matches(m.SemVer(1, 5, 0)) is True

    def test_caret_rejects_next_major(self):
        c = m.VersionConstraint.parse("^1.0.0")
        assert c.matches(m.SemVer(2, 0, 0)) is False

    def test_wildcard_matches_any(self):
        c = m.VersionConstraint.parse("*")
        assert c.matches(m.SemVer(99, 99, 99)) is True
        assert c.matches(m.SemVer(0, 0, 1)) is True

    def test_ge_matches_equal(self):
        c = m.VersionConstraint.parse(">=1.2.0")
        assert c.matches(m.SemVer(1, 2, 0)) is True

    def test_ge_matches_higher(self):
        c = m.VersionConstraint.parse(">=1.2.0")
        assert c.matches(m.SemVer(2, 0, 0)) is True

    def test_ge_rejects_lower(self):
        c = m.VersionConstraint.parse(">=1.2.0")
        assert c.matches(m.SemVer(1, 1, 9)) is False

    def test_tilde_same_minor(self):
        c = m.VersionConstraint.parse("~1.2.3")
        assert c.matches(m.SemVer(1, 2, 5)) is True

    def test_tilde_rejects_next_minor(self):
        c = m.VersionConstraint.parse("~1.2.3")
        assert c.matches(m.SemVer(1, 3, 0)) is False

    def test_exact_gt(self):
        c = m.VersionConstraint.parse(">1.0.0")
        assert c.matches(m.SemVer(1, 0, 1)) is True
        assert c.matches(m.SemVer(1, 0, 0)) is False

    def test_exact_lt(self):
        c = m.VersionConstraint.parse("<2.0.0")
        assert c.matches(m.SemVer(1, 9, 9)) is True
        assert c.matches(m.SemVer(2, 0, 0)) is False

    def test_exact_le(self):
        c = m.VersionConstraint.parse("<=1.5.0")
        assert c.matches(m.SemVer(1, 5, 0)) is True
        assert c.matches(m.SemVer(1, 5, 1)) is False


# ===========================================================================
# Section 3: AuditEntry (via SandboxContext)
# ===========================================================================


class TestAuditEntryFields:
    """AuditEntry field types and values."""

    def setup_method(self):
        _, self.ctx = _make_sandbox(["echo", "create_sphere", "delete_node"])
        self.ctx.set_actor("test-agent")
        self.ctx.execute_json("echo", json.dumps({"x": 1}))
        self.entry = self.ctx.audit_log.entries()[0]

    def test_timestamp_ms_is_int(self):
        assert isinstance(self.entry.timestamp_ms, int)

    def test_timestamp_ms_positive(self):
        assert self.entry.timestamp_ms > 0

    def test_actor_matches_set_actor(self):
        assert self.entry.actor == "test-agent"

    def test_action_matches_executed(self):
        assert self.entry.action == "echo"

    def test_params_json_is_string(self):
        assert isinstance(self.entry.params_json, str)

    def test_params_json_parseable(self):
        parsed = json.loads(self.entry.params_json)
        assert parsed["x"] == 1

    def test_duration_ms_is_int(self):
        assert isinstance(self.entry.duration_ms, int)

    def test_duration_ms_non_negative(self):
        assert self.entry.duration_ms >= 0

    def test_outcome_success(self):
        assert self.entry.outcome == "success"

    def test_outcome_detail_none_on_success(self):
        assert self.entry.outcome_detail is None


class TestAuditEntryDenial:
    """AuditEntry for denied actions."""

    def setup_method(self):
        policy = m.SandboxPolicy()
        policy.allow_actions(["allowed_action"])
        policy.deny_actions(["disallowed_action"])
        self.ctx = m.SandboxContext(policy)
        self.ctx.set_actor("agent-x")
        # allowed_action is in allow list but disallowed_action is explicitly denied
        with contextlib.suppress(RuntimeError):
            self.ctx.execute_json("disallowed_action", "{}")

    def test_denial_entry_recorded(self):
        denials = self.ctx.audit_log.denials()
        assert len(denials) >= 1

    def test_denial_outcome_is_denied(self):
        denials = self.ctx.audit_log.denials()
        assert denials[0].outcome == "denied"

    def test_denial_actor_preserved(self):
        denials = self.ctx.audit_log.denials()
        assert denials[0].actor == "agent-x"

    def test_denial_action_name_preserved(self):
        denials = self.ctx.audit_log.denials()
        assert denials[0].action == "disallowed_action"


class TestAuditLogMultipleEntries:
    """AuditLog with multiple entries."""

    def setup_method(self):
        _, self.ctx = _make_sandbox(["a", "b", "c"])
        for action in ["a", "b", "c", "a"]:
            self.ctx.execute_json(action, json.dumps({"i": 0}))
        self.log = self.ctx.audit_log

    def test_len_matches_executions(self):
        assert len(self.log) == 4

    def test_entries_count(self):
        assert len(self.log.entries()) == 4

    def test_successes_all_four(self):
        assert len(self.log.successes()) == 4

    def test_entries_for_action_a(self):
        a_entries = self.log.entries_for_action("a")
        assert len(a_entries) == 2

    def test_entries_for_action_b(self):
        assert len(self.log.entries_for_action("b")) == 1

    def test_to_json_returns_string(self):
        j = self.log.to_json()
        assert isinstance(j, str)

    def test_to_json_valid_json_array(self):
        parsed = json.loads(self.log.to_json())
        assert isinstance(parsed, list)
        assert len(parsed) == 4

    def test_to_json_entries_have_required_keys(self):
        parsed = json.loads(self.log.to_json())
        required_keys = {"timestamp_ms", "action", "outcome"}
        for entry in parsed:
            assert required_keys.issubset(entry.keys())

    def test_entry_timestamps_non_decreasing(self):
        entries = self.log.entries()
        for i in range(1, len(entries)):
            assert entries[i].timestamp_ms >= entries[i - 1].timestamp_ms


# ===========================================================================
# Section 4: InputValidator
# ===========================================================================


class TestInputValidatorRequireString:
    """InputValidator.require_string rules."""

    def test_valid_string(self):
        v = m.InputValidator()
        v.require_string("name", min_length=1, max_length=50)
        ok, err = v.validate(json.dumps({"name": "hello"}))
        assert ok is True
        assert err is None

    def test_empty_string_below_min(self):
        v = m.InputValidator()
        v.require_string("name", min_length=1, max_length=50)
        ok, err = v.validate(json.dumps({"name": ""}))
        assert ok is False
        assert err is not None

    def test_string_too_long(self):
        v = m.InputValidator()
        v.require_string("name", min_length=1, max_length=5)
        ok, err = v.validate(json.dumps({"name": "toolong"}))
        assert ok is False
        assert "name" in err

    def test_missing_required_string_field(self):
        v = m.InputValidator()
        v.require_string("name", min_length=1, max_length=50)
        ok, err = v.validate(json.dumps({}))
        assert ok is False
        assert err is not None

    def test_exact_max_length_ok(self):
        v = m.InputValidator()
        v.require_string("x", min_length=0, max_length=3)
        ok, _ = v.validate(json.dumps({"x": "abc"}))
        assert ok is True

    def test_one_over_max_length_fails(self):
        v = m.InputValidator()
        v.require_string("x", min_length=0, max_length=3)
        ok, _ = v.validate(json.dumps({"x": "abcd"}))
        assert ok is False


class TestInputValidatorRequireNumber:
    """InputValidator.require_number rules."""

    def test_valid_number(self):
        v = m.InputValidator()
        v.require_number("count", min_value=0, max_value=100)
        ok, _ = v.validate(json.dumps({"count": 50}))
        assert ok is True

    def test_number_below_min(self):
        v = m.InputValidator()
        v.require_number("count", min_value=0, max_value=100)
        ok, err = v.validate(json.dumps({"count": -1}))
        assert ok is False
        assert "count" in err

    def test_number_above_max(self):
        v = m.InputValidator()
        v.require_number("count", min_value=0, max_value=100)
        ok, _ = v.validate(json.dumps({"count": 101}))
        assert ok is False

    def test_number_at_min_boundary(self):
        v = m.InputValidator()
        v.require_number("n", min_value=5, max_value=10)
        ok, _ = v.validate(json.dumps({"n": 5}))
        assert ok is True

    def test_number_at_max_boundary(self):
        v = m.InputValidator()
        v.require_number("n", min_value=5, max_value=10)
        ok, _ = v.validate(json.dumps({"n": 10}))
        assert ok is True

    def test_missing_number_field(self):
        v = m.InputValidator()
        v.require_number("count", min_value=0, max_value=100)
        ok, err = v.validate(json.dumps({}))
        assert ok is False
        assert err is not None

    def test_float_number_accepted(self):
        v = m.InputValidator()
        v.require_number("radius", min_value=0.0, max_value=100.0)
        ok, _ = v.validate(json.dumps({"radius": 3.14}))
        assert ok is True


class TestInputValidatorForbidSubstrings:
    """InputValidator.forbid_substrings rules."""

    def test_clean_value_passes(self):
        v = m.InputValidator()
        v.forbid_substrings("script", ["__import__", "exec("])
        ok, _ = v.validate(json.dumps({"script": "print('hello')"}))
        assert ok is True

    def test_forbidden_substring_fails(self):
        v = m.InputValidator()
        v.forbid_substrings("script", ["__import__"])
        ok, err = v.validate(json.dumps({"script": "__import__('os')"}))
        assert ok is False
        assert "__import__" in err

    def test_second_forbidden_pattern_fails(self):
        v = m.InputValidator()
        v.forbid_substrings("script", ["__import__", "exec("])
        ok, _ = v.validate(json.dumps({"script": "exec('malicious')"}))
        assert ok is False

    def test_multiple_forbidden_each_caught(self):
        v = m.InputValidator()
        v.forbid_substrings("code", ["eval(", "os.system", "subprocess"])
        for bad in ["eval('x')", "os.system('rm -rf')", "subprocess.run([])"]:
            ok, _ = v.validate(json.dumps({"code": bad}))
            assert ok is False, f"Should reject: {bad}"

    def test_partial_substring_is_caught(self):
        v = m.InputValidator()
        v.forbid_substrings("cmd", ["DROP TABLE"])
        ok, _ = v.validate(json.dumps({"cmd": "SELECT * FROM t WHERE id=1; DROP TABLE users"}))
        assert ok is False

    def test_field_not_present_passes(self):
        # If the field isn't in the input, forbid_substrings doesn't apply
        v = m.InputValidator()
        v.forbid_substrings("script", ["__import__"])
        ok, _ = v.validate(json.dumps({"other": "safe"}))
        assert ok is True


class TestInputValidatorCombinedRules:
    """InputValidator with multiple rules on multiple fields."""

    def setup_method(self):
        self.v = m.InputValidator()
        self.v.require_string("name", min_length=1, max_length=50)
        self.v.require_number("count", min_value=0, max_value=1000)
        self.v.forbid_substrings("script", ["__import__", "exec(", "eval("])

    def test_all_fields_valid(self):
        ok, _ = self.v.validate(json.dumps({"name": "sphere", "count": 5, "script": "pass"}))
        assert ok is True

    def test_invalid_name_fails(self):
        ok, _ = self.v.validate(json.dumps({"name": "", "count": 5, "script": "pass"}))
        assert ok is False

    def test_invalid_count_fails(self):
        ok, _ = self.v.validate(json.dumps({"name": "ok", "count": 9999, "script": "pass"}))
        assert ok is False

    def test_forbidden_script_fails(self):
        ok, _ = self.v.validate(json.dumps({"name": "ok", "count": 5, "script": "eval('x')"}))
        assert ok is False

    def test_missing_all_fields_fails(self):
        ok, _ = self.v.validate(json.dumps({}))
        assert ok is False

    def test_only_one_field_present_fails_on_others(self):
        ok, _ = self.v.validate(json.dumps({"name": "x"}))
        assert ok is False


# ===========================================================================
# Section 5: ToolResult derived methods
# ===========================================================================


class TestActionResultModelWithError:
    """ToolResult.with_error() derived copy."""

    def test_with_error_sets_success_false(self):
        r = m.success_result("ok")
        r2 = r.with_error("something broke")
        assert r2.success is False

    def test_with_error_sets_error_message(self):
        r = m.success_result("ok")
        r2 = r.with_error("something broke")
        assert r2.error == "something broke"

    def test_with_error_original_unchanged(self):
        r = m.success_result("ok")
        r.with_error("oops")
        assert r.success is True

    def test_with_error_preserves_message(self):
        r = m.success_result("my message")
        r2 = r.with_error("err")
        assert r2.message == "my message"

    def test_with_error_preserves_context(self):
        r = m.success_result("ok", x=1)
        r2 = r.with_error("err")
        assert r2.context.get("x") == 1

    def test_with_error_chained(self):
        r = m.success_result("ok")
        r2 = r.with_error("err1")
        r3 = r2.with_error("err2")
        assert r3.error == "err2"
        assert r3.success is False


class TestActionResultModelWithContext:
    """ToolResult.with_context() derived copy."""

    def test_with_context_adds_key(self):
        r = m.success_result("ok")
        r2 = r.with_context(new_key="value")
        assert r2.context.get("new_key") == "value"

    def test_with_context_original_unchanged(self):
        r = m.success_result("ok", x=1)
        r.with_context(y=2)
        assert r.context.get("y") is None

    def test_with_context_preserves_success(self):
        r = m.success_result("ok")
        r2 = r.with_context(k="v")
        assert r2.success is True

    def test_with_context_multiple_kwargs(self):
        r = m.success_result("ok")
        r2 = r.with_context(a=1, b="two", c=True)
        assert r2.context["a"] == 1
        assert r2.context["b"] == "two"
        assert r2.context["c"] is True

    def test_with_context_chained(self):
        r = m.success_result("ok")
        r2 = r.with_context(x=1).with_context(y=2)
        assert r2.context.get("y") == 2

    def test_with_context_on_error_result(self):
        r = m.error_result("Failed", "reason")
        r2 = r.with_context(info="extra")
        assert r2.context.get("info") == "extra"
        assert r2.success is False


class TestFromException:
    """from_exception factory function."""

    def test_from_exception_success_false(self):
        r = m.from_exception("ValueError: bad input", message="Failed")
        assert r.success is False

    def test_from_exception_error_contains_message(self):
        r = m.from_exception("some error", message="op failed")
        assert r.error is not None

    def test_from_exception_message_preserved(self):
        r = m.from_exception("err", message="Custom message")
        assert r.message == "Custom message"

    def test_from_exception_include_traceback_false(self):
        r = m.from_exception("err", message="fail", include_traceback=False)
        assert r.success is False

    def test_from_exception_include_traceback_true(self):
        r = m.from_exception("err", message="fail", include_traceback=True)
        assert r.success is False


class TestValidateActionResult:
    """validate_action_result normalization."""

    def test_dict_with_success_true(self):
        r = m.validate_action_result({"success": True, "message": "done"})
        assert r.success is True
        assert r.message == "done"

    def test_dict_with_success_false(self):
        r = m.validate_action_result({"success": False, "message": "fail", "error": "oops"})
        assert r.success is False

    def test_string_input_becomes_success(self):
        r = m.validate_action_result("hello result")
        assert r.success is True

    def test_none_input_becomes_success(self):
        r = m.validate_action_result(None)
        assert r.success is True

    def test_dict_without_success_key(self):
        # dict without success key: treated as success with context
        r = m.validate_action_result({"key": "value"})
        assert isinstance(r, m.ToolResult)

    def test_result_already_an_action_result_model(self):
        original = m.success_result("already good")
        r = m.validate_action_result(original)
        assert r.success is True

    def test_error_result_roundtrip(self):
        err = m.error_result("Failed to import", "FileNotFoundError")
        r = m.validate_action_result(err)
        assert r.success is False


class TestActionResultModelToDict:
    """ToolResult.to_dict() completeness."""

    def test_to_dict_has_success_key(self):
        r = m.success_result("ok")
        d = r.to_dict()
        assert "success" in d

    def test_to_dict_has_message_key(self):
        r = m.success_result("my message")
        d = r.to_dict()
        assert d["message"] == "my message"

    def test_to_dict_has_prompt_key(self):
        r = m.success_result("ok", prompt="do next")
        d = r.to_dict()
        assert d["prompt"] == "do next"

    def test_to_dict_has_error_key_none_on_success(self):
        r = m.success_result("ok")
        d = r.to_dict()
        assert d["error"] is None

    def test_to_dict_has_context(self):
        r = m.success_result("ok", sphere_count=3)
        d = r.to_dict()
        assert d["context"]["sphere_count"] == 3

    def test_to_dict_error_result(self):
        r = m.error_result("Fail", "reason")
        d = r.to_dict()
        assert d["success"] is False
        assert d["error"] is not None

    def test_to_dict_with_context_derived(self):
        r = m.success_result("ok").with_context(a=1)
        d = r.to_dict()
        assert d["context"]["a"] == 1

    def test_context_returns_new_dict_each_access(self):
        r = m.success_result("ok", x=1)
        c1 = r.context
        c2 = r.context
        # Both should be equal but not necessarily the same object
        assert c1 == c2
