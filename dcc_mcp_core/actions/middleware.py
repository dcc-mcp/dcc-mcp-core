"""Middleware system for DCC-MCP-Core actions.

This module provides a middleware system for actions, allowing custom logic to be
inserted before and after action execution.
"""

# Import built-in modules
import logging
import time
from typing import List
from typing import Optional
from typing import Type

# Import local modules
from dcc_mcp_core.actions.base import Action
from dcc_mcp_core.models import ActionResultModel

# Setup logger
logger = logging.getLogger(__name__)


class Middleware:
    """Base middleware class for actions.

    Middleware can be used to insert custom logic before and after action execution.
    Subclasses should override the process method to implement custom logic.
    """

    def __init__(self, next_middleware: Optional["Middleware"] = None):
        """Initialize a new Middleware instance.

        Args:
            next_middleware: Next middleware in the chain

        """
        self.next_middleware = next_middleware

    def process(self, action: Action, **kwargs) -> ActionResultModel:
        """Process the action with this middleware.

        Args:
            action: Action to process
            **kwargs: Additional arguments for the action

        Returns:
            Result of the action execution

        """
        # Call the next middleware in the chain, or the action if this is the last middleware
        if self.next_middleware:
            return self.next_middleware.process(action, **kwargs)
        else:
            return action.process()

    async def process_async(self, action: Action, **kwargs) -> ActionResultModel:
        """Process the action asynchronously with this middleware.

        Args:
            action: Action to process
            **kwargs: Additional arguments for the action

        Returns:
            Result of the action execution

        """
        # Call the next middleware in the chain, or the action if this is the last middleware
        if self.next_middleware:
            return await self.next_middleware.process_async(action, **kwargs)
        else:
            return await action.process_async()


class LoggingMiddleware(Middleware):
    """Middleware for logging action execution.

    This middleware logs information about action execution, including timing and results.
    """

    def process(self, action: Action, **kwargs) -> ActionResultModel:
        """Process the action with logging.

        Args:
            action: Action to process
            **kwargs: Additional arguments for the action

        Returns:
            Result of the action execution

        """
        logger.info(f"Executing action: {action.name}")
        start_time = time.time()

        try:
            # Call the next middleware in the chain
            result = super().process(action, **kwargs)

            # Log the result
            elapsed_time = time.time() - start_time
            if result.success:
                logger.info(f"Action {action.name} completed successfully in {elapsed_time:.2f}s")
            else:
                logger.warning(f"Action {action.name} failed in {elapsed_time:.2f}s: {result.error}")

            return result
        except Exception as e:
            # Log the exception
            elapsed_time = time.time() - start_time
            logger.error(f"Action {action.name} raised an exception in {elapsed_time:.2f}s: {e}")
            raise

    async def process_async(self, action: Action, **kwargs) -> ActionResultModel:
        """Process the action asynchronously with logging.

        Args:
            action: Action to process
            **kwargs: Additional arguments for the action

        Returns:
            Result of the action execution

        """
        logger.info(f"Executing action asynchronously: {action.name}")
        start_time = time.time()

        try:
            # Call the next middleware in the chain
            result = await super().process_async(action, **kwargs)

            # Log the result
            elapsed_time = time.time() - start_time
            if result.success:
                logger.info(f"Async action {action.name} completed successfully in {elapsed_time:.2f}s")
            else:
                logger.warning(f"Async action {action.name} failed in {elapsed_time:.2f}s: {result.error}")

            return result
        except Exception as e:
            # Log the exception
            elapsed_time = time.time() - start_time
            logger.error(f"Async action {action.name} raised an exception in {elapsed_time:.2f}s: {e}")
            raise


class PerformanceMiddleware(Middleware):
    """Middleware for monitoring action performance.

    This middleware tracks execution time and can be used to identify slow actions.
    """

    def __init__(self, next_middleware: Optional["Middleware"] = None, threshold: float = 1.0):
        """Initialize a new PerformanceMiddleware instance.

        Args:
            next_middleware: Next middleware in the chain
            threshold: Threshold in seconds for slow action warnings

        """
        super().__init__(next_middleware)
        self.threshold = threshold

    def process(self, action: Action, **kwargs) -> ActionResultModel:
        """Process the action with performance monitoring.

        Args:
            action: Action to process
            **kwargs: Additional arguments for the action

        Returns:
            Result of the action execution

        """
        start_time = time.time()

        # Call the next middleware in the chain
        result = super().process(action, **kwargs)

        # Check execution time
        elapsed_time = time.time() - start_time
        if elapsed_time > self.threshold:
            logger.warning(
                f"Slow action detected: {action.name} took {elapsed_time:.2f}s (threshold: {self.threshold:.2f}s)"
            )

        # Add performance data to the result context
        if result.context is None:
            result.context = {}
        result.context["performance"] = {"execution_time": elapsed_time}

        return result

    async def process_async(self, action: Action, **kwargs) -> ActionResultModel:
        """Process the action asynchronously with performance monitoring.

        Args:
            action: Action to process
            **kwargs: Additional arguments for the action

        Returns:
            Result of the action execution

        """
        start_time = time.time()

        # Call the next middleware in the chain
        result = await super().process_async(action, **kwargs)

        # Check execution time
        elapsed_time = time.time() - start_time
        if elapsed_time > self.threshold:
            logger.warning(
                f"Slow async action detected: {action.name} took {elapsed_time:.2f}s (threshold: {self.threshold:.2f}s)"
            )

        # Add performance data to the result context
        if result.context is None:
            result.context = {}
        result.context["performance"] = {"execution_time": elapsed_time}

        return result


class MiddlewareChain:
    """Chain of middleware for action execution.

    This class manages a chain of middleware that will be applied to actions during execution.
    """

    def __init__(self):
        """Initialize a new MiddlewareChain instance."""
        self.middlewares: List[Middleware] = []

    def add(self, middleware_class: Type[Middleware], **kwargs) -> "MiddlewareChain":
        """Add a middleware to the chain.

        Args:
            middleware_class: Middleware class to add
            **kwargs: Additional arguments for the middleware constructor

        Returns:
            self: Returns self for method chaining

        """
        self.middlewares.append(middleware_class(**kwargs))
        return self

    def build(self) -> Optional[Middleware]:
        """Build the middleware chain.

        Returns:
            First middleware in the chain, or None if the chain is empty

        """
        if not self.middlewares:
            return None

        # Link middlewares together
        for i in range(len(self.middlewares) - 1):
            self.middlewares[i].next_middleware = self.middlewares[i + 1]

        # Return the first middleware in the chain
        return self.middlewares[0]
