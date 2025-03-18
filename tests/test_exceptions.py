"""Tests for the exceptions module."""


# Import local modules
from dcc_mcp_core import exceptions


def test_mcp_error():
    """Test the base MCPError class."""
    # Test with default code
    error = exceptions.MCPError("An error occurred")
    assert error.message == "An error occurred"
    assert error.code == "MCP-E-GENERIC"
    assert str(error) == "[MCP-E-GENERIC] An error occurred"

    # Test with custom code
    error = exceptions.MCPError("An error occurred", code="MCP-E-CUSTOM")
    assert error.message == "An error occurred"
    assert error.code == "MCP-E-CUSTOM"
    assert str(error) == "[MCP-E-CUSTOM] An error occurred"


def test_validation_error():
    """Test the ValidationError class."""
    # Test with default values
    error = exceptions.ValidationError("Validation failed")
    assert error.message == "Validation failed"
    assert error.code == "MCP-E-VALIDATION"
    assert error.param_name is None
    assert error.param_value is None
    assert error.expected is None
    assert str(error) == "[MCP-E-VALIDATION] Validation failed"

    # Test with custom values
    error = exceptions.ValidationError(
        "Validation failed",
        param_name="age",
        param_value="thirty",
        expected=int,
        code="MCP-E-CUSTOM-VALIDATION",
    )
    assert error.message == "Validation failed"
    assert error.code == "MCP-E-CUSTOM-VALIDATION"
    assert error.param_name == "age"
    assert error.param_value == "thirty"
    assert error.expected is int
    assert str(error) == "[MCP-E-CUSTOM-VALIDATION] Validation failed"


def test_configuration_error():
    """Test the ConfigurationError class."""
    # Test with default values
    error = exceptions.ConfigurationError("Configuration error")
    assert error.message == "Configuration error"
    assert error.code == "MCP-E-CONFIG"
    assert error.config_key is None
    assert str(error) == "[MCP-E-CONFIG] Configuration error"

    # Test with custom values
    error = exceptions.ConfigurationError(
        "Configuration error", config_key="api_key", code="MCP-E-CUSTOM-CONFIG"
    )
    assert error.message == "Configuration error"
    assert error.code == "MCP-E-CUSTOM-CONFIG"
    assert error.config_key == "api_key"
    assert str(error) == "[MCP-E-CUSTOM-CONFIG] Configuration error"


def test_connection_error():
    """Test the ConnectionError class."""
    # Test with default values
    error = exceptions.ConnectionError("Connection error")
    assert error.message == "Connection error"
    assert error.code == "MCP-E-CONNECTION"
    assert error.service_name is None
    assert str(error) == "[MCP-E-CONNECTION] Connection error"

    # Test with custom values
    error = exceptions.ConnectionError(
        "Connection error", service_name="Maya", code="MCP-E-CUSTOM-CONNECTION"
    )
    assert error.message == "Connection error"
    assert error.code == "MCP-E-CUSTOM-CONNECTION"
    assert error.service_name == "Maya"
    assert str(error) == "[MCP-E-CUSTOM-CONNECTION] Connection error"


def test_operation_error():
    """Test the OperationError class."""
    # Test with default values
    error = exceptions.OperationError("Operation error")
    assert error.message == "Operation error"
    assert error.code == "MCP-E-OPERATION"
    assert error.operation_name is None
    assert str(error) == "[MCP-E-OPERATION] Operation error"

    # Test with custom values
    error = exceptions.OperationError(
        "Operation error", operation_name="file_copy", code="MCP-E-CUSTOM-OPERATION"
    )
    assert error.message == "Operation error"
    assert error.code == "MCP-E-CUSTOM-OPERATION"
    assert error.operation_name == "file_copy"
    assert str(error) == "[MCP-E-CUSTOM-OPERATION] Operation error"


def test_version_error():
    """Test the VersionError class."""
    # Test with default values
    error = exceptions.VersionError("Version error")
    assert error.message == "Version error"
    assert error.code == "MCP-E-VERSION"
    assert error.component is None
    assert error.current_version is None
    assert error.required_version is None
    assert str(error) == "[MCP-E-VERSION] Version error"

    # Test with custom values
    error = exceptions.VersionError(
        "Version error",
        component="Maya",
        current_version="2022",
        required_version="2023",
        code="MCP-E-CUSTOM-VERSION",
    )
    assert error.message == "Version error"
    assert error.code == "MCP-E-CUSTOM-VERSION"
    assert error.component == "Maya"
    assert error.current_version == "2022"
    assert error.required_version == "2023"
    assert str(error) == "[MCP-E-CUSTOM-VERSION] Version error"


def test_exception_inheritance():
    """Test that all exceptions inherit from MCPError."""
    assert issubclass(exceptions.ValidationError, exceptions.MCPError)
    assert issubclass(exceptions.ConfigurationError, exceptions.MCPError)
    assert issubclass(exceptions.ConnectionError, exceptions.MCPError)
    assert issubclass(exceptions.OperationError, exceptions.MCPError)
    assert issubclass(exceptions.VersionError, exceptions.MCPError)
