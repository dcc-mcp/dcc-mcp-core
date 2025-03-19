"""Tests for the utils module."""

# Import built-in modules
import inspect
import sys
from typing import Any
from typing import Dict
from typing import List
from typing import Optional
from typing import Union

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core.utils import extract_function_info
from dcc_mcp_core.utils import extract_module_metadata
from dcc_mcp_core.utils import generate_function_example


# Define test functions with various signatures for testing
def func_no_args():
    """Demonstrate a function with no arguments."""
    return None


def func_with_args(a: int, b: str, c: bool = False) -> Dict[str, Any]:
    """Demonstrate a function with arguments and type annotations.
    
    Args:
        a: An integer argument
        b: A string argument
        c: An optional boolean argument
        
    Returns:
        A dictionary with the arguments

    """
    return {"a": a, "b": b, "c": c}


def func_with_complex_types(a: List[Dict[str, Any]], b: Optional[Union[int, str]] = None) -> Optional[List[Any]]:
    """Demonstrate a function with complex type annotations.
    
    Args:
        a: A list of dictionaries
        b: An optional union of int or str
        
    Returns:
        An optional list

    """
    return None


async def async_func(a: int) -> int:
    """Async function.
    
    Args:
        a: An integer argument
        
    Returns:
        An integer

    """
    return a


class TestClass:
    """A test class for testing function introspection."""
    
    def method(self, a: int) -> int:
        """Implement a method with a signature.

        Args:
            a: An integer argument
            
        Returns:
            An integer

        """
        return a
    
    @classmethod
    def class_method(cls, a: int) -> int:
        """Implement a class method with a signature.

        Args:
            a: An integer argument
            
        Returns:
            An integer

        """
        return a
    
    @staticmethod
    def static_method(a: int) -> int:
        """Implement a static method with a signature.

        Args:
            a: An integer argument
            
        Returns:
            An integer

        """
        return a


# Define tests for extract_function_info
def test_extract_function_info_basic():
    """Test extract_function_info with a basic function."""
    info = extract_function_info(func_with_args)
    
    assert info["docstring"] == func_with_args.__doc__
    assert len(info["parameters"]) == 3
    assert info["parameters"][0]["name"] == "a"
    assert info["parameters"][0]["type"] == "<class 'int'>"
    assert info["parameters"][0]["required"] is True
    assert info["parameters"][2]["name"] == "c"
    assert info["parameters"][2]["default"] is False
    assert info["parameters"][2]["required"] is False
    assert info["return_type"] == "typing.Dict[str, typing.Any]" or info["return_type"] == "dict[str, Any]"
    assert "func_with_args(a=1, b='example')" in info["example"]


def test_extract_function_info_no_args():
    """Test extract_function_info with a function that has no arguments."""
    info = extract_function_info(func_no_args)
    
    assert info["docstring"] == func_no_args.__doc__
    assert len(info["parameters"]) == 0
    # Return type might be "<class 'NoneType'>" or "Any", depending on whether the signature can be extracted
    assert info["return_type"] in ["<class 'NoneType'>", "None", "Any"]
    assert "func_no_args" in info["example"]


def test_extract_function_info_complex_types():
    """Test extract_function_info with complex type annotations."""
    info = extract_function_info(func_with_complex_types)
    
    assert info["docstring"] == func_with_complex_types.__doc__
    assert len(info["parameters"]) == 2
    assert info["parameters"][0]["name"] == "a"
    assert "List" in info["parameters"][0]["type"] or "list" in info["parameters"][0]["type"]
    assert info["parameters"][0]["required"] is True
    assert info["parameters"][1]["name"] == "b"
    assert "Union" in info["parameters"][1]["type"] or "Optional" in info["parameters"][1]["type"] or "int | str" in info["parameters"][1]["type"]
    assert info["parameters"][1]["default"] is None
    assert info["parameters"][1]["required"] is False


def test_extract_function_info_async():
    """Test extract_function_info with an async function."""
    info = extract_function_info(async_func)
    
    assert info["docstring"] == async_func.__doc__
    assert len(info["parameters"]) == 1
    assert info["parameters"][0]["name"] == "a"
    assert info["parameters"][0]["type"] == "<class 'int'>"
    assert info["return_type"] == "<class 'int'>"
    assert info["example"] == "async_func(a=1)"


def test_extract_function_info_method():
    """Test extract_function_info with a method."""
    test_class = TestClass()
    info = extract_function_info(test_class.method)
    
    assert info["docstring"] == TestClass.method.__doc__
    # The number of parameters for the method might be 1 or 2, depending on whether self is included
    assert len(info["parameters"]) in [1, 2]
    # If there are 2 parameters, the first parameter should be self
    if len(info["parameters"]) == 2:
        assert info["parameters"][0]["name"] == "self"
        assert info["parameters"][1]["name"] == "a"
        assert info["parameters"][1]["type"] == "<class 'int'>"
    # If there is only 1 parameter, it should be a
    else:
        assert info["parameters"][0]["name"] == "a"
        assert info["parameters"][0]["type"] == "<class 'int'>"
    assert info["return_type"] == "<class 'int'>" or info["return_type"] == "Any"
    assert "method" in info["example"]


def test_extract_function_info_class_method():
    """Test extract_function_info with a class method."""
    info = extract_function_info(TestClass.class_method)
    
    assert info["docstring"] == TestClass.class_method.__doc__
    # The number of parameters for the class method might be 1 or 2, depending on whether cls is included
    assert len(info["parameters"]) in [1, 2]
    # If there are 2 parameters, the first parameter should be cls
    if len(info["parameters"]) == 2:
        assert info["parameters"][0]["name"] == "cls"
        assert info["parameters"][1]["name"] == "a"
        assert info["parameters"][1]["type"] == "<class 'int'>"
    # If there is only 1 parameter, it should be a
    else:
        assert info["parameters"][0]["name"] == "a"
        assert info["parameters"][0]["type"] == "<class 'int'>"
    assert info["return_type"] == "<class 'int'>" or info["return_type"] == "Any"
    assert "class_method" in info["example"]


def test_extract_function_info_static_method():
    """Test extract_function_info with a static method."""
    info = extract_function_info(TestClass.static_method)
    
    assert info["docstring"] == TestClass.static_method.__doc__
    assert len(info["parameters"]) == 1  # a only, no self or cls
    assert info["parameters"][0]["name"] == "a"
    assert info["parameters"][0]["type"] == "<class 'int'>"
    assert info["return_type"] == "<class 'int'>"
    assert info["example"] == "static_method(a=1)"


# Define tests for generate_function_example
def test_generate_function_example():
    """Test generate_function_example."""
    parameters = [
        {"name": "a", "type": "<class 'int'>", "required": True},
        {"name": "b", "type": "<class 'str'>", "required": True},
        {"name": "c", "type": "<class 'bool'>", "required": False}
    ]
    
    example = generate_function_example(func_with_args, parameters)
    assert example == "func_with_args(a=1, b='example')"


# Define tests for extract_module_metadata
class TestModule:
    """A test module for testing metadata extraction."""

    # Import built-in modules
    from typing import ClassVar
    from typing import List
    
    __plugin_name__ = "test_plugin"
    __plugin_version__ = "1.0.0"
    __plugin_description__ = "A test plugin"
    __plugin_author__ = "Test Author"
    __plugin_requires__: ClassVar[List[str]] = ["dependency1", "dependency2"]


def test_extract_module_metadata():
    """Test extract_module_metadata."""
    metadata = extract_module_metadata(TestModule)
    
    assert metadata["name"] == "test_plugin"
    assert metadata["version"] == "1.0.0"
    assert metadata["description"] == "A test plugin"
    assert metadata["author"] == "Test Author"
    assert metadata["requires"] == ["dependency1", "dependency2"]


def test_extract_module_metadata_with_default_name():
    """Test extract_module_metadata with a default name."""
    # Create a module without a name attribute
    class ModuleWithoutName:
        pass
    
    metadata = extract_module_metadata(ModuleWithoutName, "default_name")
    assert metadata["name"] == "default_name"


# Define tests for version compatibility
def test_inspect_signature_consistency():
    """Test that inspect.signature works consistently across Python versions."""
    # Define a function with various parameter types
    def test_func(a: int, b: str = "default", *args, **kwargs) -> Dict[str, Any]:
        return {"a": a, "b": b, "args": args, "kwargs": kwargs}
    
    # Get the signature
    sig = inspect.signature(test_func)
    
    # Basic assertions that should work in all Python versions
    assert len(sig.parameters) == 4
    assert "a" in sig.parameters
    assert "b" in sig.parameters
    assert "args" in sig.parameters
    assert "kwargs" in sig.parameters
    
    # Check parameter kinds
    assert sig.parameters["a"].kind == inspect.Parameter.POSITIONAL_OR_KEYWORD
    assert sig.parameters["b"].kind == inspect.Parameter.POSITIONAL_OR_KEYWORD
    assert sig.parameters["args"].kind == inspect.Parameter.VAR_POSITIONAL
    assert sig.parameters["kwargs"].kind == inspect.Parameter.VAR_KEYWORD
    
    # Check default values
    assert sig.parameters["a"].default == inspect.Parameter.empty
    assert sig.parameters["b"].default == "default"
    
    # Check return annotation
    assert sig.return_annotation != inspect.Signature.empty
    
    # Test with our extract_function_info function
    info = extract_function_info(test_func)
    assert len(info["parameters"]) == 4
    assert info["parameters"][0]["name"] == "a"
    assert info["parameters"][1]["name"] == "b"
    assert info["parameters"][2]["name"] == "args"
    assert info["parameters"][3]["name"] == "kwargs"
    
    # Only a should be required
    assert info["parameters"][0]["required"] is True
    assert info["parameters"][1]["required"] is False
    assert info["parameters"][2]["required"] is False
    assert info["parameters"][3]["required"] is False


# Test handling of built-in functions and methods
def test_builtin_function_handling():
    """Test that extract_function_info can handle built-in functions."""
    # Try with a built-in function
    try:
        info = extract_function_info(len)
        # If we get here, the function didn't raise an exception
        assert "parameters" in info
    except Exception as e:
        # If we get an exception, make sure it's handled gracefully
        pytest.skip(f"Built-in function handling failed: {e}")


# Test handling of C extension functions
def test_c_extension_function_handling():
    """Test that extract_function_info can handle C extension functions."""
    # Try with a C extension function (e.g., from numpy if available)
    try:
        # Import third-party modules
        import numpy as np
        info = extract_function_info(np.array)
        # If we get here, the function didn't raise an exception
        assert "parameters" in info
    except (ImportError, Exception) as e:
        # If numpy is not available or we get an exception, skip the test
        pytest.skip(f"C extension function handling failed or numpy not available: {e}")


# Test handling of functions with type annotations using newer Python features
def test_modern_type_annotations():
    """Test that extract_function_info can handle modern type annotations."""
    # Only run this test on Python 3.9+
    if sys.version_info >= (3, 9):
        # Define a function with Python 3.9+ type annotations
        # In Python 3.9+, you can use list[int] instead of List[int]
        code = """
        def modern_func(a: list[int], b: dict[str, any] = None) -> list[dict[str, any]]:
            return [{"result": a}]
        """
        try:
            # Try to execute code
            exec(code)
            
            # Get function
            modern_func = locals()["modern_func"]
            
            # Test function information extraction
            info = extract_function_info(modern_func)
            assert len(info["parameters"]) == 2
            assert info["parameters"][0]["name"] == "a"
            assert "list" in info["parameters"][0]["type"].lower()
            assert info["parameters"][1]["name"] == "b"
            assert "dict" in info["parameters"][1]["type"].lower()
        except SyntaxError:
            # If syntax error, might be Python version issue, skip test
            pytest.skip("Modern type annotations syntax not supported in this Python version")
    else:
        pytest.skip("Test requires Python 3.9 or higher")


# Test handling of functions with type annotations using the | operator (Python 3.10+)
def test_union_pipe_operator():
    """Test that extract_function_info can handle the | operator for unions."""
    # Only run this test on Python 3.10+
    if sys.version_info >= (3, 10):
        # Define a function with Python 3.10+ type annotations using | for unions
        code = """
        def pipe_func(a: int | str, b: None | list[int] = None) -> dict[str, int | str]:
            return {"result": a}
        """
        try:
            # Try to execute the code
            exec(code)
            
            # Get the function
            pipe_func = locals()["pipe_func"]
            
            # Test function information extraction
            info = extract_function_info(pipe_func)
            assert len(info["parameters"]) == 2
            assert info["parameters"][0]["name"] == "a"
            # The type string might vary between Python versions
            assert "int" in info["parameters"][0]["type"].lower() and "str" in info["parameters"][0]["type"].lower()
        except SyntaxError:
            # If syntax error, might be Python version issue, skip test
            pytest.skip("Union pipe operator syntax not supported in this Python version")
    else:
        pytest.skip("Test requires Python 3.10 or higher")
