"""Tests for the middleware system in DCC-MCP-Core.

This module contains tests for the Middleware and MiddlewareChain classes.
"""

# Import built-in modules
from unittest.mock import MagicMock
from unittest.mock import patch

# Import local modules
from dcc_mcp_core.actions.base import Action
from dcc_mcp_core.actions.middleware import LoggingMiddleware
from dcc_mcp_core.actions.middleware import Middleware
from dcc_mcp_core.actions.middleware import MiddlewareChain
from dcc_mcp_core.actions.middleware import PerformanceMiddleware
from dcc_mcp_core.models import ActionResultModel


# Create a mock Action class for testing
class MockAction(Action):
    """Mock Action class for testing."""

    name = "mock_action"

    def process(self) -> ActionResultModel:
        """Process the action."""
        return ActionResultModel(success=True, message="Successfully executed mock_action")


def test_middleware_init():
    """Test Middleware initialization."""
    # Create a new Middleware instance
    middleware = Middleware()

    # Check that the next_middleware is None
    assert middleware.next_middleware is None

    # Create a middleware with a next middleware
    next_middleware = Middleware()
    middleware = Middleware(next_middleware)

    # Check that the next_middleware is set correctly
    assert middleware.next_middleware is next_middleware


def test_middleware_process():
    """Test Middleware process method."""
    # Create a mock action
    action = MockAction()

    # Create a middleware
    middleware = Middleware()

    # Process the action
    result = middleware.process(action)

    # Check that the result is correct
    assert result.success is True
    assert result.message == "Successfully executed mock_action"


def test_middleware_chain():
    """Test middleware chain processing."""
    # Create a mock action
    action = MockAction()

    # Create middleware classes
    class TestMiddleware1(Middleware):
        def process(self, action, **kwargs):
            return ActionResultModel(success=True, message="Middleware 1 processed")

    class TestMiddleware2(Middleware):
        def process(self, action, **kwargs):
            return ActionResultModel(success=True, message="Middleware 2 processed")

    # Create the chain
    middleware_chain = MiddlewareChain()
    middleware_chain.add(TestMiddleware1)
    middleware_chain.add(TestMiddleware2)

    # Build the middleware chain
    middleware = middleware_chain.build()

    # Process the action using the first middleware
    result = middleware.process(action)

    # Check that the result is from the first middleware
    assert result.success is True
    assert result.message == "Middleware 1 processed"


def test_middleware_chain_empty():
    """Test middleware chain with no middlewares."""
    # Create a mock action
    action = MockAction()

    # Create an empty middleware chain
    middleware_chain = MiddlewareChain()

    # Build the middleware chain (should be None)
    middleware = middleware_chain.build()

    # Check that no middleware was built
    assert middleware is None

    # Process the action directly
    result = action.process()

    # Check that the result is from the action
    assert result.success is True
    assert result.message == "Successfully executed mock_action"


def test_middleware_chain_add_and_clear():
    """Test adding and clearing middlewares in a chain."""

    # Create middleware classes
    class TestMiddleware1(Middleware):
        pass

    class TestMiddleware2(Middleware):
        pass

    # Create a middleware chain
    middleware_chain = MiddlewareChain()

    # Add middlewares
    middleware_chain.add(TestMiddleware1)
    middleware_chain.add(TestMiddleware2)

    # Check that the middlewares were added
    assert len(middleware_chain.middlewares) == 2
    assert isinstance(middleware_chain.middlewares[0], TestMiddleware1)
    assert isinstance(middleware_chain.middlewares[1], TestMiddleware2)

    # Clear the chain
    middleware_chain.middlewares.clear()

    # Check that the chain is empty
    assert len(middleware_chain.middlewares) == 0


def test_logging_middleware():
    """Test LoggingMiddleware."""
    # Create a mock action
    action = MockAction()

    # Create a logging middleware
    with patch("dcc_mcp_core.actions.middleware.logger") as mock_logger, patch(
        "dcc_mcp_core.actions.middleware.time.time"
    ) as mock_time:
        # Set up the mock time function to simulate elapsed time
        mock_time.side_effect = [0, 1]  # First call returns 0, second call returns 1 (1 second elapsed)

        # Create the middleware
        middleware = LoggingMiddleware()

        # Process the action
        result = middleware.process(action)

        # Check that the logger was called with the expected messages
        mock_logger.info.assert_any_call(f"Executing action: {action.name}")
        mock_logger.info.assert_any_call(f"Action {action.name} completed successfully in {1.00:.2f}s")

        # Check that the result is correct
        assert result.success is True
        assert result.message == "Successfully executed mock_action"


def test_performance_middleware():
    """Test PerformanceMiddleware."""
    # Create a mock action
    action = MockAction()

    # Create a performance middleware
    with patch("dcc_mcp_core.actions.middleware.time.time") as mock_time:
        # Set up the mock time function to simulate elapsed time
        mock_time.side_effect = [0, 1]  # First call returns 0, second call returns 1 (1 second elapsed)

        # Create the middleware
        middleware = PerformanceMiddleware()

        # Process the action
        result = middleware.process(action)

        # Check that the performance data was added to the result context
        assert result.context is not None
        assert "performance" in result.context
        assert "execution_time" in result.context["performance"]
        assert result.context["performance"]["execution_time"] == 1.0

        # Check that the result is correct
        assert result.success is True
        assert result.message == "Successfully executed mock_action"


def test_middleware_chain_process_with_kwargs():
    """Test middleware chain processing with kwargs."""
    # Create a mock action
    action = MockAction()

    # Create a middleware that passes kwargs
    class KwargsMiddleware(Middleware):
        def process(self, action, **kwargs):
            # Pass kwargs to the next middleware or action
            if self.next_middleware:
                return self.next_middleware.process(action, **kwargs)
            else:
                # Set up the action with kwargs
                action.setup(**kwargs)
                return action.process()

    # Create a middleware chain
    middleware_chain = MiddlewareChain()
    middleware_chain.add(KwargsMiddleware)

    # Build the middleware chain
    middleware = middleware_chain.build()

    # Mock the action's setup method
    action.setup = MagicMock(return_value=action)

    # Process the action with kwargs
    result = middleware.process(action, param1="value1", param2=42)

    # Check that setup was called with the kwargs
    action.setup.assert_called_once_with(param1="value1", param2=42)

    # Check that the result is correct
    assert result.success is True
    assert result.message == "Successfully executed mock_action"
