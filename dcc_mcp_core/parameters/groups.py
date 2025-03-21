"""Parameter grouping and dependency management for DCC-MCP-Core.

This module provides utilities for defining and managing parameter groups and dependencies,
allowing for more structured and intuitive parameter handling in action functions.
"""

from typing import Any, Dict, List, Optional, Set, Tuple, Union, Callable
import inspect
import logging
from pydantic import BaseModel, Field, create_model

logger = logging.getLogger(__name__)


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
    
    def __init__(self, name: str, description: str, parameters: List[str], required: bool = False, exclusive: bool = False):
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
            return False, f"At least one parameter from group '{self.name}' is required: {', '.join(self.parameters)}"
        
        # If exclusive, only one parameter can be provided
        if self.exclusive and len(provided_in_group) > 1:
            return False, f"Only one parameter from group '{self.name}' can be provided: {', '.join(provided_in_group)}"
        
        return True, None


class ParameterDependency:
    """A dependency relationship between parameters.
    
    Parameter dependencies define constraints between parameters, such as:
    - Parameter A requires Parameter B
    - Parameter A conflicts with Parameter B
    - Parameter A's value determines whether Parameter B is required
    
    Attributes:
        parameter: The parameter that has a dependency
        depends_on: The parameter(s) that this parameter depends on
        condition: Optional function that evaluates whether the dependency is satisfied
        error_message: Custom error message to display when the dependency is violated
    """
    
    def __init__(self, 
                 parameter: str, 
                 depends_on: Union[str, List[str]], 
                 condition: Optional[Callable[[Dict[str, Any]], bool]] = None,
                 error_message: Optional[str] = None):
        """Initialize a parameter dependency.
        
        Args:
            parameter: The parameter that has a dependency
            depends_on: The parameter(s) that this parameter depends on
            condition: Optional function that evaluates whether the dependency is satisfied
            error_message: Custom error message to display when the dependency is violated
        """
        self.parameter = parameter
        self.depends_on = [depends_on] if isinstance(depends_on, str) else depends_on
        self.condition = condition
        self.error_message = error_message
    
    def validate(self, provided_params: Dict[str, Any]) -> Tuple[bool, Optional[str]]:
        """Validate parameters against this dependency's constraints.
        
        Args:
            provided_params: Dictionary of parameter names to values
            
        Returns:
            Tuple of (is_valid, error_message)
        """
        # If the parameter is not provided, no need to check dependencies
        if self.parameter not in provided_params:
            return True, None
        
        # Check if all dependencies are provided
        missing_deps = [dep for dep in self.depends_on if dep not in provided_params]
        if missing_deps:
            if self.error_message:
                return False, self.error_message
            else:
                return False, f"Parameter '{self.parameter}' requires {', '.join(missing_deps)}"
        
        # Check if the condition is satisfied
        if self.condition and not self.condition(provided_params):
            if self.error_message:
                return False, self.error_message
            else:
                return False, f"Condition for parameter '{self.parameter}' is not satisfied"
        
        return True, None


class ParameterManager:
    """Manager for parameter groups and dependencies.
    
    This class provides utilities for registering and validating parameter groups and dependencies.
    
    Attributes:
        groups: Dictionary of parameter groups by name
        dependencies: List of parameter dependencies
    """
    
    def __init__(self):
        """Initialize a parameter manager."""
        self.groups: Dict[str, ParameterGroup] = {}
        self.dependencies: List[ParameterDependency] = []
    
    def add_group(self, group: ParameterGroup):
        """Add a parameter group.
        
        Args:
            group: The parameter group to add
        """
        self.groups[group.name] = group
    
    def add_dependency(self, dependency: ParameterDependency):
        """Add a parameter dependency.
        
        Args:
            dependency: The parameter dependency to add
        """
        self.dependencies.append(dependency)
    
    def validate(self, provided_params: Dict[str, Any]) -> Tuple[bool, List[str]]:
        """Validate parameters against all registered groups and dependencies.
        
        Args:
            provided_params: Dictionary of parameter names to values
            
        Returns:
            Tuple of (is_valid, error_messages)
        """
        errors = []
        
        # Validate parameter groups
        for group in self.groups.values():
            is_valid, error = group.validate(provided_params)
            if not is_valid:
                errors.append(error)
        
        # Validate parameter dependencies
        for dependency in self.dependencies:
            is_valid, error = dependency.validate(provided_params)
            if not is_valid:
                errors.append(error)
        
        return len(errors) == 0, errors


# Decorator for defining parameter groups and dependencies
def with_parameter_groups(*groups: ParameterGroup, **dependencies: Any):
    """Decorator for defining parameter groups and dependencies for a function.
    
    This decorator allows for declarative definition of parameter groups and dependencies
    directly on the function that uses them.
    
    Args:
        *groups: Parameter groups for this function
        **dependencies: Keyword arguments defining parameter dependencies
            Format: param_name=(depends_on, condition, error_message)
            where condition and error_message are optional
    
    Returns:
        Decorated function with parameter groups and dependencies
    
    Example:
        @with_parameter_groups(
            ParameterGroup("size", "Size parameters", ["width", "height"], required=True),
            position=("context", lambda params: "width" in params, "Position requires context and width")
        )
        def create_rectangle(context, width=None, height=None, position=None):
            pass
    """
    def decorator(func):
        # Create parameter manager if it doesn't exist
        if not hasattr(func, "__parameter_manager__"):
            func.__parameter_manager__ = ParameterManager()
        
        # Add parameter groups
        for group in groups:
            func.__parameter_manager__.add_group(group)
        
        # Add parameter dependencies
        for param_name, dep_spec in dependencies.items():
            # Handle different formats of dependency specification
            if isinstance(dep_spec, tuple):
                # Unpack the tuple
                if len(dep_spec) == 1:
                    # (depends_on,)
                    depends_on, condition, error_message = dep_spec[0], None, None
                elif len(dep_spec) == 2:
                    # (depends_on, condition) or (depends_on, error_message)
                    depends_on, second = dep_spec
                    if callable(second):
                        condition, error_message = second, None
                    else:
                        condition, error_message = None, second
                elif len(dep_spec) >= 3:
                    # (depends_on, condition, error_message)
                    depends_on, condition, error_message = dep_spec[0], dep_spec[1], dep_spec[2]
                else:
                    # Empty tuple - invalid
                    raise ValueError(f"Invalid dependency specification for parameter '{param_name}': {dep_spec}")
            else:
                # Direct value is treated as depends_on
                depends_on, condition, error_message = dep_spec, None, None
            
            # Create and add the dependency
            dependency = ParameterDependency(param_name, depends_on, condition, error_message)
            func.__parameter_manager__.add_dependency(dependency)
        
        return func
    
    return decorator


def validate_parameters(func: Callable, args: Tuple[Any, ...], kwargs: Dict[str, Any]) -> Tuple[bool, List[str]]:
    """Validate parameters for a function with parameter groups and dependencies.
    
    Args:
        func: The function to validate parameters for
        args: Positional arguments passed to the function
        kwargs: Keyword arguments passed to the function
        
    Returns:
        Tuple of (is_valid, error_messages)
    """
    # If the function doesn't have a parameter manager, no validation needed
    if not hasattr(func, "__parameter_manager__"):
        return True, []
    
    # Get the parameter manager
    manager = func.__parameter_manager__
    
    # Get function signature
    sig = inspect.signature(func)
    param_names = list(sig.parameters.keys())
    
    # Skip 'self' parameter for methods
    if param_names and param_names[0] == 'self':
        param_names = param_names[1:]
        
    # Convert args to kwargs
    args_dict = {}
    for i, arg in enumerate(args):
        if i < len(param_names):
            args_dict[param_names[i]] = arg
    
    # Merge args and kwargs
    all_kwargs = {**args_dict, **kwargs}
    
    # Validate parameters using the manager
    return manager.validate(all_kwargs)
