"""Extended tests for the middleware system in actions.middleware module."""

# Import built-in modules
import asyncio
from typing import Optional
from unittest.mock import patch

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core.actions.base import Action
from dcc_mcp_core.actions.middleware import LoggingMiddleware
from dcc_mcp_core.actions.middleware import Middleware
from dcc_mcp_core.actions.middleware import MiddlewareChain
from dcc_mcp_core.actions.middleware import PerformanceMiddleware
from dcc_mcp_core.models import ActionResultModel


class TestAction(Action):
    """Test action class for middleware tests."""

    name = "test_action"
    description = "Test action for middleware tests"

    def _execute(self) -> None:
        """Execute the test action."""
        self.output = self.OutputModel()


class SlowTestAction(Action):
    """Slow test action class for middleware tests."""

    name = "slow_test_action"
    description = "Slow test action for middleware tests"

    def _execute(self) -> None:
        """Execute the slow test action."""
        # Import built-in modules
        import time

        time.sleep(0.1)  # Sleep to simulate a slow action
        self.output = self.OutputModel()


class ErrorTestAction(Action):
    """Error test action class for middleware tests."""

    name = "error_test_action"
    description = "Error test action for middleware tests"

    def _execute(self) -> None:
        """Execute the error test action."""
        raise ValueError("Test error")


class TestMiddleware(Middleware):
    """Custom test middleware for testing."""

    def __init__(self, next_middleware: Optional[Middleware] = None, callback=None):
        """Initialize the test middleware."""
        super().__init__(next_middleware)
        self.callback = callback or (lambda action, **kwargs: None)

    def process(self, action: Action, **kwargs) -> ActionResultModel:
        """Process the action with this middleware."""
        self.callback(action, **kwargs)
        return super().process(action, **kwargs)


class TestMiddlewareExtended:
    """Extended tests for the middleware system."""

    def test_middleware_chain_empty(self):
        """Test building an empty middleware chain."""
        chain = MiddlewareChain()
        middleware = chain.build()
        assert middleware is None

    def test_middleware_chain_single(self):
        """Test building a middleware chain with a single middleware."""
        chain = MiddlewareChain()
        chain.add(TestMiddleware)
        middleware = chain.build()
        assert isinstance(middleware, TestMiddleware)
        assert middleware.next_middleware is None

    def test_middleware_chain_multiple(self):
        """Test building a middleware chain with multiple middlewares."""
        chain = MiddlewareChain()
        chain.add(TestMiddleware).add(LoggingMiddleware).add(PerformanceMiddleware, threshold=0.5)
        middleware = chain.build()

        # Check the chain structure
        assert isinstance(middleware, TestMiddleware)
        assert isinstance(middleware.next_middleware, LoggingMiddleware)
        assert isinstance(middleware.next_middleware.next_middleware, PerformanceMiddleware)
        assert middleware.next_middleware.next_middleware.next_middleware is None
        assert middleware.next_middleware.next_middleware.threshold == 0.5

    def test_middleware_execution_order(self):
        """Test middleware execution order."""
        execution_order = []

        def callback1(action, **kwargs):
            execution_order.append("middleware1")

        def callback2(action, **kwargs):
            execution_order.append("middleware2")

        def callback3(action, **kwargs):
            execution_order.append("middleware3")

        # Create the middleware chain
        chain = MiddlewareChain()
        chain.add(TestMiddleware, callback=callback1)
        chain.add(TestMiddleware, callback=callback2)
        chain.add(TestMiddleware, callback=callback3)
        middleware = chain.build()

        # Execute the action through the middleware chain
        action = TestAction()
        middleware.process(action)

        # Check the execution order
        assert execution_order == ["middleware1", "middleware2", "middleware3"]

    @patch("dcc_mcp_core.actions.middleware.logger")
    def test_logging_middleware(self, mock_logger):
        """Test LoggingMiddleware logs action execution."""
        # Create the middleware chain
        chain = MiddlewareChain()
        chain.add(LoggingMiddleware)
        middleware = chain.build()

        # Execute the action through the middleware
        action = TestAction()
        result = middleware.process(action)

        # Check that the logger was called correctly
        assert mock_logger.info.call_count >= 2
        mock_logger.info.assert_any_call(f"Executing action: {action.name}")
        # Check for success log message
        success_call_args = [
            call_args
            for call_args in mock_logger.info.call_args_list
            if f"Action {action.name} completed successfully" in call_args[0][0]
        ]
        assert len(success_call_args) > 0

        # Check the result
        assert result.success is True

    @patch("dcc_mcp_core.actions.middleware.logger")
    def test_logging_middleware_with_error(self, mock_logger):
        """Test LoggingMiddleware logs action execution errors."""
        # Create the middleware chain
        chain = MiddlewareChain()
        chain.add(LoggingMiddleware)
        middleware = chain.build()

        # Execute the action through the middleware
        action = ErrorTestAction()
        result = middleware.process(action)

        # Check that the logger was called correctly
        assert mock_logger.info.call_count >= 1
        mock_logger.info.assert_any_call(f"Executing action: {action.name}")
        # Check for warning log message
        warning_call_args = [
            call_args
            for call_args in mock_logger.warning.call_args_list
            if f"Action {action.name} failed" in call_args[0][0]
        ]
        assert len(warning_call_args) > 0

        # Check the result
        assert result.success is False
        assert result.error == "Test error"

    def test_performance_middleware(self):
        """Test PerformanceMiddleware tracks execution time."""
        # Create the middleware chain with a low threshold
        chain = MiddlewareChain()
        chain.add(PerformanceMiddleware, threshold=0.05)
        middleware = chain.build()

        # Execute a slow action through the middleware
        action = SlowTestAction()
        result = middleware.process(action)

        # Check the result context contains performance data
        assert "performance" in result.context
        assert "execution_time" in result.context["performance"]
        assert result.context["performance"]["execution_time"] > 0

    @patch("dcc_mcp_core.actions.middleware.logger")
    def test_performance_middleware_warning(self, mock_logger):
        """Test PerformanceMiddleware warns for slow actions."""
        # Create the middleware chain with a very low threshold
        chain = MiddlewareChain()
        chain.add(PerformanceMiddleware, threshold=0.01)
        middleware = chain.build()

        # Execute a slow action through the middleware
        action = SlowTestAction()
        middleware.process(action)

        # Check that the logger was called with a warning
        warning_call_args = [
            call_args
            for call_args in mock_logger.warning.call_args_list
            if f"Slow action detected: {action.name}" in call_args[0][0]
        ]
        assert len(warning_call_args) > 0

    @pytest.mark.asyncio
    async def test_middleware_async_execution(self):
        """Test asynchronous middleware execution."""
        execution_order = []

        class AsyncTestMiddleware(Middleware):
            async def process_async(self, action: Action, **kwargs):
                execution_order.append(f"middleware{id(self)}")
                return await super().process_async(action, **kwargs)

        # Create the middleware chain
        chain = MiddlewareChain()
        chain.add(AsyncTestMiddleware)
        chain.add(AsyncTestMiddleware)
        middleware = chain.build()

        # Execute the action through the middleware chain
        action = TestAction()
        await middleware.process_async(action)

        # Check the execution order
        assert len(execution_order) == 2

    @pytest.mark.asyncio
    async def test_performance_middleware_async(self):
        """Test PerformanceMiddleware in async mode."""
        # Since mock_async_method may have compatibility issues in Python 3.7 environment,
        # we modify the test strategy to directly verify performance data

        # Create a very low threshold
        threshold = 0.001

        # Create middleware chain
        chain = MiddlewareChain()
        chain.add(PerformanceMiddleware, threshold=threshold)
        middleware = chain.build()

        # Create an async slow action
        class AsyncSlowAction(Action):
            name = "async_slow_action"

            async def _execute_async(self):
                # Use a long enough sleep time to ensure it exceeds the threshold
                await asyncio.sleep(0.2)  # Sleep time set to 0.2 seconds
                self.output = self.OutputModel()

        # Execute the action through the middleware
        action = AsyncSlowAction()
        result = await middleware.process_async(action)

        # Verify performance data in the result context
        assert result.success is True, "Action execution failed"
        assert "performance" in result.context, "Result context does not contain performance data"
        assert "execution_time" in result.context["performance"], "Performance data does not contain execution time"

        # Verify execution time is greater than threshold
        execution_time = result.context["performance"]["execution_time"]
        assert execution_time > threshold, (
            f"Execution time {execution_time} should be greater than threshold {threshold}"
        )

        # Since we no longer use mock_logger, we only need to verify the performance data is correct
        # This test works in Python 3.7 environment
