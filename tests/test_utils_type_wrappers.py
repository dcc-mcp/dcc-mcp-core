#!/usr/bin/env python
"""Tests for the type_wrappers module."""

# Import built-in modules
import pickle

# Import third-party modules
import pytest

# Import local modules
# Import internal modules
from dcc_mcp_core.utils.type_wrappers import BaseWrapper
from dcc_mcp_core.utils.type_wrappers import BooleanWrapper
from dcc_mcp_core.utils.type_wrappers import FloatWrapper
from dcc_mcp_core.utils.type_wrappers import IntWrapper
from dcc_mcp_core.utils.type_wrappers import StringWrapper
from dcc_mcp_core.utils.type_wrappers import unwrap_parameters
from dcc_mcp_core.utils.type_wrappers import unwrap_value
from dcc_mcp_core.utils.type_wrappers import wrap_boolean_parameters
from dcc_mcp_core.utils.type_wrappers import wrap_value


class TestBaseWrapper:
    """Tests for the BaseWrapper class."""

    def test_init(self):
        """Test initialization."""
        wrapper = BaseWrapper(42)
        assert wrapper.value == 42

    def test_reduce(self):
        """Test serialization support."""
        wrapper = BaseWrapper(42)
        serialized = pickle.dumps(wrapper)
        deserialized = pickle.loads(serialized)
        assert isinstance(deserialized, BaseWrapper)
        assert deserialized.value == 42

    def test_repr(self):
        """Test string representation."""
        wrapper = BaseWrapper(42)
        assert repr(wrapper) == "BaseWrapper(42)"

    def test_str(self):
        """Test string conversion."""
        wrapper = BaseWrapper(42)
        assert str(wrapper) == "42"

    def test_eq(self):
        """Test equality comparison."""
        wrapper1 = BaseWrapper(42)
        wrapper2 = BaseWrapper(42)
        wrapper3 = BaseWrapper(43)
        assert wrapper1 == wrapper2
        assert wrapper1 != wrapper3
        assert wrapper1 == 42
        assert wrapper1 != 43


class TestBooleanWrapper:
    """Tests for the BooleanWrapper class."""

    def test_init_with_bool(self):
        """Test initialization with boolean values."""
        wrapper = BooleanWrapper(True)
        assert wrapper.value is True
        wrapper = BooleanWrapper(False)
        assert wrapper.value is False

    def test_init_with_int(self):
        """Test initialization with integer values."""
        wrapper = BooleanWrapper(1)
        assert wrapper.value is True
        wrapper = BooleanWrapper(0)
        assert wrapper.value is False

    def test_init_with_float(self):
        """Test initialization with float values."""
        wrapper = BooleanWrapper(1.0)
        assert wrapper.value is True
        wrapper = BooleanWrapper(0.0)
        assert wrapper.value is False

    def test_init_with_string(self):
        """Test initialization with string values."""
        # True strings
        for true_str in ["true", "True", "TRUE", "1", "yes", "Yes", "YES", "on", "On", "ON"]:
            wrapper = BooleanWrapper(true_str)
            assert wrapper.value is True, f"Failed for string: {true_str}"

        # False strings
        for false_str in ["false", "False", "FALSE", "0", "no", "No", "NO", "off", "Off", "OFF"]:
            wrapper = BooleanWrapper(false_str)
            assert wrapper.value is False, f"Failed for string: {false_str}"

        # Other strings should be False
        wrapper = BooleanWrapper("invalid")
        assert wrapper.value is False

    def test_init_with_other_types(self):
        """Test initialization with other types."""
        # Empty collections should be False
        wrapper = BooleanWrapper([])
        assert wrapper.value is False
        wrapper = BooleanWrapper({})
        assert wrapper.value is False

        # Non-empty collections should be True
        wrapper = BooleanWrapper([1, 2, 3])
        assert wrapper.value is True
        wrapper = BooleanWrapper({"key": "value"})
        assert wrapper.value is True

    def test_bool_conversion(self):
        """Test boolean conversion."""
        wrapper = BooleanWrapper(True)
        assert bool(wrapper) is True
        wrapper = BooleanWrapper(False)
        assert bool(wrapper) is False


class TestIntWrapper:
    """Tests for the IntWrapper class."""

    def test_init_with_int(self):
        """Test initialization with integer values."""
        wrapper = IntWrapper(42)
        assert wrapper.value == 42
        wrapper = IntWrapper(-42)
        assert wrapper.value == -42

    def test_init_with_bool(self):
        """Test initialization with boolean values."""
        wrapper = IntWrapper(True)
        assert wrapper.value == 1
        wrapper = IntWrapper(False)
        assert wrapper.value == 0

    def test_init_with_float(self):
        """Test initialization with float values."""
        wrapper = IntWrapper(42.5)
        assert wrapper.value == 42
        wrapper = IntWrapper(-42.5)
        assert wrapper.value == -42

    def test_init_with_string(self):
        """Test initialization with string values."""
        wrapper = IntWrapper("42")
        assert wrapper.value == 42
        wrapper = IntWrapper("-42")
        assert wrapper.value == -42

        # Invalid strings should be 0
        wrapper = IntWrapper("invalid")
        assert wrapper.value == 0

    def test_init_with_other_types(self):
        """Test initialization with other types."""
        # None should be 0
        wrapper = IntWrapper(None)
        assert wrapper.value == 0

    def test_int_conversion(self):
        """Test integer conversion."""
        wrapper = IntWrapper(42)
        assert int(wrapper) == 42

    def test_index(self):
        """Test index conversion."""
        wrapper = IntWrapper(42)
        assert wrapper.__index__() == 42
        # Test in a context that uses __index__
        lst = [0, 1, 2, 3, 4, 5]
        assert lst[wrapper] == 42 if len(lst) > 42 else pytest.skip("List too small for test")


class TestFloatWrapper:
    """Tests for the FloatWrapper class."""

    def test_init_with_float(self):
        """Test initialization with float values."""
        wrapper = FloatWrapper(42.5)
        assert wrapper.value == 42.5
        wrapper = FloatWrapper(-42.5)
        assert wrapper.value == -42.5

    def test_init_with_int(self):
        """Test initialization with integer values."""
        wrapper = FloatWrapper(42)
        assert wrapper.value == 42.0
        assert isinstance(wrapper.value, float)
        wrapper = FloatWrapper(-42)
        assert wrapper.value == -42.0
        assert isinstance(wrapper.value, float)

    def test_init_with_bool(self):
        """Test initialization with boolean values."""
        wrapper = FloatWrapper(True)
        assert wrapper.value == 1.0
        assert isinstance(wrapper.value, float)
        wrapper = FloatWrapper(False)
        assert wrapper.value == 0.0
        assert isinstance(wrapper.value, float)

    def test_init_with_string(self):
        """Test initialization with string values."""
        wrapper = FloatWrapper("42.5")
        assert wrapper.value == 42.5
        wrapper = FloatWrapper("-42.5")
        assert wrapper.value == -42.5

        # Invalid strings should be 0.0
        wrapper = FloatWrapper("invalid")
        assert wrapper.value == 0.0

    def test_init_with_other_types(self):
        """Test initialization with other types."""
        # None should be 0.0
        wrapper = FloatWrapper(None)
        assert wrapper.value == 0.0

    def test_float_conversion(self):
        """Test float conversion."""
        wrapper = FloatWrapper(42.5)
        assert float(wrapper) == 42.5


class TestStringWrapper:
    """Tests for the StringWrapper class."""

    def test_init_with_string(self):
        """Test initialization with string values."""
        wrapper = StringWrapper("hello")
        assert wrapper.value == "hello"

    def test_init_with_int(self):
        """Test initialization with integer values."""
        wrapper = StringWrapper(42)
        assert wrapper.value == "42"

    def test_init_with_float(self):
        """Test initialization with float values."""
        wrapper = StringWrapper(42.5)
        assert wrapper.value == "42.5"

    def test_init_with_bool(self):
        """Test initialization with boolean values."""
        wrapper = StringWrapper(True)
        assert wrapper.value == "True"
        wrapper = StringWrapper(False)
        assert wrapper.value == "False"

    def test_init_with_none(self):
        """Test initialization with None."""
        wrapper = StringWrapper(None)
        assert wrapper.value == "None"

    def test_init_with_complex_types(self):
        """Test initialization with complex types."""
        wrapper = StringWrapper([1, 2, 3])
        assert wrapper.value == "[1, 2, 3]"
        wrapper = StringWrapper({"key": "value"})
        # Dictionary string representation may vary by Python version, only check key and value
        assert "key" in wrapper.value
        assert "value" in wrapper.value

    def test_str_conversion(self):
        """Test string conversion."""
        wrapper = StringWrapper("hello")
        assert str(wrapper) == "hello"


class TestWrapValue:
    """Tests for the wrap_value function."""

    def test_wrap_bool(self):
        """Test wrapping boolean values."""
        wrapped = wrap_value(True)
        assert isinstance(wrapped, BooleanWrapper)
        assert wrapped.value is True

    def test_wrap_int(self):
        """Test wrapping integer values."""
        wrapped = wrap_value(42)
        assert isinstance(wrapped, IntWrapper)
        assert wrapped.value == 42

    def test_wrap_float(self):
        """Test wrapping float values."""
        wrapped = wrap_value(42.5)
        assert isinstance(wrapped, FloatWrapper)
        assert wrapped.value == 42.5

    def test_wrap_string(self):
        """Test wrapping string values."""
        wrapped = wrap_value("hello")
        assert isinstance(wrapped, StringWrapper)
        assert wrapped.value == "hello"

    def test_wrap_none(self):
        """Test wrapping None."""
        wrapped = wrap_value(None)
        assert wrapped is None

    def test_wrap_already_wrapped(self):
        """Test wrapping already wrapped values."""
        original = BooleanWrapper(True)
        wrapped = wrap_value(original)
        assert wrapped is original


class TestWrapBooleanParameters:
    """Tests for the wrap_boolean_parameters function."""

    def test_wrap_boolean_params(self):
        """Test wrapping boolean parameters."""
        params = {"bool_param": True, "int_param": 42, "float_param": 42.5, "string_param": "hello"}
        wrapped = wrap_boolean_parameters(params)
        assert isinstance(wrapped["bool_param"], BooleanWrapper)
        assert wrapped["bool_param"].value is True
        assert wrapped["int_param"] == 42
        assert wrapped["float_param"] == 42.5
        assert wrapped["string_param"] == "hello"

    def test_wrap_nested_boolean_params(self):
        """Test wrapping nested boolean parameters."""
        params = {"bool_param": True, "nested": {"bool_param": False, "int_param": 42}}
        wrapped = wrap_boolean_parameters(params)
        assert isinstance(wrapped["bool_param"], BooleanWrapper)
        assert wrapped["bool_param"].value is True
        assert isinstance(wrapped["nested"]["bool_param"], BooleanWrapper)
        assert wrapped["nested"]["bool_param"].value is False
        assert wrapped["nested"]["int_param"] == 42


class TestUnwrapValue:
    """Tests for the unwrap_value function."""

    def test_unwrap_wrapped_values(self):
        """Test unwrapping wrapped values."""
        assert unwrap_value(BooleanWrapper(True)) is True
        assert unwrap_value(IntWrapper(42)) == 42
        assert unwrap_value(FloatWrapper(42.5)) == 42.5
        assert unwrap_value(StringWrapper("hello")) == "hello"

    def test_unwrap_unwrapped_values(self):
        """Test unwrapping already unwrapped values."""
        assert unwrap_value(True) is True
        assert unwrap_value(42) == 42
        assert unwrap_value(42.5) == 42.5
        assert unwrap_value("hello") == "hello"
        assert unwrap_value(None) is None


class TestUnwrapParameters:
    """Tests for the unwrap_parameters function."""

    def test_unwrap_params(self):
        """Test unwrapping parameters."""
        params = {
            "bool_param": BooleanWrapper(True),
            "int_param": IntWrapper(42),
            "float_param": FloatWrapper(42.5),
            "string_param": StringWrapper("hello"),
            "unwrapped_param": "already unwrapped",
        }
        unwrapped = unwrap_parameters(params)
        assert unwrapped["bool_param"] is True
        assert unwrapped["int_param"] == 42
        assert unwrapped["float_param"] == 42.5
        assert unwrapped["string_param"] == "hello"
        assert unwrapped["unwrapped_param"] == "already unwrapped"

    def test_unwrap_nested_params(self):
        """Test unwrapping nested parameters."""
        params = {
            "bool_param": BooleanWrapper(True),
            "nested": {"bool_param": BooleanWrapper(False), "int_param": IntWrapper(42)},
            "list_param": [BooleanWrapper(True), IntWrapper(42)],
            "tuple_param": (BooleanWrapper(True), IntWrapper(42)),
        }
        unwrapped = unwrap_parameters(params)
        assert unwrapped["bool_param"] is True
        assert unwrapped["nested"]["bool_param"] is False
        assert unwrapped["nested"]["int_param"] == 42
        assert unwrapped["list_param"][0] is True
        assert unwrapped["list_param"][1] == 42
        assert unwrapped["tuple_param"][0] is True
        assert unwrapped["tuple_param"][1] == 42

    def test_unwrap_none_params(self):
        """Test unwrapping None parameters."""
        assert unwrap_parameters(None) == {}

    def test_unwrap_empty_params(self):
        """Test unwrapping empty parameters."""
        assert unwrap_parameters({}) == {}
