#!/usr/bin/env python
"""ActionResultModel factory functions."""

# Import built-in modules
import traceback
from typing import Any
from typing import Dict
from typing import List
from typing import Optional

# Import local modules
# Import internal modules
from dcc_mcp_core.models import ActionResultModel
from dcc_mcp_core.utils.exceptions import ActionError
from dcc_mcp_core.utils.exceptions import ActionExecutionError
from dcc_mcp_core.utils.exceptions import ActionParameterError


def success_result(message: str, prompt: Optional[str] = None, **context) -> ActionResultModel:
    """Create an instance representing a successful execution.

    Args:
        message: Success message
        prompt: Suggestion for AI about next steps
        **context: Additional context data

    Returns:
        ActionResultModel instance

    """
    return ActionResultModel(success=True, message=message, prompt=prompt, context=context)


def error_result(
    message: str, error: str, prompt: Optional[str] = None, possible_solutions: Optional[List[str]] = None, **context
) -> ActionResultModel:
    """Create an instance representing a failed execution.

    Args:
        message: Error message
        error: Error details
        prompt: Suggestion for AI about next steps
        possible_solutions: List of possible solutions
        **context: Additional context data

    Returns:
        ActionResultModel instance

    """
    ctx = dict(context)
    if possible_solutions:
        ctx["possible_solutions"] = possible_solutions

    return ActionResultModel(success=False, message=message, prompt=prompt, error=error, context=ctx)


def from_exception(
    e: Exception,
    message: Optional[str] = None,
    prompt: Optional[str] = None,
    include_traceback: bool = True,
    possible_solutions: Optional[List[str]] = None,
    **context,
) -> ActionResultModel:
    """Create an instance from an exception.

    Args:
        e: Exception object
        message: Custom error message, if None, uses the exception's string representation
        prompt: Suggestion for AI about next steps
        include_traceback: Whether to include traceback
        possible_solutions: List of possible solutions to the error
        **context: Additional context data

    Returns:
        ActionResultModel instance

    """
    # Use exception classes imported at the module level

    ctx = dict(context)
    ctx["error_type"] = type(e).__name__

    # Extract additional information from specific exception types
    if isinstance(e, ActionError):
        # Add action information
        if e.action_name and "action_name" not in ctx:
            ctx["action_name"] = e.action_name
        if e.action_class and "action_class" not in ctx:
            ctx["action_class"] = e.action_class

        # Add specific error details based on exception type
        if isinstance(e, ActionParameterError):
            if e.parameter_name:
                ctx["parameter_name"] = e.parameter_name
            if e.parameter_value is not None:
                ctx["parameter_value"] = str(e.parameter_value)
            if e.validation_error:
                ctx["validation_error"] = e.validation_error

        elif isinstance(e, ActionExecutionError):
            if e.execution_phase:
                ctx["execution_phase"] = e.execution_phase
            if e.traceback and include_traceback:
                ctx["traceback"] = e.traceback

    # Add traceback if requested and not already added
    if include_traceback and "traceback" not in ctx:
        ctx["traceback"] = traceback.format_exc()

    # Add possible solutions if provided
    if possible_solutions:
        ctx["possible_solutions"] = possible_solutions

    # Generate appropriate prompt based on error type
    default_prompt = "Please check error details and retry"
    if isinstance(e, ActionParameterError):
        default_prompt = "Check parameters are correct, and retry the operation"
    elif isinstance(e, ActionExecutionError):
        default_prompt = "An error occurred during execution, please view detailed information and retry"

    return ActionResultModel(
        success=False, message=message or f"Error: {e!s}", prompt=prompt or default_prompt, error=str(e), context=ctx
    )


def ensure_dict_context(context: Any) -> Dict[str, Any]:
    """Ensure the context is a dictionary.

    Args:
        context: The context data

    Returns:
        Dictionary representation of the context data

    """
    if isinstance(context, dict):
        return context
    elif isinstance(context, (list, tuple, set)):
        return {"items": context}
    else:
        return {"value": context}


def validate_action_result(result: Any) -> ActionResultModel:
    """Validate and ensure the result is an ActionResultModel.

    Args:
        result: The result to validate

    Returns:
        ActionResultModel instance

    """
    # If the result is already an ActionResultModel, ensure the context is a dictionary
    if isinstance(result, ActionResultModel):
        if not isinstance(result.context, dict):
            return ActionResultModel(
                success=result.success,
                message=result.message,
                prompt=result.prompt,
                error=result.error,
                context=ensure_dict_context(result.context),
            )
        return result

    # If the result is a dictionary, ensure the context is a dictionary
    if isinstance(result, dict):
        try:
            # Ensure the context is a dictionary
            if "context" in result and not isinstance(result["context"], dict):
                result["context"] = ensure_dict_context(result["context"])
            return ActionResultModel(**result)
        except Exception as e:
            # If the conversion fails, return a new ActionResultModel with error information
            return ActionResultModel(
                success=False,
                message="Unable to convert result to ActionResultModel",
                error=str(e),
                context={"original_result": result},
            )

    # For other types, wrap the result in an ActionResultModel
    return ActionResultModel(success=True, message="Successfully processed result", context=ensure_dict_context(result))
