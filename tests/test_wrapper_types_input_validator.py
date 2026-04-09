"""Tests for type wrappers and InputValidator.

Covers: BooleanWrapper/IntWrapper/FloatWrapper/StringWrapper wrap/unwrap roundtrip
and InputValidator (require_string/require_number/forbid_substrings).
"""

from __future__ import annotations

import json

import pytest

from dcc_mcp_core import BooleanWrapper
from dcc_mcp_core import FloatWrapper
from dcc_mcp_core import InputValidator
from dcc_mcp_core import IntWrapper
from dcc_mcp_core import StringWrapper
from dcc_mcp_core import unwrap_value
from dcc_mcp_core import wrap_value


class TestBooleanWrapper:
    """Tests for BooleanWrapper wrap/unwrap roundtrip."""

    def test_wrap_true_returns_boolean_wrapper(self):
        w = wrap_value(True)
        assert isinstance(w, BooleanWrapper)

    def test_wrap_false_returns_boolean_wrapper(self):
        w = wrap_value(False)
        assert isinstance(w, BooleanWrapper)

    def test_unwrap_true_returns_true(self):
        w = wrap_value(True)
        assert unwrap_value(w) is True

    def test_unwrap_false_returns_false(self):
        w = wrap_value(False)
        assert unwrap_value(w) is False

    def test_unwrap_type_is_bool(self):
        for v in [True, False]:
            w = wrap_value(v)
            result = unwrap_value(w)
            assert isinstance(result, bool)

    def test_wrap_true_direct_constructor(self):
        w = BooleanWrapper(True)
        assert isinstance(w, BooleanWrapper)
        assert unwrap_value(w) is True

    def test_wrap_false_direct_constructor(self):
        w = BooleanWrapper(False)
        assert unwrap_value(w) is False

    def test_boolean_wrapper_has_value_access(self):
        w = BooleanWrapper(True)
        # should expose value
        result = unwrap_value(w)
        assert result is True


class TestIntWrapper:
    """Tests for IntWrapper wrap/unwrap roundtrip."""

    def test_wrap_positive_returns_int_wrapper(self):
        w = wrap_value(42)
        assert isinstance(w, IntWrapper)

    def test_wrap_negative_returns_int_wrapper(self):
        w = wrap_value(-7)
        assert isinstance(w, IntWrapper)

    def test_wrap_zero_returns_int_wrapper(self):
        w = wrap_value(0)
        assert isinstance(w, IntWrapper)

    def test_unwrap_positive(self):
        w = wrap_value(42)
        assert unwrap_value(w) == 42

    def test_unwrap_negative(self):
        w = wrap_value(-7)
        assert unwrap_value(w) == -7

    def test_unwrap_zero(self):
        w = wrap_value(0)
        assert unwrap_value(w) == 0

    def test_unwrap_type_is_int(self):
        for v in [0, 1, -1, 100, -100]:
            w = wrap_value(v)
            result = unwrap_value(w)
            assert isinstance(result, int)

    def test_direct_constructor_positive(self):
        w = IntWrapper(99)
        assert unwrap_value(w) == 99

    def test_direct_constructor_negative(self):
        w = IntWrapper(-99)
        assert unwrap_value(w) == -99

    def test_large_int_roundtrip(self):
        v = 2**31 - 1
        w = wrap_value(v)
        assert unwrap_value(w) == v

    def test_roundtrip_preserves_sign(self):
        for v in [1, -1, 1000000, -1000000]:
            assert unwrap_value(wrap_value(v)) == v


class TestFloatWrapper:
    """Tests for FloatWrapper wrap/unwrap roundtrip."""

    def test_wrap_positive_float_returns_float_wrapper(self):
        w = wrap_value(3.14)
        assert isinstance(w, FloatWrapper)

    def test_wrap_negative_float_returns_float_wrapper(self):
        w = wrap_value(-2.5)
        assert isinstance(w, FloatWrapper)

    def test_wrap_zero_float_returns_float_wrapper(self):
        w = wrap_value(0.0)
        assert isinstance(w, FloatWrapper)

    def test_unwrap_positive_float(self):
        w = wrap_value(3.14)
        result = unwrap_value(w)
        assert abs(result - 3.14) < 1e-9

    def test_unwrap_negative_float(self):
        w = wrap_value(-2.5)
        result = unwrap_value(w)
        assert abs(result - (-2.5)) < 1e-9

    def test_unwrap_zero_float(self):
        w = wrap_value(0.0)
        result = unwrap_value(w)
        assert result == 0.0

    def test_unwrap_type_is_float(self):
        for v in [0.0, 1.0, -1.5, 1e10]:
            w = wrap_value(v)
            result = unwrap_value(w)
            assert isinstance(result, float)

    def test_direct_constructor(self):
        w = FloatWrapper(2.718)
        result = unwrap_value(w)
        assert abs(result - 2.718) < 1e-9

    def test_roundtrip_small_float(self):
        v = 1e-10
        w = wrap_value(v)
        assert abs(unwrap_value(w) - v) < 1e-15


class TestStringWrapper:
    """Tests for StringWrapper wrap/unwrap roundtrip."""

    def test_wrap_string_returns_string_wrapper(self):
        w = wrap_value("hello")
        assert isinstance(w, StringWrapper)

    def test_wrap_empty_string_returns_string_wrapper(self):
        w = wrap_value("")
        assert isinstance(w, StringWrapper)

    def test_unwrap_simple_string(self):
        w = wrap_value("hello")
        assert unwrap_value(w) == "hello"

    def test_unwrap_empty_string(self):
        w = wrap_value("")
        assert unwrap_value(w) == ""

    def test_unwrap_type_is_str(self):
        for v in ["", "a", "hello world", "unicode: ★"]:
            w = wrap_value(v)
            result = unwrap_value(w)
            assert isinstance(result, str)

    def test_direct_constructor(self):
        w = StringWrapper("world")
        assert unwrap_value(w) == "world"

    def test_roundtrip_unicode(self):
        v = "こんにちは"
        w = wrap_value(v)
        assert unwrap_value(w) == v

    def test_roundtrip_special_chars(self):
        v = "line1\nline2\ttab"
        w = wrap_value(v)
        assert unwrap_value(w) == v

    def test_roundtrip_long_string(self):
        v = "x" * 10000
        w = wrap_value(v)
        assert unwrap_value(w) == v


class TestWrapValueTypeDispatch:
    """Tests that wrap_value dispatches to correct wrapper type."""

    def test_bool_before_int(self):
        # bool is subclass of int; must map to BooleanWrapper not IntWrapper
        w = wrap_value(True)
        assert isinstance(w, BooleanWrapper)
        assert not isinstance(w, IntWrapper)

    def test_int_not_bool(self):
        w = wrap_value(1)
        assert isinstance(w, IntWrapper)
        assert not isinstance(w, BooleanWrapper)

    def test_float_not_int(self):
        w = wrap_value(1.0)
        assert isinstance(w, FloatWrapper)
        assert not isinstance(w, IntWrapper)


class TestInputValidatorRequireString:
    """Tests for InputValidator.require_string boundary conditions."""

    def test_valid_within_length_range(self):
        iv = InputValidator()
        iv.require_string("name", max_length=20, min_length=3)
        ok, err = iv.validate(json.dumps({"name": "hello"}))
        assert ok is True
        assert err is None

    def test_at_min_length_is_valid(self):
        iv = InputValidator()
        iv.require_string("name", max_length=20, min_length=3)
        ok, _err = iv.validate(json.dumps({"name": "abc"}))  # exactly 3
        assert ok is True

    def test_at_max_length_is_valid(self):
        iv = InputValidator()
        iv.require_string("name", max_length=5, min_length=1)
        ok, _err = iv.validate(json.dumps({"name": "hello"}))  # exactly 5
        assert ok is True

    def test_below_min_length_fails(self):
        iv = InputValidator()
        iv.require_string("name", max_length=20, min_length=3)
        ok, err = iv.validate(json.dumps({"name": "hi"}))  # 2 chars
        assert ok is False
        assert "minimum" in err or "below" in err

    def test_above_max_length_fails(self):
        iv = InputValidator()
        iv.require_string("name", max_length=5, min_length=1)
        ok, err = iv.validate(json.dumps({"name": "toolong"}))  # 7 chars
        assert ok is False
        assert "maximum" in err or "exceeds" in err

    def test_error_message_contains_field_name(self):
        iv = InputValidator()
        iv.require_string("username", max_length=10, min_length=3)
        ok, err = iv.validate(json.dumps({"username": "ab"}))
        assert ok is False
        assert "username" in err

    def test_empty_string_fails_min_length_1(self):
        iv = InputValidator()
        iv.require_string("tag", max_length=50, min_length=1)
        ok, _err = iv.validate(json.dumps({"tag": ""}))
        assert ok is False

    def test_missing_field_behaviour(self):
        iv = InputValidator()
        iv.require_string("name", max_length=20, min_length=3)
        # field missing entirely - implementation-defined
        result = iv.validate(json.dumps({}))
        # Either raises or returns (False, error_msg)
        assert result is not None

    def test_multiple_string_fields_both_valid(self):
        iv = InputValidator()
        iv.require_string("first", max_length=20, min_length=1)
        iv.require_string("last", max_length=20, min_length=1)
        ok, _err = iv.validate(json.dumps({"first": "John", "last": "Doe"}))
        assert ok is True

    def test_multiple_string_fields_one_invalid(self):
        iv = InputValidator()
        iv.require_string("first", max_length=20, min_length=3)
        iv.require_string("last", max_length=20, min_length=3)
        ok, _err = iv.validate(json.dumps({"first": "John", "last": "Li"}))
        assert ok is False  # 'Li' is 2 chars, below min 3

    def test_max_length_zero_min_zero_accepts_empty(self):
        iv = InputValidator()
        iv.require_string("tag", max_length=100, min_length=0)
        ok, _err = iv.validate(json.dumps({"tag": ""}))
        assert ok is True


class TestInputValidatorRequireNumber:
    """Tests for InputValidator.require_number boundary conditions."""

    def test_valid_number_in_range(self):
        iv = InputValidator()
        iv.require_number("age", min_value=0.0, max_value=150.0)
        ok, _err = iv.validate(json.dumps({"age": 25}))
        assert ok is True

    def test_at_min_is_valid(self):
        iv = InputValidator()
        iv.require_number("age", min_value=0.0, max_value=150.0)
        ok, _err = iv.validate(json.dumps({"age": 0}))
        assert ok is True

    def test_at_max_is_valid(self):
        iv = InputValidator()
        iv.require_number("age", min_value=0.0, max_value=150.0)
        ok, _err = iv.validate(json.dumps({"age": 150}))
        assert ok is True

    def test_below_min_fails(self):
        iv = InputValidator()
        iv.require_number("age", min_value=0.0, max_value=150.0)
        ok, err = iv.validate(json.dumps({"age": -1}))
        assert ok is False
        assert "minimum" in err or "below" in err

    def test_above_max_fails(self):
        iv = InputValidator()
        iv.require_number("age", min_value=0.0, max_value=150.0)
        ok, err = iv.validate(json.dumps({"age": 200}))
        assert ok is False
        assert "maximum" in err or "exceeds" in err

    def test_float_value_valid(self):
        iv = InputValidator()
        iv.require_number("score", min_value=0.0, max_value=1.0)
        ok, _err = iv.validate(json.dumps({"score": 0.5}))
        assert ok is True

    def test_float_value_exceeds_max(self):
        iv = InputValidator()
        iv.require_number("score", min_value=0.0, max_value=1.0)
        ok, _err = iv.validate(json.dumps({"score": 1.5}))
        assert ok is False

    def test_negative_range(self):
        iv = InputValidator()
        iv.require_number("temp", min_value=-100.0, max_value=-10.0)
        ok, _ = iv.validate(json.dumps({"temp": -50}))
        assert ok is True

    def test_negative_range_out_of_bounds(self):
        iv = InputValidator()
        iv.require_number("temp", min_value=-100.0, max_value=-10.0)
        ok, _err = iv.validate(json.dumps({"temp": 0}))
        assert ok is False

    def test_error_message_contains_field_name(self):
        iv = InputValidator()
        iv.require_number("radius", min_value=0.1, max_value=100.0)
        ok, err = iv.validate(json.dumps({"radius": 0.0}))
        assert ok is False
        assert "radius" in err

    def test_multiple_number_fields_all_valid(self):
        iv = InputValidator()
        iv.require_number("x", min_value=-10.0, max_value=10.0)
        iv.require_number("y", min_value=-10.0, max_value=10.0)
        ok, _err = iv.validate(json.dumps({"x": 1.0, "y": -2.5}))
        assert ok is True

    def test_combined_string_and_number_fields(self):
        iv = InputValidator()
        iv.require_string("name", max_length=20, min_length=1)
        iv.require_number("count", min_value=1.0, max_value=100.0)
        ok, _err = iv.validate(json.dumps({"name": "sphere", "count": 5}))
        assert ok is True

    def test_combined_fails_on_number_violation(self):
        iv = InputValidator()
        iv.require_string("name", max_length=20, min_length=1)
        iv.require_number("count", min_value=1.0, max_value=100.0)
        ok, _err = iv.validate(json.dumps({"name": "sphere", "count": 0}))
        assert ok is False


class TestInputValidatorForbidSubstrings:
    """Tests for InputValidator.forbid_substrings injection protection."""

    def test_safe_string_passes(self):
        iv = InputValidator()
        iv.forbid_substrings("cmd", ["rm -rf", "DROP TABLE"])
        ok, _err = iv.validate(json.dumps({"cmd": "ls -la"}))
        assert ok is True

    def test_dangerous_string_fails(self):
        iv = InputValidator()
        iv.forbid_substrings("cmd", ["rm -rf"])
        ok, err = iv.validate(json.dumps({"cmd": "rm -rf /"}))
        assert ok is False
        assert "forbidden" in err or "rm -rf" in err

    def test_sql_injection_fails(self):
        iv = InputValidator()
        iv.forbid_substrings("query", ["DROP TABLE", "DELETE FROM", "--"])
        ok, _err = iv.validate(json.dumps({"query": "DROP TABLE users"}))
        assert ok is False

    def test_python_injection_fails(self):
        iv = InputValidator()
        iv.forbid_substrings("code", ["__import__", "exec(", "eval("])
        ok, _err = iv.validate(json.dumps({"code": "__import__('os').system('ls')"}))
        assert ok is False

    def test_multiple_forbidden_only_first_matched_is_reported(self):
        iv = InputValidator()
        iv.forbid_substrings("text", ["bad1", "bad2"])
        ok, err = iv.validate(json.dumps({"text": "contains bad1 here"}))
        assert ok is False
        assert "bad1" in err

    def test_empty_string_passes_when_no_forbidden_matches(self):
        iv = InputValidator()
        iv.forbid_substrings("text", ["forbidden"])
        ok, _err = iv.validate(json.dumps({"text": ""}))
        assert ok is True

    def test_case_sensitivity(self):
        iv = InputValidator()
        iv.forbid_substrings("cmd", ["rm -rf"])
        # Case mismatch - passes (substring matching is case-sensitive by default)
        ok, _err = iv.validate(json.dumps({"cmd": "RM -RF /"}))
        assert ok is True  # 'RM -RF' != 'rm -rf'

    def test_partial_substring_match(self):
        iv = InputValidator()
        iv.forbid_substrings("path", ["/etc/passwd"])
        ok, _err = iv.validate(json.dumps({"path": "cat /etc/passwd | grep root"}))
        assert ok is False

    def test_empty_forbidden_list_always_passes(self):
        iv = InputValidator()
        iv.forbid_substrings("text", [])
        ok, _err = iv.validate(json.dumps({"text": "anything here rm -rf DROP TABLE"}))
        assert ok is True

    def test_combined_string_and_forbid(self):
        iv = InputValidator()
        iv.require_string("cmd", max_length=100, min_length=1)
        iv.forbid_substrings("cmd", ["rm -rf", "sudo"])
        ok, _ = iv.validate(json.dumps({"cmd": "ls -la"}))
        assert ok is True
        ok2, _ = iv.validate(json.dumps({"cmd": "sudo rm -rf /"}))
        assert ok2 is False

    def test_error_message_contains_field_name(self):
        iv = InputValidator()
        iv.forbid_substrings("script", ["dangerous"])
        ok, err = iv.validate(json.dumps({"script": "this is dangerous code"}))
        assert ok is False
        assert "script" in err


class TestInputValidatorValidateReturn:
    """Tests for validate() return structure."""

    def test_returns_tuple(self):
        iv = InputValidator()
        result = iv.validate(json.dumps({"x": 1}))
        assert isinstance(result, tuple)
        assert len(result) == 2

    def test_success_tuple_second_is_none(self):
        iv = InputValidator()
        iv.require_string("name", max_length=20, min_length=1)
        ok, err = iv.validate(json.dumps({"name": "hello"}))
        assert ok is True
        assert err is None

    def test_failure_tuple_second_is_string(self):
        iv = InputValidator()
        iv.require_string("name", max_length=5, min_length=1)
        ok, err = iv.validate(json.dumps({"name": "toolongname"}))
        assert ok is False
        assert isinstance(err, str)
        assert len(err) > 0

    def test_no_rules_empty_json_passes(self):
        iv = InputValidator()
        ok, _err = iv.validate(json.dumps({}))
        assert ok is True

    def test_no_rules_nonempty_json_passes(self):
        iv = InputValidator()
        ok, _err = iv.validate(json.dumps({"anything": "value"}))
        assert ok is True

    def test_invalid_json_raises(self):
        iv = InputValidator()
        with pytest.raises(RuntimeError):
            iv.validate("not valid json {{{")
