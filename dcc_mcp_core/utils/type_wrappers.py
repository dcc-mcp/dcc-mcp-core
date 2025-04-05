#!/usr/bin/env python
"""Type Wrappers Module.

This module provides basic type wrapper classes for maintaining type integrity in remote process calls.
These wrappers are particularly useful for maintaining specific types of data when transmitted through RPyC.
"""

# Import built-in modules
import logging
from typing import Any
from typing import Dict

logger = logging.getLogger(__name__)


class BaseWrapper:
    """Base type wrapper class.

    All specific type wrappers should inherit from this class.
    Provides basic serialization support and common methods.
    """

    def __init__(self, value: Any):
        """Initialize the wrapper.

        Args:
            value: The value to wrap

        """
        self.value = value

    def __reduce__(self):
        """Support for serialization.

        This method is called by the pickle module when the instance is serialized.
        Ensures that the wrapper can be correctly serialized and deserialized during network transmission.

        Returns:
            A tuple (callable, args), where callable is a callable object,
            args is a tuple containing callable arguments.

        """
        return (self.__class__, (self.value,))

    def __repr__(self) -> str:
        """Return the string representation of the wrapper.

        Returns:
            The string representation of the wrapper

        """
        return f"{self.__class__.__name__}({self.value!r})"

    def __str__(self) -> str:
        """Return the string representation of the wrapper.

        Returns:
            The string representation of the wrapper

        """
        return str(self.value)

    def __eq__(self, other: Any) -> bool:
        """Check if the wrapper's value is equal to another value.

        Args:
            other: The value to compare

        Returns:
            True if the values are equal, otherwise False

        """
        if isinstance(other, self.__class__):
            return self.value == other.value
        return self.value == other


class BooleanWrapper(BaseWrapper):
    """Boolean wrapper class.

    This class wraps a boolean value and ensures it is preserved in remote process calls.
    Provides methods for converting to boolean and operations related to boolean values.
    """

    def __init__(self, value: Any):
        """Initialize the boolean wrapper.

        Args:
            value: The value to wrap (will be converted to boolean)

        """
        self.value = self._to_bool(value)

    def _to_bool(self, value: Any) -> bool:
        """Convert the value to a boolean.

        Args:
            value: The value to convert

        Returns:
            Boolean representation

        """
        if isinstance(value, bool):
            return value
        elif isinstance(value, (int, float)):
            return bool(value)
        elif isinstance(value, str):
            lower_val = value.lower()
            if lower_val in ("true", "1", "yes", "on"):
                return True
            elif lower_val in ("false", "0", "no", "off"):
                return False
            else:
                return False
        else:
            # Try converting other types
            try:
                return bool(value)
            except (ValueError, TypeError):
                return False

    def __bool__(self) -> bool:
        """Return the boolean value.

        Returns:
            Boolean value

        """
        return self.value


class IntWrapper(BaseWrapper):
    """Integer wrapper class.

    This class wraps an integer and ensures it is preserved in remote process calls.
    Provides methods for converting to integer and operations related to integer values.
    """

    def __init__(self, value: Any):
        """Initialize the integer wrapper.

        Args:
            value: The value to wrap (will be converted to integer)

        """
        self.value = self._to_int(value)

    def _to_int(self, value: Any) -> int:
        """Convert the value to an integer.

        Args:
            value: The value to convert

        Returns:
            Integer representation

        """
        if isinstance(value, int) and not isinstance(value, bool):
            return value
        elif isinstance(value, bool):
            return 1 if value else 0
        elif isinstance(value, float):
            return int(value)
        elif isinstance(value, str):
            try:
                return int(value)
            except (ValueError, TypeError):
                return 0
        else:
            # Try converting other types
            try:
                return int(value)
            except (ValueError, TypeError):
                return 0

    def __int__(self) -> int:
        """Return the integer value.

        Returns:
            Integer value

        """
        return self.value

    def __index__(self) -> int:
        """Return the integer index.

        Returns:
            Integer index

        """
        return self.value


class FloatWrapper(BaseWrapper):
    """Floating-point wrapper class.

    This class wraps a floating-point number and ensures it is preserved in remote process calls.
    Provides methods for converting to float and operations related to floating-point values.
    """

    def __init__(self, value: Any):
        """Initialize the float wrapper.

        Args:
            value: The value to wrap (will be converted to float)

        """
        self.value = self._to_float(value)

    def _to_float(self, value: Any) -> float:
        """Convert the value to a float.

        Args:
            value: The value to convert

        Returns:
            Float representation

        """
        if isinstance(value, float):
            return value
        elif isinstance(value, int):
            return float(value)
        elif isinstance(value, bool):
            return 1.0 if value else 0.0
        elif isinstance(value, str):
            try:
                return float(value)
            except (ValueError, TypeError):
                return 0.0
        else:
            # Try converting other types
            try:
                return float(value)
            except (ValueError, TypeError):
                return 0.0

    def __float__(self) -> float:
        """Return the float value.

        Returns:
            Float value

        """
        return self.value


class StringWrapper(BaseWrapper):
    """String wrapper class.

    This class wraps a string and ensures it is preserved in remote process calls.
    Provides methods for converting to string and operations related to string values.
    """

    def __init__(self, value: Any):
        """Initialize the string wrapper.

        Args:
            value: The value to wrap (will be converted to string)

        """
        self.value = self._to_str(value)

    def _to_str(self, value: Any) -> str:
        """Convert the value to a string.

        Args:
            value: The value to convert

        Returns:
            String representation

        """
        if isinstance(value, str):
            return value
        else:
            # Try converting other types
            try:
                return str(value)
            except (ValueError, TypeError):
                return ""

    def __str__(self) -> str:
        """Return the string value.

        Returns:
            String value

        """
        return self.value


def wrap_value(value: Any) -> Any:
    """Select an appropriate wrapper based on the value's type.

    Args:
        value: The value to wrap

    Returns:
        Wrapped value

    """
    if isinstance(value, bool):
        return BooleanWrapper(value)
    elif isinstance(value, int) and not isinstance(value, bool):
        return IntWrapper(value)
    elif isinstance(value, float):
        return FloatWrapper(value)
    elif isinstance(value, str):
        return StringWrapper(value)
    else:
        return value


def wrap_boolean_parameters(params: Dict[str, Any]) -> Dict[str, Any]:
    """Wrap boolean parameters in a dictionary.

    Args:
        params: Dictionary containing parameters

    Returns:
        Dictionary containing wrapped parameters

    """
    result = {}
    for key, value in params.items():
        if isinstance(value, bool):
            result[key] = BooleanWrapper(value)
        elif isinstance(value, dict):
            # Recursively process nested dictionaries
            result[key] = wrap_boolean_parameters(value)
        else:
            result[key] = value
    return result


def unwrap_value(value: Any) -> Any:
    """Unwrap the wrapped value.

    Args:
        value: The value that may be wrapped

    Returns:
        Unwrapped value

    """
    if hasattr(value, "__class__") and "Wrapper" in value.__class__.__name__:
        return value.value
    return value


def unwrap_parameters(params: Dict[str, Any]) -> Dict[str, Any]:
    """Unwrap all wrapped parameters in a dictionary.

    Args:
        params: Dictionary containing parameters that may be wrapped

    Returns:
        Dictionary containing unwrapped parameters

    """
    if not params:
        return {}

    result = {}
    for key, value in params.items():
        # 处理包装器类型
        if hasattr(value, "__class__") and "Wrapper" in value.__class__.__name__:
            result[key] = value.value
        # 递归处理字典
        elif isinstance(value, dict):
            result[key] = unwrap_parameters(value)
        # 处理列表
        elif isinstance(value, list):
            result[key] = [unwrap_value(item) for item in value]
        # 处理元组
        elif isinstance(value, tuple):
            result[key] = tuple(unwrap_value(item) for item in value)
        # 处理集合
        elif isinstance(value, set):
            result[key] = set(unwrap_value(item) for item in value)
        # 其他类型直接保留
        else:
            result[key] = value
    return result
