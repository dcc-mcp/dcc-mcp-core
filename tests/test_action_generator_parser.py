"""Tests for the _parse_functions_description function in actions.generator module."""

# Import built-in modules

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core.actions.generator import _parse_functions_description


@pytest.fixture
def sample_functions_description():
    """Fixture to provide a sample natural language description of functions."""
    return """
    Function 1: create_sphere
    This function creates a sphere in the scene.
    Parameter: radius (float) - Radius of the sphere
    Parameter: segments (int) - Number of segments

    Function 2: delete_objects
    This function deletes selected objects from the scene.
    Parameter: confirm (bool) - Confirm deletion
    """


def test_parse_functions_with_mixed_parameter_formats():
    """Test parsing functions with mixed parameter formats."""
    description = """
    Function: create_material
    Creates a new material with specified properties.
    Parameter: name (str) - Name of the material
    param: color (List[float]) - RGB color values
    arg: roughness (float) - Surface roughness
    Parameter roughness_map (str) - Path to roughness texture
    """

    result = _parse_functions_description(description)

    assert len(result) == 1
    assert result[0]["name"] == "create_material"
    assert result[0]["description"] == "Creates a new material with specified properties."

    # Check parameters
    params = result[0]["parameters"]
    assert len(params) == 4

    # Check each parameter
    param_names = [p["name"] for p in params]
    assert "name" in param_names
    assert "color" in param_names
    assert "roughness" in param_names
    assert "roughness_map" in param_names

    # Check parameter types
    name_param = next(p for p in params if p["name"] == "name")
    assert name_param["type"] == "str"

    color_param = next(p for p in params if p["name"] == "color")
    assert color_param["type"] == "float"

    roughness_param = next(p for p in params if p["name"] == "roughness")
    assert roughness_param["type"] == "float"

    map_param = next(p for p in params if p["name"] == "roughness_map")
    assert map_param["type"] == "str"


def test_parse_functions_with_unusual_formatting():
    """Test parsing functions with unusual formatting and spacing."""
    description = """
    Function:   render_scene    
      This function renders the current scene with specified settings.
      
    Parameter:   output_path    (str)   -    Path to save the rendered image
    Parameter:quality(int)-Render quality (1-100)
    Parameter:   use_gpu    (bool)   
    """

    result = _parse_functions_description(description)

    assert len(result) == 1
    assert result[0]["name"] == "render_scene"
    assert result[0]["description"] == "This function renders the current scene with specified settings."

    # Check parameters
    params = result[0]["parameters"]
    assert len(params) == 3

    # Check parameter names and types
    output_param = next(p for p in params if p["name"] == "output_path")
    assert output_param["type"] == "str"

    quality_param = next(p for p in params if p["name"] == "quality")
    assert quality_param["type"] == "int"

    gpu_param = next(p for p in params if p["name"] == "use_gpu")
    assert gpu_param["type"] == "bool"


def test_parse_functions_with_markdown_style():
    """Test parsing functions with markdown-style formatting."""
    description = """
    ## export_model
    
    Exports the current model to various file formats.
    
    * **file_path** (str): Path to save the exported file
    * **format** (str): Export format (obj, fbx, gltf)
    * **include_materials** (bool): Whether to include materials
    """

    result = _parse_functions_description(description)

    assert len(result) == 1
    assert result[0]["name"] == "export_model"

    # The function should have no parameters since our parser doesn't recognize the markdown format
    # This test demonstrates a limitation of the current parser
    assert len(result[0]["parameters"]) == 0


def test_parse_functions_with_multiple_descriptions():
    """Test parsing functions with multiple description lines."""
    description = """
    Function: animate_object
    This function creates an animation for an object.
    It supports various animation types and easing functions.
    Parameter: object_name (str) - Name of the object to animate
    Parameter: duration (float) - Animation duration in seconds
    """

    result = _parse_functions_description(description)

    assert len(result) == 1
    assert result[0]["name"] == "animate_object"
    # The parser should pick the first non-parameter line after the function name as description
    assert result[0]["description"] == "This function creates an animation for an object."

    # Check parameters
    params = result[0]["parameters"]
    assert len(params) == 2

    # Check parameter names
    param_names = [p["name"] for p in params]
    assert "object_name" in param_names
    assert "duration" in param_names


def test_parse_functions_with_no_parameters():
    """Test parsing functions with no parameters."""
    description = """
    Function: get_scene_statistics
    Returns statistics about the current scene including object count and memory usage.
    """

    result = _parse_functions_description(description)

    assert len(result) == 1
    assert result[0]["name"] == "get_scene_statistics"
    assert (
        result[0]["description"]
        == "Returns statistics about the current scene including object count and memory usage."
    )
    assert len(result[0]["parameters"]) == 0


def test_parse_functions_with_multiple_functions_no_numbering():
    """Test parsing multiple functions without numbering."""
    description = """
    Function: create_cube
    Creates a cube in the scene.
    Parameter: size (float) - Size of the cube
    
    Function: create_cylinder
    Creates a cylinder in the scene.
    Parameter: radius (float) - Radius of the cylinder
    Parameter: height (float) - Height of the cylinder
    """

    result = _parse_functions_description(description)

    assert len(result) == 2

    # Check first function
    assert result[0]["name"] == "create_cube"
    assert result[0]["description"] == "Creates a cube in the scene."
    assert len(result[0]["parameters"]) == 1
    assert result[0]["parameters"][0]["name"] == "size"

    # Check second function
    assert result[1]["name"] == "create_cylinder"
    assert result[1]["description"] == "Creates a cylinder in the scene."
    assert len(result[1]["parameters"]) == 2
    param_names = [p["name"] for p in result[1]["parameters"]]
    assert "radius" in param_names
    assert "height" in param_names


def test_parse_functions_with_parameter_default_values():
    """Test that the parser sets default values to None for parameters."""
    description = """
    Function: set_viewport_settings
    Sets viewport display settings.
    Parameter: wireframe (bool) - Show wireframe
    Parameter: shadows (bool) - Show shadows
    """

    result = _parse_functions_description(description)

    assert len(result) == 1
    params = result[0]["parameters"]
    assert len(params) == 2

    # Check that default values are set to None
    wireframe_param = next(p for p in params if p["name"] == "wireframe")
    assert wireframe_param["default"] is None

    shadows_param = next(p for p in params if p["name"] == "shadows")
    assert shadows_param["default"] is None


def test_parse_functions_with_invalid_input():
    """Test parsing with invalid input that doesn't match expected patterns."""
    description = "This is not a valid function description format."

    result = _parse_functions_description(description)

    assert len(result) == 1
    # Check that the function name is extracted from the text
    assert result[0]["name"] == "This"
    # Check that the description is set
    assert "description" in result[0]
    assert len(result[0]["parameters"]) == 0
