"""Tests for wrapper types, wrap/unwrap functions, and skill dependency functions.

Covers:
- BooleanWrapper — value/bool/repr/hash/eq (identity-based)
- IntWrapper — value/int/index/repr/hash/eq (value-based)
- FloatWrapper — value/float/repr/eq (value-based)
- StringWrapper — value/str/repr/hash/eq (identity-based)
- wrap_value — dispatch to correct wrapper type (bool/int/float/str/passthrough)
- unwrap_value — extract from wrapper or passthrough (list/dict/None/plain)
- unwrap_parameters — batch dict unwrap
- resolve_dependencies — topological sort by depends
- validate_dependencies — report missing dependencies
- expand_transitive_dependencies — transitive dep expansion
"""

from __future__ import annotations

import pytest

from dcc_mcp_core import BooleanWrapper
from dcc_mcp_core import FloatWrapper
from dcc_mcp_core import IntWrapper
from dcc_mcp_core import SkillMetadata
from dcc_mcp_core import StringWrapper
from dcc_mcp_core import expand_transitive_dependencies
from dcc_mcp_core import resolve_dependencies
from dcc_mcp_core import unwrap_parameters
from dcc_mcp_core import unwrap_value
from dcc_mcp_core import validate_dependencies
from dcc_mcp_core import wrap_value

# ---------------------------------------------------------------------------
# BooleanWrapper
# ---------------------------------------------------------------------------


class TestBooleanWrapper:
    """Tests for BooleanWrapper."""

    def test_value_true(self) -> None:
        bw = BooleanWrapper(True)
        assert bw.value is True

    def test_value_false(self) -> None:
        bw = BooleanWrapper(False)
        assert bw.value is False

    def test_bool_true(self) -> None:
        bw = BooleanWrapper(True)
        assert bool(bw) is True

    def test_bool_false(self) -> None:
        bw = BooleanWrapper(False)
        assert bool(bw) is False

    def test_repr_true(self) -> None:
        r = repr(BooleanWrapper(True))
        assert "True" in r or "true" in r.lower()

    def test_repr_false(self) -> None:
        r = repr(BooleanWrapper(False))
        assert "False" in r or "false" in r.lower()

    def test_repr_is_string(self) -> None:
        assert isinstance(repr(BooleanWrapper(True)), str)

    def test_hash_is_int(self) -> None:
        bw = BooleanWrapper(True)
        assert isinstance(hash(bw), int)

    def test_eq_is_identity_based(self) -> None:
        # Two distinct BooleanWrapper(True) objects are NOT equal
        bw1 = BooleanWrapper(True)
        bw2 = BooleanWrapper(True)
        # identity-based: different objects → not equal
        assert (bw1 == bw2) is False

    def test_same_object_eq_self(self) -> None:
        # Even same-value different object is not equal (identity)
        bw = BooleanWrapper(True)
        bw_ref = bw
        # identity ref should work via 'is'
        assert bw is bw_ref


# ---------------------------------------------------------------------------
# IntWrapper
# ---------------------------------------------------------------------------


class TestIntWrapper:
    """Tests for IntWrapper."""

    def test_value(self) -> None:
        iw = IntWrapper(42)
        assert iw.value == 42

    def test_negative_value(self) -> None:
        iw = IntWrapper(-5)
        assert iw.value == -5

    def test_zero(self) -> None:
        iw = IntWrapper(0)
        assert iw.value == 0

    def test_int_conversion(self) -> None:
        iw = IntWrapper(99)
        assert int(iw) == 99

    def test_index_for_sequence(self) -> None:
        iw = IntWrapper(2)
        seq = [10, 20, 30]
        assert seq[iw] == 30  # uses __index__

    def test_repr_is_string(self) -> None:
        assert isinstance(repr(IntWrapper(1)), str)

    def test_repr_contains_value(self) -> None:
        assert "42" in repr(IntWrapper(42))

    def test_hash_is_int(self) -> None:
        assert isinstance(hash(IntWrapper(10)), int)

    def test_eq_value_based_same(self) -> None:
        # IntWrapper is value-based for equality
        assert IntWrapper(10) == IntWrapper(10)

    def test_eq_value_based_different(self) -> None:
        assert IntWrapper(10) != IntWrapper(20)

    def test_large_int(self) -> None:
        iw = IntWrapper(10**9)
        assert iw.value == 10**9


# ---------------------------------------------------------------------------
# FloatWrapper
# ---------------------------------------------------------------------------


class TestFloatWrapper:
    """Tests for FloatWrapper."""

    def test_value(self) -> None:
        fw = FloatWrapper(3.14)
        assert fw.value == pytest.approx(3.14)

    def test_zero(self) -> None:
        fw = FloatWrapper(0.0)
        assert fw.value == pytest.approx(0.0)

    def test_negative(self) -> None:
        fw = FloatWrapper(-1.5)
        assert fw.value == pytest.approx(-1.5)

    def test_float_conversion(self) -> None:
        fw = FloatWrapper(2.5)
        assert float(fw) == pytest.approx(2.5)

    def test_repr_is_string(self) -> None:
        assert isinstance(repr(FloatWrapper(1.0)), str)

    def test_repr_contains_value(self) -> None:
        r = repr(FloatWrapper(3.14))
        assert "3.14" in r

    def test_eq_value_based_same(self) -> None:
        assert FloatWrapper(1.0) == FloatWrapper(1.0)

    def test_eq_value_based_different(self) -> None:
        assert FloatWrapper(1.0) != FloatWrapper(2.0)


# ---------------------------------------------------------------------------
# StringWrapper
# ---------------------------------------------------------------------------


class TestStringWrapper:
    """Tests for StringWrapper."""

    def test_value(self) -> None:
        sw = StringWrapper("hello")
        assert sw.value == "hello"

    def test_empty_value(self) -> None:
        sw = StringWrapper("")
        assert sw.value == ""

    def test_str_conversion(self) -> None:
        sw = StringWrapper("world")
        assert str(sw) == "world"

    def test_repr_is_string(self) -> None:
        assert isinstance(repr(StringWrapper("x")), str)

    def test_repr_contains_value(self) -> None:
        r = repr(StringWrapper("maya"))
        assert "maya" in r

    def test_hash_is_int(self) -> None:
        assert isinstance(hash(StringWrapper("key")), int)

    def test_eq_is_identity_based(self) -> None:
        # StringWrapper eq is identity-based (not value-based)
        sw1 = StringWrapper("hello")
        sw2 = StringWrapper("hello")
        assert (sw1 == sw2) is False

    def test_unicode_value(self) -> None:
        sw = StringWrapper("场景_001")
        assert sw.value == "场景_001"


# ---------------------------------------------------------------------------
# wrap_value
# ---------------------------------------------------------------------------


class TestWrapValue:
    """Tests for wrap_value dispatch function."""

    def test_wrap_true(self) -> None:
        w = wrap_value(True)
        assert isinstance(w, BooleanWrapper)
        assert w.value is True

    def test_wrap_false(self) -> None:
        w = wrap_value(False)
        assert isinstance(w, BooleanWrapper)
        assert w.value is False

    def test_wrap_int(self) -> None:
        w = wrap_value(42)
        assert isinstance(w, IntWrapper)
        assert w.value == 42

    def test_wrap_zero_int(self) -> None:
        w = wrap_value(0)
        assert isinstance(w, IntWrapper)
        assert w.value == 0

    def test_wrap_negative_int(self) -> None:
        w = wrap_value(-10)
        assert isinstance(w, IntWrapper)
        assert w.value == -10

    def test_wrap_float(self) -> None:
        w = wrap_value(1.5)
        assert isinstance(w, FloatWrapper)
        assert float(w) == pytest.approx(1.5)

    def test_wrap_zero_float(self) -> None:
        w = wrap_value(0.0)
        assert isinstance(w, FloatWrapper)

    def test_wrap_string(self) -> None:
        w = wrap_value("hello")
        assert isinstance(w, StringWrapper)
        assert w.value == "hello"

    def test_wrap_empty_string(self) -> None:
        w = wrap_value("")
        assert isinstance(w, StringWrapper)

    def test_wrap_non_primitive_passthrough(self) -> None:
        # Non-primitive types are returned as-is
        lst = [1, 2, 3]
        result = wrap_value(lst)
        assert result is lst


# ---------------------------------------------------------------------------
# unwrap_value
# ---------------------------------------------------------------------------


class TestUnwrapValue:
    """Tests for unwrap_value extraction function."""

    def test_unwrap_boolean_true(self) -> None:
        assert unwrap_value(BooleanWrapper(True)) is True

    def test_unwrap_boolean_false(self) -> None:
        assert unwrap_value(BooleanWrapper(False)) is False

    def test_unwrap_int(self) -> None:
        assert unwrap_value(IntWrapper(99)) == 99

    def test_unwrap_zero_int(self) -> None:
        assert unwrap_value(IntWrapper(0)) == 0

    def test_unwrap_float(self) -> None:
        assert unwrap_value(FloatWrapper(2.5)) == pytest.approx(2.5)

    def test_unwrap_string(self) -> None:
        assert unwrap_value(StringWrapper("hello")) == "hello"

    def test_unwrap_empty_string(self) -> None:
        assert unwrap_value(StringWrapper("")) == ""

    def test_unwrap_plain_int_passthrough(self) -> None:
        assert unwrap_value(7) == 7

    def test_unwrap_plain_str_passthrough(self) -> None:
        assert unwrap_value("raw") == "raw"

    def test_unwrap_none_passthrough(self) -> None:
        assert unwrap_value(None) is None

    def test_unwrap_list_passthrough(self) -> None:
        lst = [1, 2, 3]
        assert unwrap_value(lst) == lst

    def test_unwrap_dict_passthrough(self) -> None:
        d = {"a": 1}
        result = unwrap_value(d)
        assert result == d


# ---------------------------------------------------------------------------
# unwrap_parameters
# ---------------------------------------------------------------------------


class TestUnwrapParameters:
    """Tests for unwrap_parameters batch unwrap."""

    def test_all_wrapper_types(self) -> None:
        params = {
            "flag": BooleanWrapper(True),
            "count": IntWrapper(5),
            "scale": FloatWrapper(1.5),
            "name": StringWrapper("sphere"),
        }
        result = unwrap_parameters(params)
        assert result["flag"] is True
        assert result["count"] == 5
        assert result["scale"] == pytest.approx(1.5)
        assert result["name"] == "sphere"

    def test_plain_values_preserved(self) -> None:
        params = {"x": 10, "label": "hello", "active": False}
        result = unwrap_parameters(params)
        assert result["x"] == 10
        assert result["label"] == "hello"
        assert result["active"] is False

    def test_empty_dict(self) -> None:
        assert unwrap_parameters({}) == {}

    def test_mixed_wrapped_and_plain(self) -> None:
        params = {
            "wrapped_int": IntWrapper(3),
            "plain_str": "abc",
        }
        result = unwrap_parameters(params)
        assert result["wrapped_int"] == 3
        assert result["plain_str"] == "abc"

    def test_keys_preserved(self) -> None:
        params = {"key_a": IntWrapper(1), "key_b": StringWrapper("v")}
        result = unwrap_parameters(params)
        assert set(result.keys()) == {"key_a", "key_b"}


# ---------------------------------------------------------------------------
# resolve_dependencies
# ---------------------------------------------------------------------------


class TestResolveDependencies:
    """Tests for resolve_dependencies topological sort."""

    def test_no_deps_any_order(self) -> None:
        s1 = SkillMetadata("alpha")
        s2 = SkillMetadata("beta")
        result = resolve_dependencies([s1, s2])
        names = [s.name for s in result]
        assert "alpha" in names
        assert "beta" in names

    def test_dep_comes_before_dependant(self) -> None:
        base = SkillMetadata("base")
        child = SkillMetadata("child", depends=["base"])
        result = resolve_dependencies([child, base])
        names = [s.name for s in result]
        assert names.index("base") < names.index("child")

    def test_chain_ordering(self) -> None:
        a = SkillMetadata("a")
        b = SkillMetadata("b", depends=["a"])
        c = SkillMetadata("c", depends=["b"])
        result = resolve_dependencies([c, b, a])
        names = [s.name for s in result]
        assert names.index("a") < names.index("b")
        assert names.index("b") < names.index("c")

    def test_empty_list(self) -> None:
        result = resolve_dependencies([])
        assert result == []

    def test_single_skill(self) -> None:
        s = SkillMetadata("solo")
        result = resolve_dependencies([s])
        assert len(result) == 1
        assert result[0].name == "solo"

    def test_missing_dep_raises(self) -> None:
        s = SkillMetadata("orphan", depends=["missing"])
        with pytest.raises(ValueError):
            resolve_dependencies([s])

    def test_cycle_raises(self) -> None:
        a = SkillMetadata("a", depends=["b"])
        b = SkillMetadata("b", depends=["a"])
        with pytest.raises(ValueError):
            resolve_dependencies([a, b])

    def test_result_is_list(self) -> None:
        s = SkillMetadata("only")
        result = resolve_dependencies([s])
        assert isinstance(result, list)


# ---------------------------------------------------------------------------
# validate_dependencies
# ---------------------------------------------------------------------------


class TestValidateDependencies:
    """Tests for validate_dependencies missing dependency reporting."""

    def test_no_deps_no_errors(self) -> None:
        s = SkillMetadata("standalone")
        errors = validate_dependencies([s])
        assert errors == []

    def test_satisfied_dep_no_errors(self) -> None:
        base = SkillMetadata("base")
        child = SkillMetadata("child", depends=["base"])
        errors = validate_dependencies([base, child])
        assert errors == []

    def test_missing_dep_produces_error(self) -> None:
        s = SkillMetadata("orphan", depends=["nonexistent"])
        errors = validate_dependencies([s])
        assert len(errors) == 1
        assert "nonexistent" in errors[0]

    def test_multiple_missing_deps(self) -> None:
        s1 = SkillMetadata("s1", depends=["missing1"])
        s2 = SkillMetadata("s2", depends=["missing2"])
        errors = validate_dependencies([s1, s2])
        assert len(errors) == 2

    def test_error_message_contains_skill_name(self) -> None:
        s = SkillMetadata("child", depends=["absent"])
        errors = validate_dependencies([s])
        assert "child" in errors[0] or "absent" in errors[0]

    def test_empty_list_no_errors(self) -> None:
        assert validate_dependencies([]) == []

    def test_returns_list(self) -> None:
        result = validate_dependencies([SkillMetadata("s")])
        assert isinstance(result, list)


# ---------------------------------------------------------------------------
# expand_transitive_dependencies
# ---------------------------------------------------------------------------


class TestExpandTransitiveDependencies:
    """Tests for expand_transitive_dependencies full dep tree."""

    def test_direct_dep(self) -> None:
        base = SkillMetadata("base")
        child = SkillMetadata("child", depends=["base"])
        result = expand_transitive_dependencies([base, child], "child")
        assert "base" in result

    def test_transitive_dep_included(self) -> None:
        a = SkillMetadata("a")
        b = SkillMetadata("b", depends=["a"])
        c = SkillMetadata("c", depends=["b"])
        result = expand_transitive_dependencies([a, b, c], "c")
        assert "a" in result
        assert "b" in result

    def test_no_deps_empty_result(self) -> None:
        s = SkillMetadata("standalone")
        result = expand_transitive_dependencies([s], "standalone")
        assert result == []

    def test_unknown_skill_returns_empty(self) -> None:
        # expand_transitive_dependencies returns [] for unknown skill names
        s = SkillMetadata("existing")
        result = expand_transitive_dependencies([s], "nonexistent")
        assert result == []

    def test_result_is_list(self) -> None:
        base = SkillMetadata("base")
        child = SkillMetadata("child", depends=["base"])
        result = expand_transitive_dependencies([base, child], "child")
        assert isinstance(result, list)

    def test_skill_not_included_in_its_own_deps(self) -> None:
        a = SkillMetadata("a")
        b = SkillMetadata("b", depends=["a"])
        result = expand_transitive_dependencies([a, b], "b")
        assert "b" not in result

    def test_deep_chain_all_included(self) -> None:
        a = SkillMetadata("level1")
        b = SkillMetadata("level2", depends=["level1"])
        c = SkillMetadata("level3", depends=["level2"])
        d = SkillMetadata("level4", depends=["level3"])
        result = expand_transitive_dependencies([a, b, c, d], "level4")
        assert "level1" in result
        assert "level2" in result
        assert "level3" in result
        assert len(result) == 3

    def test_missing_dep_raises(self) -> None:
        s = SkillMetadata("orphan", depends=["ghost"])
        with pytest.raises(ValueError):
            expand_transitive_dependencies([s], "orphan")
