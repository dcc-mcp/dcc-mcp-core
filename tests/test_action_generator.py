"""Tests for the actions.generator module."""

# Import built-in modules
from datetime import datetime
import os
from typing import Any
from typing import Dict
from typing import List
from unittest.mock import MagicMock
from unittest.mock import mock_open
from unittest.mock import patch

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core.actions.generator import _generate_action_content
from dcc_mcp_core.actions.generator import _parse_functions_description
from dcc_mcp_core.actions.generator import create_action_template
from dcc_mcp_core.actions.generator import generate_action_for_ai
from dcc_mcp_core.models import ActionResultModel
from dcc_mcp_core.utils.platform import get_actions_dir


@pytest.fixture
def sample_function_list():
    """Fixture to provide a sample list of function definitions."""
    return [
        {
            "name": "create_sphere",
            "description": "Create a sphere in the scene",
            "parameters": [
                {
                    "name": "radius",
                    "type": "float",
                    "description": "Radius of the sphere",
                    "default": 1.0
                },
                {
                    "name": "segments",
                    "type": "int",
                    "description": "Number of segments",
                    "default": 8
                }
            ],
            "return_description": "Returns an ActionResultModel with the created sphere information"
        }
    ]


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


@pytest.fixture
def temp_actions_dir(tmp_path):
    """Create a temporary directory for actions."""
    actions_dir = tmp_path / "actions" / "maya"
    actions_dir.mkdir(parents=True, exist_ok=True)
    return str(actions_dir)


@patch('dcc_mcp_core.actions.generator.get_actions_dir')
@patch('os.makedirs')
@patch('os.path.exists')
@patch('dcc_mcp_core.actions.generator._generate_action_content')
@patch('dcc_mcp_core.actions.generator.open', new_callable=mock_open)
def test_create_action_template(mock_open, mock_generate_content, mock_exists,
                               mock_makedirs, mock_get_actions_dir,
                               sample_function_list, temp_actions_dir):
    """Test creating an action template file."""
    # Setup mocks
    mock_get_actions_dir.return_value = temp_actions_dir
    mock_exists.return_value = False
    mock_generate_content.return_value = 'Test content'
    mock_file = mock_open.return_value.__enter__.return_value

    # Call the function
    result = create_action_template(
        dcc_name='maya',
        action_name='test_action',
        description='Test action description',
        functions=sample_function_list,
        author='Test Author'
    )

    # Verify the result
    assert result.success is True
    assert 'Created action file' in result.message
    expected_path = os.path.join(temp_actions_dir, 'test_action.py')
    assert result.context['file_path'] == expected_path

    # Verify the mocks were called correctly
    mock_get_actions_dir.assert_called_once_with('maya')
    mock_makedirs.assert_called_once_with(temp_actions_dir, exist_ok=True)
    mock_exists.assert_called_once_with(expected_path)
    mock_generate_content.assert_called_once_with(
        'test_action', 'Test action description', sample_function_list, 'Test Author', 'maya'
    )
    mock_open.assert_called_once_with(expected_path, 'w')
    mock_file.write.assert_called_once_with('Test content')


@patch('dcc_mcp_core.actions.generator.get_actions_dir')
@patch('os.path.exists')
def test_create_action_template_file_exists(mock_exists, mock_get_actions_dir,
                                          sample_function_list, temp_actions_dir):
    """Test creating an action template when the file already exists."""
    # Setup mocks
    mock_get_actions_dir.return_value = temp_actions_dir
    mock_exists.return_value = True

    # Call the function
    result = create_action_template(
        dcc_name='maya',
        action_name='test_action',
        description='Test action description',
        functions=sample_function_list
    )

    # Verify the result
    assert result.success is False
    assert 'already exists' in result.message
    expected_path = os.path.join(temp_actions_dir, 'test_action.py')
    assert result.context['file_path'] == expected_path


@patch('dcc_mcp_core.actions.generator.get_actions_dir')
@patch('os.makedirs')
@patch('os.path.exists')
@patch('dcc_mcp_core.actions.generator._generate_action_content')
@patch('dcc_mcp_core.actions.generator.open', side_effect=Exception('Test exception'))
def test_create_action_template_exception(mock_open, mock_generate_content, mock_exists,
                                       mock_makedirs, mock_get_actions_dir,
                                       sample_function_list, temp_actions_dir):
    """Test handling exceptions when creating an action template."""
    # Setup mocks
    mock_get_actions_dir.return_value = temp_actions_dir
    mock_exists.return_value = False
    mock_generate_content.return_value = 'Test content'

    # Call the function
    result = create_action_template(
        dcc_name='maya',
        action_name='test_action',
        description='Test action description',
        functions=sample_function_list
    )

    # Verify the result
    assert result.success is False
    assert 'Failed to create action file' in result.message
    assert 'Test exception' in result.message


@patch('dcc_mcp_core.actions.generator.render_template')
def test_generate_action_file_content(mock_render_template, sample_function_list):
    """Test generating action file content."""
    # Setup mocks
    mock_render_template.return_value = 'Test content'

    # Call the function
    result = _generate_action_content(
        action_name='test_action',
        description='Test action description',
        functions=sample_function_list,
        author='Test Author',
        dcc_name='maya'
    )

    # Verify the result
    assert result == 'Test content'

    # Get the current date for comparison
    current_date = datetime.now().strftime('%Y-%m-%d')

    # Verify the mock was called correctly with the date parameter
    mock_render_template.assert_called_once_with(
        'action.template',
        {
            'action_name': 'test_action',
            'description': 'Test action description',
            'functions': sample_function_list,
            'author': 'Test Author',
            'dcc_name': 'maya',
            'date': current_date
        }
    )


@patch('dcc_mcp_core.actions.generator.create_action_template')
@patch('dcc_mcp_core.actions.generator._parse_functions_description')
@patch('dcc_mcp_core.actions.generator.get_actions_dir')
def test_generate_action_for_ai_success(mock_get_actions_dir, mock_parse_description, mock_create_template,
                                      sample_function_list, sample_functions_description,
                                      temp_actions_dir):
    """Test generating an action for AI with success."""
    # Setup mocks
    mock_get_actions_dir.return_value = temp_actions_dir
    mock_parse_description.return_value = sample_function_list
    expected_path = os.path.join(temp_actions_dir, 'test_action.py')
    mock_create_template.return_value = ActionResultModel(
        success=True,
        message='Created action file',
        context={"file_path": expected_path}
    )

    # Call the function
    result = generate_action_for_ai(
        dcc_name='maya',
        action_name='test_action',
        description='Test action description',
        functions_description=sample_functions_description
    )

    # Verify the result
    assert result.success is True
    assert 'Successfully created action template' in result.message
    assert result.prompt is not None
    assert result.context is not None
    assert result.context['file_path'] == expected_path
    assert result.context['action_name'] == 'test_action'
    assert 'functions' in result.context

    # Verify the mocks were called correctly
    mock_parse_description.assert_called_once_with(sample_functions_description)
    mock_create_template.assert_called_once_with(
        'maya', 'test_action', 'Test action description', sample_function_list
    )


@patch('dcc_mcp_core.actions.generator.create_action_template')
@patch('dcc_mcp_core.actions.generator._parse_functions_description')
@patch('dcc_mcp_core.actions.generator.get_actions_dir')
def test_generate_action_for_ai_failure(mock_get_actions_dir, mock_parse_description, mock_create_template,
                                       sample_function_list, sample_functions_description,
                                       temp_actions_dir):
    """Test generating an action for AI with failure."""
    # Setup mocks
    mock_get_actions_dir.return_value = temp_actions_dir
    mock_parse_description.return_value = sample_function_list
    expected_path = os.path.join(temp_actions_dir, 'test_action.py')
    mock_create_template.return_value = ActionResultModel(
        success=False,
        message='Failed to create action file',
        context={"file_path": expected_path, "error": "Failed to create action file"}
    )

    # Call the function
    result = generate_action_for_ai(
        dcc_name='maya',
        action_name='test_action',
        description='Test action description',
        functions_description=sample_functions_description
    )

    # Verify the result
    assert result.success is False
    assert 'Failed to create action template' in result.message
    assert result.error is not None
    assert result.context is not None
    assert result.context['file_path'] == expected_path
    assert result.context.get('error') == 'Failed to create action file'


@patch('dcc_mcp_core.actions.generator._parse_functions_description')
@patch('dcc_mcp_core.actions.generator.get_actions_dir')
def test_generate_action_for_ai_exception(mock_get_actions_dir, mock_parse_description,
                                        sample_functions_description, temp_actions_dir):
    """Test handling exceptions when generating an action for AI."""
    # Setup mocks
    mock_get_actions_dir.return_value = temp_actions_dir
    mock_parse_description.side_effect = Exception('Test exception')

    # Call the function
    result = generate_action_for_ai(
        dcc_name='maya',
        action_name='test_action',
        description='Test action description',
        functions_description=sample_functions_description
    )

    # Verify the result
    assert result.success is False
    assert 'Error generating action template' in result.message
    assert 'Test exception' in result.error
    assert result.context is not None
    assert result.context.get('error') == 'Test exception'


def test_parse_functions_description_with_function_keyword(sample_functions_description):
    """Test parsing functions description with 'Function' keyword."""
    # Call the function
    functions = _parse_functions_description(sample_functions_description)

    # Verify the result
    assert len(functions) == 2

    # Check first function
    assert functions[0]['name'] == 'create_sphere'
    assert 'creates a sphere' in functions[0]['description']
    assert len(functions[0]['parameters']) == 2
    assert functions[0]['parameters'][0]['name'] == 'radius'
    assert functions[0]['parameters'][0]['type'] == 'float'
    assert functions[0]['parameters'][1]['name'] == 'segments'
    assert functions[0]['parameters'][1]['type'] == 'int'

    # Check second function
    assert functions[1]['name'] == 'delete_objects'
    assert 'deletes selected objects' in functions[1]['description']
    assert len(functions[1]['parameters']) == 1
    assert functions[1]['parameters'][0]['name'] == 'confirm'
    assert functions[1]['parameters'][0]['type'] == 'bool'


def test_parse_functions_description_with_numbered_list():
    """Test parsing functions description with numbered list format."""
    description = """
    1. create_sphere
    This function creates a sphere in the scene.
    Parameter: radius (float) - Radius of the sphere

    2. delete_objects
    This function deletes selected objects from the scene.
    Parameter: confirm (bool) - Confirm deletion
    """

    # Call the function
    functions = _parse_functions_description(description)

    # Verify the result
    assert len(functions) == 2
    assert functions[0]['name'] == 'create_sphere'
    assert functions[1]['name'] == 'delete_objects'


def test_parse_functions_description_with_different_parameter_formats():
    """Test parsing functions description with different parameter formats."""
    description = """
    Function: test_function
    This is a test function.
    param: int_param (int) - Integer parameter
    parameter: float_param (float) - Float parameter
    arg: str_param (string) - String parameter
    parameter: list_param (list) - List parameter
    parameter: dict_param (dictionary) - Dictionary parameter
    parameter: custom_param (custom type) - Custom parameter
    """

    # Call the function
    functions = _parse_functions_description(description)

    # Verify the result
    assert len(functions) == 1
    assert functions[0]['name'] == 'test_function'

    # Check parameters
    params = {p['name']: p for p in functions[0]['parameters']}
    assert len(params) == 6

    assert 'int_param' in params
    assert params['int_param']['type'] == 'int'

    assert 'float_param' in params
    assert params['float_param']['type'] == 'float'

    assert 'str_param' in params
    assert params['str_param']['type'] == 'str'

    assert 'list_param' in params
    assert params['list_param']['type'] == 'List[Any]'

    assert 'dict_param' in params
    assert params['dict_param']['type'] == 'Dict[str, Any]'

    assert 'custom_param' in params
    assert params['custom_param']['type'] == 'Any'


def test_parse_functions_description_empty():
    """Test parsing an empty functions description."""
    # Call the function with empty description
    functions = _parse_functions_description("")

    # Verify a default function is created
    assert len(functions) == 1
    assert functions[0]['name'] == 'execute_action'
    assert 'Execute the main action functionality' in functions[0]['description']
    assert len(functions[0]['parameters']) == 0
