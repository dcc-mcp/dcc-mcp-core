"""Tests for the template utilities module."""

import pytest

from dcc_mcp_core.utils.template import render_template, get_template


# Mock template directory and template content for testing
@pytest.fixture
def template_dir(tmp_path):
    """Create a temporary directory with a test template."""
    template_content = "Hello, {{ name }}!"
    template_path = tmp_path / "test.template"
    template_path.write_text(template_content)
    return str(tmp_path)


def test_render_template(template_dir):
    """Test the render_template function."""
    # Test with a valid template and context
    result = render_template("test.template", {"name": "World"}, template_dir=template_dir)
    assert result == "Hello, World!"
    
    # Test with an empty context - should not raise exception
    result = render_template("test.template", {}, template_dir=template_dir)
    assert "Hello, " in result
    
    # Test with a non-existent template
    with pytest.raises(Exception):
        render_template("non_existent.template", {"name": "World"}, template_dir=template_dir)


def test_get_template(template_dir):
    """Test the get_template function."""
    # Test with a valid template
    template_content = get_template("test.template", template_dir=template_dir)
    assert template_content == "Hello, {{ name }}!"
    
    # Test with a non-existent template
    with pytest.raises(Exception):
        get_template("non_existent.template", template_dir=template_dir)
