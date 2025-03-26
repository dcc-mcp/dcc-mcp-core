"""Logging configuration for the DCC-MCP ecosystem.

This module provides a centralized logging configuration for all DCC-MCP components.
It supports integration with various DCC software's logging systems and offers both
standard Python logging and optional loguru-based enhanced logging.
"""

# Import built-in modules
import logging
import os
from pathlib import Path
import sys
from typing import Any
from typing import Dict
from typing import Optional
from typing import Union

# Import local modules
from dcc_mcp_core.constants import DEFAULT_LOG_LEVEL
from dcc_mcp_core.constants import ENV_LOG_LEVEL
from dcc_mcp_core.utils.filesystem import get_log_dir

# Constants
LOG_LEVEL = os.getenv(ENV_LOG_LEVEL, DEFAULT_LOG_LEVEL)

# Get platform-specific log directory
LOG_DIR = Path(get_log_dir())
LOG_DIR.mkdir(parents=True, exist_ok=True)

# Store configured loggers for reference
_configured_loggers: Dict[str, Any] = {}

# Check if loguru is available (optional dependency)
try:
    # Import third-party modules
    from loguru import logger as loguru_logger

    LOGURU_AVAILABLE = True
except ImportError:
    LOGURU_AVAILABLE = False

# Default log format
DEFAULT_FORMAT = "%(asctime)s | %(levelname)-8s | %(name)s:%(funcName)s:%(lineno)d - %(message)s"

# Default loguru format (only used if loguru is available)
LOGURU_FORMAT = (
    "<green>{time:YYYY-MM-DD HH:mm:ss.SSS}</green> | "
    "<level>{level: <8}</level> | "
    "<cyan>{name}</cyan>:<cyan>{function}</cyan>:<cyan>{line}</cyan> - "
    "<level>{message}</level>"
)


def get_logger(name: str, dcc_type: Optional[str] = None, use_loguru: bool = False) -> Union[logging.Logger, Any]:
    """Get a configured logger.

    This is the main entry point for getting a logger. It will return either a standard
    Python logger or a loguru logger based on the use_loguru parameter.

    Args:
        name: Logger name for identification in logs
        dcc_type: Type of DCC software (e.g., 'maya', 'houdini', 'nuke')
        use_loguru: Whether to use loguru for enhanced logging (if available)

    Returns:
        Configured logger instance

    """
    # Create a logger identifier that includes the DCC type if provided
    logger_id = f"{dcc_type}_{name}" if dcc_type else name

    # Check if this logger is already configured
    if logger_id in _configured_loggers:
        return _configured_loggers[logger_id]["logger"]

    # Use loguru if requested and available
    if use_loguru and LOGURU_AVAILABLE:
        return setup_loguru_logger(name, dcc_type)

    # Otherwise use standard Python logging
    return setup_standard_logger(name, dcc_type)


def setup_standard_logger(name: str, dcc_type: Optional[str] = None) -> logging.Logger:
    """Configure a standard Python logger.

    Args:
        name: Logger name for identification in logs
        dcc_type: Type of DCC software (e.g., 'maya', 'houdini', 'nuke')

    Returns:
        Configured standard Python logger

    """
    # Create a logger identifier that includes the DCC type if provided
    logger_id = f"{dcc_type}_{name}" if dcc_type else name

    # Set up log file path
    log_file_dir = LOG_DIR
    if dcc_type:
        log_file_dir = log_file_dir / dcc_type
        log_file_dir.mkdir(parents=True, exist_ok=True)

    log_file = log_file_dir / f"{name}.log"

    # Get or create logger
    logger = logging.getLogger(logger_id)

    # Only configure if not already configured
    if not logger.handlers:
        logger.setLevel(getattr(logging, LOG_LEVEL))

        # Create console handler
        console_handler = logging.StreamHandler(sys.stdout)
        console_handler.setLevel(getattr(logging, LOG_LEVEL))
        console_formatter = logging.Formatter(DEFAULT_FORMAT)
        console_handler.setFormatter(console_formatter)

        # Create file handler
        file_handler = logging.FileHandler(str(log_file))
        file_handler.setLevel(getattr(logging, LOG_LEVEL))
        file_formatter = logging.Formatter(DEFAULT_FORMAT)
        file_handler.setFormatter(file_formatter)

        # Add handlers to logger
        logger.addHandler(console_handler)
        logger.addHandler(file_handler)

        # Store logger configuration
        _configured_loggers[logger_id] = {
            "logger": logger,
            "log_file": str(log_file),
            "dcc_type": dcc_type,
            "handlers": [console_handler, file_handler],
            "type": "standard",
        }

        # Log startup information
        logger.info(f"{logger_id} logging initialized")
        logger.info(f"Log file: {log_file}")
        logger.info(f"Log level: {LOG_LEVEL}")

    return logger


def setup_loguru_logger(name: str, dcc_type: Optional[str] = None) -> Any:
    """Configure a loguru logger.

    Args:
        name: Logger name for identification in logs
        dcc_type: Type of DCC software (e.g., 'maya', 'houdini', 'nuke')

    Returns:
        Configured loguru logger instance

    """
    if not LOGURU_AVAILABLE:
        return setup_standard_logger(name, dcc_type)

    # Create a logger identifier that includes the DCC type if provided
    logger_id = f"{dcc_type}_{name}" if dcc_type else name

    # Set up log file path
    log_file_dir = LOG_DIR
    if dcc_type:
        log_file_dir = log_file_dir / dcc_type
        log_file_dir.mkdir(parents=True, exist_ok=True)

    log_file = log_file_dir / f"{name}.log"

    # Configure handlers
    console_handler = {
        "sink": sys.stdout,
        "format": LOGURU_FORMAT,
        "level": LOG_LEVEL,
        "enqueue": True,
    }

    file_handler = {
        "sink": str(log_file),
        "rotation": "10 MB",
        "retention": "1 week",
        "compression": "zip",
        "format": LOGURU_FORMAT,
        "level": LOG_LEVEL,
        "enqueue": True,
    }

    # Create a new logger instance
    logger = loguru_logger.bind(name=name)

    # Add handlers
    console_id = logger.configure(handlers=[console_handler])[0]
    file_id = logger.add(**file_handler)

    # Store logger configuration
    _configured_loggers[logger_id] = {
        "logger": logger,
        "log_file": str(log_file),
        "dcc_type": dcc_type,
        "handlers": [console_id, file_id],
        "type": "loguru",
    }

    # Log startup information
    logger.info(f"{logger_id} logging initialized")
    logger.info(f"Log file: {log_file}")
    logger.info(f"Log level: {LOG_LEVEL}")

    return logger


def integrate_with_dcc_logger(dcc_logger: Any, name: str, dcc_type: str) -> Union[logging.Logger, Any]:
    """Provide a basic interface for integrating with DCC loggers.

    This function provides a minimal interface for integrating with DCC logging systems.
    It does not implement any DCC-specific logic, as that should be handled by the
    specific DCC adapter packages.

    Args:
        dcc_logger: Existing logger from the DCC application
        name: Logger name for identification in logs
        dcc_type: Type of DCC software (e.g., 'maya', 'houdini', 'nuke')

    Returns:
        Configured logger that can be used with the DCC

    """
    # Get our logger (standard or loguru based on availability)
    use_loguru = LOGURU_AVAILABLE
    our_logger = get_logger(name, dcc_type, use_loguru=use_loguru)

    # If no DCC logger is provided, just return our logger
    if dcc_logger is None:
        return our_logger

    # If DCC logger is a standard Python logger, we can provide basic integration
    if isinstance(dcc_logger, logging.Logger):
        # Add our handlers to the DCC logger
        logger_info = _configured_loggers.get(f"{dcc_type}_{name}")
        if logger_info and logger_info["type"] == "standard":
            for handler in logger_info["handlers"]:
                if handler not in dcc_logger.handlers:
                    dcc_logger.addHandler(handler)

        # Also forward DCC logs to our logger if using loguru
        if logger_info and logger_info["type"] == "loguru":

            class LoguruHandler(logging.Handler):
                def emit(self, record):
                    # Get corresponding level
                    try:
                        level = loguru_logger.level(record.levelname).name
                    except (ValueError, AttributeError):
                        level = record.levelno

                    # Forward to loguru
                    loguru_logger.opt(depth=0, exception=record.exc_info).log(level, record.getMessage())

            # Add our handler to DCC's logger
            dcc_logger.addHandler(LoguruHandler())

    # For non-standard loggers, we don't provide specific integrations
    # This should be handled by specific DCC adapter packages

    # Return our logger as the default behavior
    return our_logger


def get_logger_info(name: str, dcc_type: Optional[str] = None) -> Dict[str, Any]:
    """Get information about a configured logger.

    Args:
        name: Logger name for identification in logs
        dcc_type: Type of DCC software (e.g., 'maya', 'houdini', 'nuke')

    Returns:
        Dictionary containing information about the logger

    """
    # Create a logger identifier that includes the DCC type if provided
    logger_key = f"{dcc_type}_{name}" if dcc_type else name

    if logger_key not in _configured_loggers:
        # If the logger is not configured, return basic information
        return {
            "name": name,
            "dcc_type": dcc_type,
            "configured": False,
            "log_file": None,
            "level": LOG_LEVEL,
            "console_handler": None,
            "file_handler": None,
        }

    logger_info = _configured_loggers[logger_key]

    # Determine log file path and handlers
    log_file = None
    console_handler = None
    file_handler = None

    if "handlers" in logger_info:
        for handler in logger_info["handlers"]:
            if hasattr(handler, "baseFilename"):
                log_file = handler.baseFilename
                file_handler = handler
            elif isinstance(handler, logging.StreamHandler) and handler.stream == sys.stdout:
                console_handler = handler

    return {
        "name": name,
        "dcc_type": dcc_type,
        "configured": True,
        "log_file": log_file,
        "level": logger_info.get("level", LOG_LEVEL),
        "handlers": logger_info.get("handlers", []),
        "console_handler": console_handler,
        "file_handler": file_handler,
    }


def set_log_level(level: str) -> None:
    """Set the global log level for all loggers.

    Args:
        level: Log level to set (DEBUG, INFO, WARNING, ERROR, CRITICAL)

    """
    global LOG_LEVEL

    # Validate and normalize the log level
    level = level.upper()
    if level not in ["DEBUG", "INFO", "WARNING", "ERROR", "CRITICAL"]:
        level = "INFO"
        if LOGURU_AVAILABLE:
            loguru_logger.warning(f"Invalid log level: {level}, defaulting to INFO")
        else:
            logging.warning(f"Invalid log level: {level}, defaulting to INFO")

    # Update the global log level
    LOG_LEVEL = level

    # Update all configured loggers
    for logger_id, logger_info in _configured_loggers.items():
        if logger_info["type"] == "standard":
            logger_info["logger"].setLevel(getattr(logging, level))
            for handler in logger_info["handlers"]:
                handler.setLevel(getattr(logging, level))
        elif logger_info["type"] == "loguru" and LOGURU_AVAILABLE:
            # Loguru handles level changes differently
            pass  # Level changes are handled at the sink level in loguru

    # Also set the level for the root Python logger
    logging.getLogger().setLevel(getattr(logging, level))

    # Log the change
    if LOGURU_AVAILABLE:
        loguru_logger.info(f"Log level set to {level}")
    else:
        logging.info(f"Log level set to {level}")


# Backwards compatibility functions
def setup_logging(name: str = "dcc_mcp", dcc_type: Optional[str] = None) -> Any:
    """Legacy function for backwards compatibility.

    Args:
        name: Logger name for identification in logs
        dcc_type: Type of DCC software (e.g., 'maya', 'houdini', 'nuke')

    Returns:
        Configured logger instance

    """
    use_loguru = LOGURU_AVAILABLE
    return get_logger(name, dcc_type, use_loguru=use_loguru)


def setup_dcc_logging(dcc_type: str, dcc_logger: Optional[Any] = None) -> Any:
    """Legacy function for backwards compatibility.

    Args:
        dcc_type: Type of DCC software (e.g., 'maya', 'houdini', 'nuke')
        dcc_logger: Existing logger from the DCC application (optional)

    Returns:
        Configured logger for the DCC

    """
    return integrate_with_dcc_logger(dcc_logger, dcc_type, dcc_type)


def setup_rpyc_logging() -> Any:
    """Configure RPyC-specific logging.

    RPyC uses the standard Python logging module, so we integrate with it
    using our standard approach.

    Returns:
        Configured logger for RPyC

    """
    # Get the RPyC logger
    rpyc_logger = logging.getLogger("rpyc")

    # Integrate with our logging system
    return integrate_with_dcc_logger(rpyc_logger, "rpyc", "rpyc")
