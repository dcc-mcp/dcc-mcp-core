"""Tests for the logg_config module."""

import os
import sys
import logging
import tempfile
from pathlib import Path

import pytest
from loguru import logger

from dcc_mcp_core.logg_config import (
    setup_logging,
    setup_dcc_logging,
    setup_rpyc_logging,
    get_logger_info,
    set_log_level,
)


@pytest.fixture
def cleanup_loggers():
    """Clean up loggers after tests."""
    # Store original handlers
    original_handlers = {}
    for handler_id, handler in logger._core.handlers.items():
        original_handlers[handler_id] = handler
    
    yield
    
    # Remove any handlers added during tests
    for handler_id in list(logger._core.handlers.keys()):
        if handler_id not in original_handlers:
            logger.remove(handler_id)


def test_setup_logging(cleanup_loggers):
    """Test the setup_logging function."""
    # Set up a logger
    test_logger = setup_logging("test_logger")
    
    # Check that the logger was created
    assert test_logger is not None
    
    # Check that logger info was stored
    logger_info = get_logger_info("test_logger")
    assert logger_info is not None
    assert "log_file" in logger_info
    assert "console_handler" in logger_info
    assert "file_handler" in logger_info
    
    # Test logging - we'll just check that the log file exists
    # since redirecting stdout in tests can be problematic
    log_file = Path(logger_info["log_file"])
    assert log_file.exists()
    
    # Log a message to ensure file is written to
    test_logger.info("Test message")


def test_setup_dcc_logging(cleanup_loggers):
    """Test the setup_dcc_logging function."""
    # Create a standard Python logger to simulate a DCC logger
    dcc_logger = logging.getLogger("test_dcc")
    
    # Set up DCC logging
    test_logger = setup_dcc_logging("maya", dcc_logger)
    
    # Check that the logger was created
    assert test_logger is not None
    
    # Check that logger info was stored
    logger_info = get_logger_info("maya", "maya")
    assert logger_info is not None
    assert "log_file" in logger_info
    assert "dcc_type" in logger_info
    assert logger_info["dcc_type"] == "maya"
    
    # Check that the log file exists
    log_file = Path(logger_info["log_file"])
    assert log_file.exists()
    
    # Log a test message
    test_logger.info("DCC test message")
    dcc_logger.info("Python logger message")


def test_setup_rpyc_logging(cleanup_loggers):
    """Test the setup_rpyc_logging function."""
    # Set up RPyC logging
    rpyc_logger = setup_rpyc_logging()
    
    # Check that the logger was created
    assert rpyc_logger is not None
    
    # Get the standard Python RPyC logger
    python_rpyc_logger = logging.getLogger("rpyc")
    
    # Log a test message
    python_rpyc_logger.setLevel(logging.INFO)
    python_rpyc_logger.info("RPyC test message")
    rpyc_logger.info("Direct RPyC message")
    
    # We can't easily verify the output, but at least we can check
    # that no exceptions were raised


def test_set_log_level(cleanup_loggers):
    """Test the set_log_level function."""
    # Set up a logger
    test_logger = setup_logging("test_level_logger")
    
    # Set log level to INFO
    set_log_level("INFO")
    
    # Log messages at different levels
    test_logger.debug("Debug message")  # Should not be logged
    test_logger.info("Info message")    # Should be logged
    
    # Set log level to DEBUG
    set_log_level("DEBUG")
    
    # Log messages at different levels again
    test_logger.debug("Debug message 2")  # Should now be logged
    
    # We can't easily verify the output in a test, but we can check
    # that no exceptions were raised when changing log levels


def test_dcc_specific_logger(cleanup_loggers):
    """Test creating loggers for different DCC types."""
    # Set up loggers for different DCC types
    maya_logger = setup_logging("test_logger", "maya")
    houdini_logger = setup_logging("test_logger", "houdini")
    nuke_logger = setup_logging("test_logger", "nuke")
    
    # Check that the loggers were created
    assert maya_logger is not None
    assert houdini_logger is not None
    assert nuke_logger is not None
    
    # Check that logger info was stored correctly
    maya_info = get_logger_info("test_logger", "maya")
    houdini_info = get_logger_info("test_logger", "houdini")
    nuke_info = get_logger_info("test_logger", "nuke")
    
    assert maya_info["dcc_type"] == "maya"
    assert houdini_info["dcc_type"] == "houdini"
    assert nuke_info["dcc_type"] == "nuke"
    
    # Check that log files are in different directories
    maya_log_file = Path(maya_info["log_file"])
    houdini_log_file = Path(houdini_info["log_file"])
    nuke_log_file = Path(nuke_info["log_file"])
    
    assert "maya" in str(maya_log_file)
    assert "houdini" in str(houdini_log_file)
    assert "nuke" in str(nuke_log_file)
    
    # Test logging to each logger
    maya_logger.info("Maya test message")
    houdini_logger.info("Houdini test message")
    nuke_logger.info("Nuke test message")
    
    # Check that the log files exist
    assert maya_log_file.exists()
    assert houdini_log_file.exists()
    assert nuke_log_file.exists()
