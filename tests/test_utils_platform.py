"""Tests for the platform utilities module."""

import os
import pytest
from pathlib import Path

from dcc_mcp_core.utils.platform import (
    get_platform_dir,
    get_config_dir,
    get_data_dir,
    get_log_dir,
    get_actions_dir
)
from dcc_mcp_core.utils.constants import APP_NAME, APP_AUTHOR


def test_get_platform_dir():
    """Test the get_platform_dir function."""
    # Test with a valid directory type
    config_dir = get_platform_dir('config', ensure_exists=True)
    assert config_dir is not None
    
    # Test with an invalid directory type
    with pytest.raises(ValueError):
        get_platform_dir('invalid_type')


def test_get_config_dir():
    """Test the get_config_dir function."""
    config_dir = get_config_dir()
    assert config_dir is not None
    
    # If it's a Path object
    if isinstance(config_dir, Path):
        assert config_dir.exists()
    else:  # If it's a string
        assert os.path.exists(config_dir)


def test_get_data_dir():
    """Test the get_data_dir function."""
    data_dir = get_data_dir()
    assert data_dir is not None
    
    # If it's a Path object
    if isinstance(data_dir, Path):
        assert data_dir.exists()
    else:  # If it's a string
        assert os.path.exists(data_dir)


def test_get_log_dir():
    """Test the get_log_dir function."""
    log_dir = get_log_dir()
    assert log_dir is not None
    
    # If it's a Path object
    if isinstance(log_dir, Path):
        assert log_dir.exists()
    else:  # If it's a string
        assert os.path.exists(log_dir)


def test_get_actions_dir():
    """Test the get_actions_dir function."""
    # Test with a DCC name
    maya_actions_dir = get_actions_dir('maya')
    assert maya_actions_dir is not None
    
    # If it's a Path object
    if isinstance(maya_actions_dir, Path):
        assert maya_actions_dir.exists()
        assert 'maya' in str(maya_actions_dir)
    else:  # If it's a string
        assert os.path.exists(maya_actions_dir)
        assert 'maya' in maya_actions_dir
