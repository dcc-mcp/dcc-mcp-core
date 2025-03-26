"""Event system for DCC-MCP-Core actions.

This module provides a simple event system for actions, allowing them to publish
events and subscribe to events from other actions or components.
"""

# Import built-in modules
import asyncio
from concurrent.futures import ThreadPoolExecutor
import logging
from typing import Callable
from typing import Dict
from typing import Set

# Setup logger
logger = logging.getLogger(__name__)


class Event:
    """Event class for action events.

    This class represents an event in the action system, containing
    the event name and any additional data associated with the event.

    Attributes:
        name: Name of the event
        data: Additional data associated with the event

    """

    def __init__(self, name: str, **data):
        """Initialize a new Event instance.

        Args:
            name: Name of the event
            **data: Additional data associated with the event

        """
        self.name = name
        self.data = data

    def __str__(self) -> str:
        """Return a string representation of the event."""
        return f"Event(name={self.name}, data={self.data})"


class EventBus:
    """Simple event bus for action events.

    This class provides a centralized event bus for actions to publish events
    and subscribe to events from other actions or components.
    """

    def __init__(self):
        """Initialize a new EventBus instance."""
        self._subscribers: Dict[str, Set[Callable]] = {}
        self._async_subscribers: Dict[str, Set[Callable]] = {}

    def subscribe(self, event_name: str, callback: Callable) -> None:
        """Subscribe to an event.

        Args:
            event_name: Name of the event to subscribe to
            callback: Callback function to call when the event is published

        """
        if event_name not in self._subscribers:
            self._subscribers[event_name] = set()
        self._subscribers[event_name].add(callback)

    def unsubscribe(self, event_name: str, callback: Callable) -> None:
        """Unsubscribe from an event.

        Args:
            event_name: Name of the event to unsubscribe from
            callback: Callback function to remove

        """
        if event_name in self._subscribers and callback in self._subscribers[event_name]:
            self._subscribers[event_name].remove(callback)
            if not self._subscribers[event_name]:
                del self._subscribers[event_name]

    def publish(self, event_name: str, *args, **kwargs) -> None:
        """Publish an event.

        Args:
            event_name: Name of the event to publish
            *args: Positional arguments to pass to the subscribers
            **kwargs: Keyword arguments to pass to the subscribers

        """
        if event_name in self._subscribers:
            for callback in list(self._subscribers[event_name]):
                try:
                    callback(*args, **kwargs)
                except Exception as e:
                    logger.error(f"Error in event subscriber for {event_name}: {e}")
                    logger.debug(f"Subscriber: {callback}")

    async def subscribe_async(self, event_name: str, callback: Callable) -> None:
        """Subscribe to an event with an async callback.

        Args:
            event_name: Name of the event to subscribe to
            callback: Async callback function to call when the event is published

        """
        if event_name not in self._async_subscribers:
            self._async_subscribers[event_name] = set()
        self._async_subscribers[event_name].add(callback)

    async def unsubscribe_async(self, event_name: str, callback: Callable) -> None:
        """Unsubscribe from an event with an async callback.

        Args:
            event_name: Name of the event to unsubscribe from
            callback: Async callback function to remove

        """
        if event_name in self._async_subscribers and callback in self._async_subscribers[event_name]:
            self._async_subscribers[event_name].remove(callback)
            if not self._async_subscribers[event_name]:
                del self._async_subscribers[event_name]

    async def publish_async(self, event_name: str, *args, **kwargs) -> None:
        """Publish an event asynchronously.

        Args:
            event_name: Name of the event to publish
            *args: Positional arguments to pass to the subscribers
            **kwargs: Keyword arguments to pass to the subscribers

        """
        # Call synchronous subscribers in a thread pool
        if event_name in self._subscribers:
            loop = asyncio.get_event_loop()
            executor = ThreadPoolExecutor()
            for callback in list(self._subscribers[event_name]):
                try:
                    await loop.run_in_executor(executor, lambda: callback(*args, **kwargs))
                except Exception as e:
                    logger.error(f"Error in event subscriber for {event_name}: {e}")
                    logger.debug(f"Subscriber: {callback}")

        # Call asynchronous subscribers
        if event_name in self._async_subscribers:
            for callback in list(self._async_subscribers[event_name]):
                try:
                    await callback(*args, **kwargs)
                except Exception as e:
                    logger.error(f"Error in async event subscriber for {event_name}: {e}")
                    logger.debug(f"Subscriber: {callback}")


# Global event bus instance
event_bus = EventBus()
