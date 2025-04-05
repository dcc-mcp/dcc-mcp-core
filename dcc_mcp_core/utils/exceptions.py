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

    def __init__(self, message, component=None, current_version=None, required_version=None, code=None):
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


class ParameterValidationError(ValidationError):
    """Exception raised when parameter validation fails."""


class ActionError(MCPError):
    """Base exception class for all Action-related errors.

    This is the base class for all exceptions related to the Action system.
    It provides additional context about the action that caused the error.

    Attributes:
        action_name (str): The name of the action that caused the error.
        action_class (str): The class name of the action that caused the error.

    """

    def __init__(self, message, action_name=None, action_class=None, code=None):
        """Initialize a new ActionError instance.

        Args:
            message (str): A human-readable error message.
            action_name (str, optional): The name of the action that caused the error.
                Defaults to None.
            action_class (str, optional): The class name of the action that caused the error.
                Defaults to None.
            code (str, optional): A unique error code for machine-readable error identification.
                Defaults to None.

        """
        self.action_name = action_name
        self.action_class = action_class
        super().__init__(message, code or "MCP-E-ACTION")

    def __str__(self):
        """Return a string representation of the error.

        Returns:
            str: A string representation of the error, including the action name and class if available.

        """
        parts = []
        if self.code:
            parts.append(f"[{self.code}]")
        if self.action_name:
            parts.append(f"Action '{self.action_name}':")
        elif self.action_class:
            parts.append(f"Action class '{self.action_class}':")
        parts.append(self.message)
        return " ".join(parts)


class ActionExecutionError(ActionError):
    """Exception raised when an action execution fails.

    This exception is raised when the execution of an action fails,
    such as a DCC operation failing or an unexpected error occurring.

    Attributes:
        execution_phase (str): The phase of execution where the error occurred.
        traceback (str): The traceback of the original exception, if available.

    """

    def __init__(self, message, action_name=None, action_class=None, execution_phase=None, traceback=None, code=None):
        """Initialize a new ActionExecutionError instance.

        Args:
            message (str): A human-readable error message.
            action_name (str, optional): The name of the action that caused the error.
                Defaults to None.
            action_class (str, optional): The class name of the action that caused the error.
                Defaults to None.
            execution_phase (str, optional): The phase of execution where the error occurred.
                Defaults to None.
            traceback (str, optional): The traceback of the original exception, if available.
                Defaults to None.
            code (str, optional): A unique error code for machine-readable error identification.
                Defaults to None.

        """
        self.execution_phase = execution_phase
        self.traceback = traceback
        super().__init__(message, action_name, action_class, code or "MCP-E-ACTION-EXECUTION")


class ActionSetupError(ActionError):
    """Exception raised when action setup fails.

    This exception is raised when the setup of an action fails,
    such as missing required context or dependencies.

    Attributes:
        missing_dependencies (list): A list of missing dependencies.

    """

    def __init__(self, message, action_name=None, action_class=None, missing_dependencies=None, code=None):
        """Initialize a new ActionSetupError instance.

        Args:
            message (str): A human-readable error message.
            action_name (str, optional): The name of the action that caused the error.
                Defaults to None.
            action_class (str, optional): The class name of the action that caused the error.
                Defaults to None.
            missing_dependencies (list, optional): A list of missing dependencies.
                Defaults to None.
            code (str, optional): A unique error code for machine-readable error identification.
                Defaults to None.

        """
        self.missing_dependencies = missing_dependencies or []
        super().__init__(message, action_name, action_class, code or "MCP-E-ACTION-SETUP")


class ActionParameterError(ActionError, ValidationError):
    """Exception raised when action parameter validation fails.

    This exception is raised when a parameter for an action fails validation,
    such as being of the wrong type, missing a required field, or having an invalid value.

    Attributes:
        parameter_name (str): The name of the parameter that failed validation.
        parameter_value (any): The value of the parameter that failed validation.
        validation_error (str): The specific validation error message.

    """

    def __init__(
        self,
        message,
        action_name=None,
        action_class=None,
        parameter_name=None,
        parameter_value=None,
        validation_error=None,
        code=None,
    ):
        """Initialize a new ActionParameterError instance.

        Args:
            message (str): A human-readable error message.
            action_name (str, optional): The name of the action that caused the error.
                Defaults to None.
            action_class (str, optional): The class name of the action that caused the error.
                Defaults to None.
            parameter_name (str, optional): The name of the parameter that failed validation.
                Defaults to None.
            parameter_value (any, optional): The value of the parameter that failed validation.
                Defaults to None.
            validation_error (str, optional): The specific validation error message.
                Defaults to None.
            code (str, optional): A unique error code for machine-readable error identification.
                Defaults to None.

        """
        self.parameter_name = parameter_name
        self.parameter_value = parameter_value
        self.validation_error = validation_error
        ActionError.__init__(self, message, action_name, action_class, code or "MCP-E-ACTION-PARAMETER")


class ActionValidationError(ActionError):
    """Exception raised when action validation fails.

    This exception is raised when the validation of an action fails,
    such as failing to meet preconditions or having invalid state.

    Attributes:
        validation_errors (dict): A dictionary of validation errors.

    """

    def __init__(self, message, action_name=None, action_class=None, validation_errors=None, code=None):
        """Initialize a new ActionValidationError instance.

        Args:
            message (str): A human-readable error message.
            action_name (str, optional): The name of the action that caused the error.
                Defaults to None.
            action_class (str, optional): The class name of the action that caused the error.
                Defaults to None.
            validation_errors (dict, optional): A dictionary of validation errors.
                Defaults to None.
            code (str, optional): A unique error code for machine-readable error identification.
                Defaults to None.

        """
        self.validation_errors = validation_errors or {}
        super().__init__(message, action_name, action_class, code or "MCP-E-ACTION-VALIDATION")
