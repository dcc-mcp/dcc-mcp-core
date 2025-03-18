"""Tests for the parameters module."""

import json
import ast
import pytest

from dcc_mcp_core import exceptions
from dcc_mcp_core.parameters import (
    process_parameters,
    parse_kwargs_string,
    parse_json,
    parse_ast_literal,
    parse_key_value_pairs,
)


def test_process_parameters():
    """Test processing parameters."""
    # Test with dictionary parameters
    params = {"name": "John", "age": 30, "is_active": True}
    result = process_parameters(params)
    assert result == params
    
    # Test with string parameters
    params_str = '{"name": "John", "age": 30, "is_active": true}'
    result = process_parameters(params_str)
    assert result == {"name": "John", "age": 30, "is_active": True}
    
    # Test with key=value string parameters
    params_str = 'name=John age=30 is_active=True'
    result = process_parameters(params_str)
    assert result == {"name": "John", "age": 30, "is_active": True}
    
    # Test boolean conversion
    params = {"query": 1, "edit": 0, "normal_value": 1}
    result = process_parameters(params)
    assert result == {"query": True, "edit": False, "normal_value": 1}


def test_parse_kwargs_string():
    """Test parsing kwargs string."""
    # Test with JSON format
    kwargs_str = '{"name": "John", "age": 30}'
    result = parse_kwargs_string(kwargs_str)
    assert result == {"name": "John", "age": 30}
    
    # Test with Python dict literal format
    kwargs_str = "{'name': 'John', 'age': 30}"
    result = parse_kwargs_string(kwargs_str)
    assert result == {"name": "John", "age": 30}
    
    # Test with key=value format
    kwargs_str = "name=John age=30"
    result = parse_kwargs_string(kwargs_str)
    assert result == {"name": "John", "age": 30}
    
    # Test with invalid format
    kwargs_str = "invalid format"
    result = parse_kwargs_string(kwargs_str)
    assert result == {}


def test_parse_json():
    """Test parsing JSON string."""
    # Test with valid JSON
    kwargs_str = '{"name": "John", "age": 30, "is_active": true}'
    result = parse_json(kwargs_str)
    assert result == {"name": "John", "age": 30, "is_active": True}
    
    # Test with invalid JSON
    kwargs_str = '{name: John}'
    with pytest.raises(json.JSONDecodeError):
        parse_json(kwargs_str)


def test_parse_ast_literal():
    """Test parsing string using ast.literal_eval."""
    # Test with valid Python dict literal
    kwargs_str = "{'name': 'John', 'age': 30, 'is_active': True}"
    result = parse_ast_literal(kwargs_str)
    assert result == {"name": "John", "age": 30, "is_active": True}
    
    # Test with invalid Python dict literal
    kwargs_str = "{name: 'John'}"
    with pytest.raises(SyntaxError):
        parse_ast_literal(kwargs_str)
    
    # Test with non-dict result
    kwargs_str = "[1, 2, 3]"
    with pytest.raises(ValueError):
        parse_ast_literal(kwargs_str)


def test_parse_key_value_pairs():
    """Test parsing key=value pairs."""
    # Test with simple key=value pairs
    kwargs_str = "name=John age=30 is_active=True"
    result = parse_key_value_pairs(kwargs_str)
    assert result == {"name": "John", "age": 30, "is_active": True}
    
    # Test with quoted values
    kwargs_str = 'name="John Doe" age=30'
    result = parse_key_value_pairs(kwargs_str)
    assert result == {"name": "John Doe", "age": 30}
    
    # Test with boolean values
    kwargs_str = "is_active=True is_admin=False"
    result = parse_key_value_pairs(kwargs_str)
    assert result == {"is_active": True, "is_admin": False}
    
    # Test with None value
    kwargs_str = "name=None age=30"
    result = parse_key_value_pairs(kwargs_str)
    assert result == {"name": None, "age": 30}
    
    # Test with numeric values
    kwargs_str = "int_val=42 float_val=3.14"
    result = parse_key_value_pairs(kwargs_str)
    assert result == {"int_val": 42, "float_val": 3.14}
    
    # Test with boolean flags
    kwargs_str = "query=1 edit=0"
    result = parse_key_value_pairs(kwargs_str)
    assert result == {"query": True, "edit": False}
