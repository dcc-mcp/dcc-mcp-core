"""Parameter grouping and dependency management for DCC-MCP-Core.

This module provides utilities for defining and managing parameter groups and dependencies,
allowing for more structured and intuitive parameter handling in action functions.
"""

# Import built-in modules
from enum import Enum
import inspect
import logging
from typing import Any
from typing import Callable
from typing import Dict
from typing import List
from typing import Optional
from typing import Tuple
from typing import Union

# Import local modules
from dcc_mcp_core.models import ActionResultModel

logger = logging.getLogger(__name__)


class DependencyType(Enum):
    """Types of parameter dependencies."""

    REQUIRES = "requires"  # Parameter A requires Parameter B
    CONFLICTS = "conflicts"  # Parameter A conflicts with Parameter B
    ONE_OF = "one_of"  # Exactly one of the parameters must be provided
    AT_LEAST_ONE = "at_least_one"  # At least one of the parameters must be provided
    MUTUALLY_EXCLUSIVE = "mutually_exclusive"  # Parameters are mutually exclusive


class ParameterGroup:
    """A group of related parameters.

    Parameter groups allow for logical organization of parameters that are related to each other,
    making it easier for users and AI to understand parameter relationships.

    Attributes:
        name: Name of the parameter group
        description: Description of the parameter group
        parameters: List of parameter names in this group
        required: Whether at least one parameter in this group is required
        exclusive: Whether only one parameter in this group can be provided

    """

    def __init__(
        self, name: str, description: str, parameters: List[str], required: bool = False, exclusive: bool = False
    ):
        """Initialize a parameter group.

        Args:
            name: Name of the parameter group
            description: Description of the parameter group
            parameters: List of parameter names in this group
            required: Whether at least one parameter in this group is required
            exclusive: Whether only one parameter in this group can be provided

        """
        self.name = name
        self.description = description
        self.parameters = parameters
        self.required = required
        self.exclusive = exclusive

    def validate(self, provided_params: Dict[str, Any]) -> Tuple[bool, Optional[str]]:
        """Validate parameters against this group's constraints.

        Args:
            provided_params: Dictionary of parameter names to values

        Returns:
            Tuple of (is_valid, error_message)

        """
        # Check which parameters in this group are provided
        provided_in_group = [param for param in self.parameters if param in provided_params]

        # If required, at least one parameter must be provided
        if self.required and not provided_in_group:
            error_msg = f"At least one parameter from group '{self.name}' is required: {', '.join(self.parameters)}"
            logger.debug(f"Parameter group validation failed: {error_msg}")
            return False, error_msg

        # If exclusive, only one parameter can be provided
        if self.exclusive and len(provided_in_group) > 1:
            error_msg = f"Only one parameter from group '{self.name}' can be provided: {', '.join(provided_in_group)}"
            logger.debug(f"Parameter group validation failed: {error_msg}")
            return False, error_msg

        return True, None

    def to_dict(self) -> Dict[str, Any]:
        """Convert the parameter group to a dictionary for serialization.

        Returns:
            Dictionary representation of the parameter group

        """
        return {
            "name": self.name,
            "description": self.description,
            "parameters": self.parameters,
            "required": self.required,
            "exclusive": self.exclusive,
        }


class ParameterDependency:
    """A dependency relationship between parameters.

    Parameter dependencies define constraints between parameters, such as:
    - Parameter A requires Parameter B
    - Parameter A conflicts with Parameter B

    Attributes:
        parameter: The parameter that has a dependency
        depends_on: The parameter(s) that this parameter depends on
        dependency_type: The type of dependency relationship
        error_message: Custom error message to display when the dependency is violated

    """

    def __init__(
        self,
        parameter: str,
        depends_on: Union[str, List[str]],
        dependency_type: DependencyType = DependencyType.REQUIRES,
        error_message: Optional[str] = None,
    ):
        """Initialize a parameter dependency.

        Args:
            parameter: The parameter that has a dependency
            depends_on: The parameter(s) that this parameter depends on
            dependency_type: The type of dependency relationship
            error_message: Custom error message to display when the dependency is violated

        """
        self.parameter = parameter
        self.depends_on = [depends_on] if isinstance(depends_on, str) else depends_on
        self.dependency_type = dependency_type
        self.error_message = error_message

    def validate(self, provided_params: Dict[str, Any]) -> Tuple[bool, Optional[str]]:
        """Validate parameters against this dependency's constraints.

        Args:
            provided_params: Dictionary of parameter names to values

        Returns:
            Tuple of (is_valid, error_message)

        """
        if self.dependency_type == DependencyType.REQUIRES:
            return self._validate_requires(provided_params)
        elif self.dependency_type == DependencyType.CONFLICTS:
            return self._validate_conflicts(provided_params)
        elif self.dependency_type == DependencyType.ONE_OF:
            return self._validate_one_of(provided_params)
        elif self.dependency_type == DependencyType.AT_LEAST_ONE:
            return self._validate_at_least_one(provided_params)
        elif self.dependency_type == DependencyType.MUTUALLY_EXCLUSIVE:
            return self._validate_mutually_exclusive(provided_params)
        else:
            logger.warning(f"Unknown dependency type: {self.dependency_type}")
            return True, None

    def _validate_requires(self, provided_params: Dict[str, Any]) -> Tuple[bool, Optional[str]]:
        """Validate 'requires' dependency: if parameter is provided, all dependencies must be provided."""
        # If the parameter is not provided, no need to check dependencies
        if self.parameter not in provided_params:
            return True, None

        # Check if all dependencies are provided
        missing_deps = [dep for dep in self.depends_on if dep not in provided_params]
        if missing_deps:
            if self.error_message:
                error_msg = self.error_message
            else:
                error_msg = f"Parameter '{self.parameter}' requires {', '.join(missing_deps)}"
            logger.debug(f"Parameter dependency validation failed: {error_msg}")
            return False, error_msg

        return True, None

    def _validate_conflicts(self, provided_params: Dict[str, Any]) -> Tuple[bool, Optional[str]]:
        """Validate 'conflicts' dependency: if parameter is provided, none of the conflicts can be provided."""
        # If the parameter is not provided, no need to check conflicts
        if self.parameter not in provided_params:
            return True, None

        # Check if any of the conflicts are provided
        conflicts = [dep for dep in self.depends_on if dep in provided_params]
        if conflicts:
            if self.error_message:
                error_msg = self.error_message
            else:
                error_msg = f"Parameter '{self.parameter}' conflicts with {', '.join(conflicts)}"
            logger.debug(f"Parameter dependency validation failed: {error_msg}")
            return False, error_msg

        return True, None

    def _validate_one_of(self, provided_params: Dict[str, Any]) -> Tuple[bool, Optional[str]]:
        """Validate 'one_of' dependency: exactly one of the parameters must be provided."""
        # Check how many of the parameters are provided
        provided = [param for param in [self.parameter, *self.depends_on] if param in provided_params]

        if len(provided) != 1:
            if self.error_message:
                error_msg = self.error_message
            else:
                all_params = [self.parameter, *self.depends_on]
                error_msg = f"Exactly one of these parameters must be provided: {', '.join(all_params)}"
            logger.debug(f"Parameter dependency validation failed: {error_msg}")
            return False, error_msg

        return True, None

    def _validate_at_least_one(self, provided_params: Dict[str, Any]) -> Tuple[bool, Optional[str]]:
        """Validate 'at_least_one' dependency: at least one of the parameters must be provided."""
        # Check if any of the parameters are provided
        provided = [param for param in [self.parameter, *self.depends_on] if param in provided_params]

        if not provided:
            if self.error_message:
                error_msg = self.error_message
            else:
                all_params = [self.parameter, *self.depends_on]
                error_msg = f"At least one of these parameters must be provided: {', '.join(all_params)}"
            logger.debug(f"Parameter dependency validation failed: {error_msg}")
            return False, error_msg

        return True, None

    def _validate_mutually_exclusive(self, provided_params: Dict[str, Any]) -> Tuple[bool, Optional[str]]:
        """Validate 'mutually_exclusive' dependency: at most one of the parameters can be provided."""
        # Check how many of the parameters are provided
        provided = [param for param in [self.parameter, *self.depends_on] if param in provided_params]

        if len(provided) > 1:
            if self.error_message:
                error_msg = self.error_message
            else:
                error_msg = f"These parameters are mutually exclusive: {', '.join(provided)}"
            logger.debug(f"Parameter dependency validation failed: {error_msg}")
            return False, error_msg

        return True, None

    def to_dict(self) -> Dict[str, Any]:
        """Convert the parameter dependency to a dictionary for serialization.

        Returns:
            Dictionary representation of the parameter dependency

        """
        return {
            "parameter": self.parameter,
            "depends_on": self.depends_on,
            "dependency_type": self.dependency_type.value,
            "error_message": self.error_message,
        }


# Decorators for defining parameter groups and dependencies
def with_parameter_groups(*groups: ParameterGroup):
    """Define parameter groups for a function.

    This decorator allows for declarative definition of parameter groups
    directly on the function that uses them.

    Args:
        *groups: Parameter groups for this function

    Returns:
        Decorated function with parameter groups

    """

    def decorator(func):
        # Store parameter groups on the function
        func.__parameter_groups__ = groups

        return func

    return decorator


def with_parameter_dependencies(*dependencies: ParameterDependency):
    """Define parameter dependencies for a function.

    This decorator allows for declarative definition of parameter dependencies
    directly on the function that uses them.

    Args:
        *dependencies: Parameter dependencies for this function

    Returns:
        Decorated function with parameter dependencies

    """

    def decorator(func):
        # Store parameter dependencies on the function
        func.__parameter_dependencies__ = dependencies

        return func

    return decorator


def validate_parameter_constraints(
    func: Callable, args: Tuple[Any, ...], kwargs: Dict[str, Any]
) -> Tuple[bool, List[str]]:
    """Validate parameters for a function with parameter groups and dependencies.

    Args:
        func: The function to validate parameters for
        args: Positional arguments passed to the function
        kwargs: Keyword arguments passed to the function

    Returns:
        Tuple of (is_valid, error_messages)

    """
    # If the function doesn't have parameter constraints, validation passes
    has_groups = hasattr(func, "__parameter_groups__")
    has_dependencies = hasattr(func, "__parameter_dependencies__")

    if not has_groups and not has_dependencies:
        return True, []

    # Get function signature
    sig = inspect.signature(func)
    param_names = list(sig.parameters.keys())

    # Skip 'self' parameter for methods
    if param_names and param_names[0] == "self":
        param_names = param_names[1:]

    # Convert args to kwargs
    args_dict = {}
    for i, arg in enumerate(args):
        if i < len(param_names):
            args_dict[param_names[i]] = arg

    # Merge args and kwargs
    all_kwargs = {**args_dict, **kwargs}

    # Validate parameter groups
    errors = []

    if has_groups:
        groups = func.__parameter_groups__
        for group in groups:
            is_valid, error = group.validate(all_kwargs)
            if not is_valid and error:
                errors.append(error)

    # Validate parameter dependencies
    if has_dependencies:
        dependencies = func.__parameter_dependencies__
        for dependency in dependencies:
            is_valid, error = dependency.validate(all_kwargs)
            if not is_valid and error:
                errors.append(error)

    return len(errors) == 0, errors


def with_parameter_validation(func):
    """Add parameter validation to a function.

    This decorator validates parameters against defined groups and dependencies
    before calling the function.

    Args:
        func: The function to add parameter validation to

    Returns:
        Decorated function with parameter validation

    """

    def wrapper(*args, **kwargs):
        # Validate parameters
        is_valid, errors = validate_parameter_constraints(func, args, kwargs)

        if not is_valid:
            # Return a structured error response
            return ActionResultModel(
                success=False,
                message="Parameter validation failed",
                error="\n".join(errors),
                prompt="Please check the parameter values and try again.",
                context={"validation_errors": errors},
            )

        # Call the function with the original parameters
        return func(*args, **kwargs)

    # Copy function metadata
    wrapper.__name__ = func.__name__
    wrapper.__doc__ = func.__doc__
    wrapper.__module__ = func.__module__
    wrapper.__qualname__ = func.__qualname__
    wrapper.__annotations__ = func.__annotations__

    return wrapper
