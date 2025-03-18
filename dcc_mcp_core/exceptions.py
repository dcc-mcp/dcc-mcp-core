"""Exception classes for the DCC-MCP ecosystem.

This module defines a hierarchy of exceptions for different error conditions
in the DCC-MCP ecosystem. All exceptions inherit from the base MCPError class.
"""


class MCPError(Exception):
    """Base exception class for all DCC-MCP errors.

    All other exceptions in the DCC-MCP ecosystem should inherit from this class.
    This allows users to catch all DCC-MCP related exceptions with a single except clause.

    Attributes:
        code (str): A unique error code for machine-readable error identification.
        message (str): A human-readable error message.

    """

    def __init__(self, message, code=None):
        """Initialize a new MCPError instance.

        Args:
            message (str): A human-readable error message.
            code (str, optional): A unique error code for machine-readable error identification.
                Defaults to None.

        """
        self.message = message
        self.code = code or "MCP-E-GENERIC"
        super().__init__(self.message)

    def __str__(self):
        """Return a string representation of the error.

        Returns:
            str: A string representation of the error, including the error code if available.

        """
        if self.code:
            return f"[{self.code}] {self.message}"
        return self.message


class ValidationError(MCPError):
    """Exception raised when parameter validation fails.

    This exception is raised when a parameter fails validation, such as being
    of the wrong type, missing a required field, or having an invalid value.

    Attributes:
        param_name (str): The name of the parameter that failed validation.
        param_value: The value of the parameter that failed validation.
        expected: The expected type or value of the parameter.

    """

    def __init__(self, message, param_name=None, param_value=None, expected=None, code=None):
        """Initialize a new ValidationError instance.

        Args:
            message (str): A human-readable error message.
            param_name (str, optional): The name of the parameter that failed validation.
                Defaults to None.
            param_value (Any, optional): The value of the parameter that failed validation.
                Defaults to None.
            expected (Any, optional): The expected type or value of the parameter.
                Defaults to None.
            code (str, optional): A unique error code for machine-readable error identification.
                Defaults to None.

        """
        self.param_name = param_name
        self.param_value = param_value
        self.expected = expected
        super().__init__(message, code or "MCP-E-VALIDATION")


class ConfigurationError(MCPError):
    """Exception raised when there is an error in the configuration.

    This exception is raised when there is an error in the configuration,
    such as missing required configuration values or invalid configuration.

    Attributes:
        config_key (str): The key in the configuration that caused the error.

    """

    def __init__(self, message, config_key=None, code=None):
        """Initialize a new ConfigurationError instance.

        Args:
            message (str): A human-readable error message.
            config_key (str, optional): The key in the configuration that caused the error.
                Defaults to None.
            code (str, optional): A unique error code for machine-readable error identification.
                Defaults to None.

        """
        self.config_key = config_key
        super().__init__(message, code or "MCP-E-CONFIG")


class ConnectionError(MCPError):
    """Exception raised when there is an error connecting to a service.

    This exception is raised when there is an error connecting to a service,
    such as a DCC application or a remote server.

    Attributes:
        service_name (str): The name of the service that could not be connected to.

    """

    def __init__(self, message, service_name=None, code=None):
        """Initialize a new ConnectionError instance.

        Args:
            message (str): A human-readable error message.
            service_name (str, optional): The name of the service that could not be connected to.
                Defaults to None.
            code (str, optional): A unique error code for machine-readable error identification.
                Defaults to None.

        """
        self.service_name = service_name
        super().__init__(message, code or "MCP-E-CONNECTION")


class OperationError(MCPError):
    """Exception raised when an operation fails.

    This exception is raised when an operation fails, such as a file operation,
    a network operation, or a DCC operation.

    Attributes:
        operation_name (str): The name of the operation that failed.

    """

    def __init__(self, message, operation_name=None, code=None):
        """Initialize a new OperationError instance.

        Args:
            message (str): A human-readable error message.
            operation_name (str, optional): The name of the operation that failed.
                Defaults to None.
            code (str, optional): A unique error code for machine-readable error identification.
                Defaults to None.

        """
        self.operation_name = operation_name
        super().__init__(message, code or "MCP-E-OPERATION")


class VersionError(MCPError):
    """Exception raised when there is a version compatibility issue.

    This exception is raised when there is a version compatibility issue,
    such as an incompatible version of a dependency or a DCC application.

    Attributes:
        component (str): The component with the version issue.
        current_version (str): The current version of the component.
        required_version (str): The required version of the component.

    """

    def __init__(
        self, message, component=None, current_version=None, required_version=None, code=None
    ):
        """Initialize a new VersionError instance.

        Args:
            message (str): A human-readable error message.
            component (str, optional): The component with the version issue.
                Defaults to None.
            current_version (str, optional): The current version of the component.
                Defaults to None.
            required_version (str, optional): The required version of the component.
                Defaults to None.
            code (str, optional): A unique error code for machine-readable error identification.
                Defaults to None.

        """
        self.component = component
        self.current_version = current_version
        self.required_version = required_version
        super().__init__(message, code or "MCP-E-VERSION")
