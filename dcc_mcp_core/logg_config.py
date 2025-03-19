"""Logging configuration for the DCC-MCP ecosystem.

This module provides a centralized logging configuration for all DCC-MCP components.
It supports integration with various DCC software's logging systems and uses loguru
for simplified and powerful logging capabilities.
"""

# Import built-in modules
import logging
import os
from pathlib import Path
import sys
from typing import Any
from typing import Dict
from typing import Optional

# Import third-party modules
from loguru import logger

# Import local modules
from dcc_mcp_core.constants import DEFAULT_LOG_LEVEL
from dcc_mcp_core.constants import ENV_LOG_LEVEL
from dcc_mcp_core.constants import LOG_APP_NAME
from dcc_mcp_core.platform_utils import get_log_dir

# Constants
APP_NAME = LOG_APP_NAME
LOG_LEVEL = os.getenv(ENV_LOG_LEVEL, DEFAULT_LOG_LEVEL)

# Get platform-specific log directory
LOG_DIR = Path(get_log_dir())
LOG_DIR.mkdir(parents=True, exist_ok=True)

# Define log format with colors and structure
LOG_FORMAT = (
    "<green>{time:YYYY-MM-DD HH:mm:ss.SSS}</green> | "
    "<level>{level: <8}</level> | "
    "<cyan>{name}</cyan>:<cyan>{function}</cyan>:<cyan>{line}</cyan> - "
    "<level>{message}</level>"
)

# Store DCC-specific loggers
_dcc_loggers: Dict[str, Any] = {}


def setup_logging(name: str = "dcc_mcp", dcc_type: Optional[str] = None) -> Any:
    """Configure logging with loguru.

    Args:
        name: Logger name for identification in logs
        dcc_type: Type of DCC software (e.g., 'maya', 'houdini', 'nuke')

    Returns:
        Configured loguru logger instance

    """
    # Create a logger identifier that includes the DCC type if provided
    logger_id = f"{dcc_type}_{name}" if dcc_type else name

    # Set up log file path
    log_file_dir = LOG_DIR
    if dcc_type:
        log_file_dir = log_file_dir / dcc_type
        log_file_dir.mkdir(parents=True, exist_ok=True)

    log_file = log_file_dir / f"{name}.log"

    # Configure logger with both console and file handlers in one call
    logger_config = {
        "handlers": [
            # Console handler
            {
                "sink": sys.stdout,
                "format": LOG_FORMAT,
                "level": LOG_LEVEL,
                "enqueue": True,
                "backtrace": True,
                "diagnose": True,
            },
            # File handler
            {
                "sink": str(log_file),
                "rotation": "10 MB",
                "retention": "1 week",
                "compression": "zip",
                "format": LOG_FORMAT,
                "level": LOG_LEVEL,
                "enqueue": True,
                "backtrace": True,
                "diagnose": True,
            }
        ],
    }

    # Apply configuration
    logger.configure(**logger_config)

    # Add handlers individually to get their IDs for the test compatibility
    console_handler_id = logger.add(
        sys.stdout,
        format=LOG_FORMAT,
        level=LOG_LEVEL,
        enqueue=True,
        backtrace=True,
        diagnose=True,
    )

    file_handler_id = logger.add(
        str(log_file),
        rotation="10 MB",
        retention="1 week",
        compression="zip",
        format=LOG_FORMAT,
        level=LOG_LEVEL,
        enqueue=True,
        backtrace=True,
        diagnose=True,
    )

    # Store the logger info with handler IDs for test compatibility
    _dcc_loggers[logger_id] = {
        "console_handler": console_handler_id,
        "file_handler": file_handler_id,
        "log_file": str(log_file),
        "dcc_type": dcc_type,
    }

    # Log startup information
    bound_logger = logger.bind(name=name)
    bound_logger.info(f"{logger_id} logging initialized")
    bound_logger.info(f"Log file: {log_file}")
    bound_logger.info(f"Log level: {LOG_LEVEL}")

    return bound_logger


def setup_dcc_logging(dcc_type: str, dcc_logger: Optional[Any] = None) -> Any:
    """Configure logging specifically for a DCC application.

    This function integrates with the DCC's existing logging system if provided,
    or creates a new logger if none is provided.

    Args:
        dcc_type: Type of DCC software (e.g., 'maya', 'houdini', 'nuke')
        dcc_logger: Existing logger from the DCC application (optional)

    Returns:
        Configured logger for the DCC

    """
    # Create a standard logger for this DCC type
    mcp_logger = setup_logging(dcc_type, dcc_type)

    # If no DCC logger is provided, just return our logger
    if dcc_logger is None:
        return mcp_logger

    # If DCC logger is a standard Python logger, integrate with it
    if isinstance(dcc_logger, logging.Logger):
        # Create a handler that forwards DCC logs to our loguru logger
        class DCCLogHandler(logging.Handler):
            def emit(self, record):
                # Get corresponding Loguru level if it exists
                try:
                    level = logger.level(record.levelname).name
                except ValueError:
                    level = record.levelno

                # Forward the log message to our loguru logger
                mcp_logger.opt(depth=0, exception=record.exc_info).log(
                    level, record.getMessage()
                )

        # Add our handler to DCC's logger
        handler = DCCLogHandler()
        dcc_logger.addHandler(handler)

    # Return our logger in all cases
    return mcp_logger


def get_logger_info(name: str, dcc_type: Optional[str] = None) -> Optional[Dict[str, Any]]:
    """Get information about a configured logger.

    Args:
        name: Logger name
        dcc_type: Type of DCC software

    Returns:
        Dictionary with logger information or None if logger not found

    """
    logger_id = f"{dcc_type}_{name}" if dcc_type else name
    return _dcc_loggers.get(logger_id)


def set_log_level(level: str) -> None:
    """Set the global log level for all loggers.

    Args:
        level: Log level to set (DEBUG, INFO, WARNING, ERROR, CRITICAL)

    """
    global LOG_LEVEL

    # Validate and normalize the log level
    level = level.upper()
    if level not in ["DEBUG", "INFO", "WARNING", "ERROR", "CRITICAL"]:
        logger.warning(f"Invalid log level: {level}, defaulting to INFO")
        level = "INFO"

    # Update the global log level
    LOG_LEVEL = level

    # Use loguru's built-in configure method to set the level for all handlers
    logger.configure(handlers=[{"sink": sys.stderr, "level": level}])

    # Log the change
    logger.info(f"Log level set to {level}")

    # Also set the level for the Python logging module
    logging.getLogger().setLevel(getattr(logging, level))


def setup_rpyc_logging() -> Any:
    """Configure RPyC-specific logging.

    RPyC uses the standard Python logging module, so we need to
    configure it to work with loguru.

    Returns:
        Configured loguru logger for RPyC

    """
    # Create a handler that uses loguru for the standard logging module
    class InterceptHandler(logging.Handler):
        def emit(self, record):
            # Get corresponding Loguru level if it exists
            try:
                level = logger.level(record.levelname).name
            except ValueError:
                level = record.levelno

            # Find caller from where originated the logged message
            frame, depth = logging.currentframe(), 2
            while frame.f_code.co_filename == logging.__file__:
                frame = frame.f_back
                depth += 1

            # Ensure the record has a name to avoid filtering issues
            if not hasattr(record, 'name') or record.name is None:
                record.name = "rpyc_unnamed"

            logger.opt(depth=depth, exception=record.exc_info).log(
                level, record.getMessage()
            )

    # Configure RPyC logger
    rpyc_logger = logging.getLogger("rpyc")
    rpyc_logger.setLevel(logging.DEBUG if LOG_LEVEL == "DEBUG" else logging.INFO)

    # Remove any existing handlers
    if rpyc_logger.handlers:
        for handler in rpyc_logger.handlers:
            rpyc_logger.removeHandler(handler)

    # Add our custom handler
    rpyc_logger.addHandler(InterceptHandler())

    # Create a specific loguru logger for RPyC
    rpyc_log_file = LOG_DIR / "rpyc.log"
    logger.add(
        str(rpyc_log_file),
        rotation="10 MB",
        retention="1 week",
        compression="zip",
        format=LOG_FORMAT,
        filter=lambda record: (
            record.get("name") is not None and
            ("rpyc" in record["name"] or record["name"] == "rpyc_unnamed")
        ),
        level=LOG_LEVEL,
        enqueue=True,
    )

    bound_logger = logger.bind(name="rpyc")
    bound_logger.info("RPyC logging initialized")
    bound_logger.info(f"RPyC log file: {rpyc_log_file}")

    return bound_logger
