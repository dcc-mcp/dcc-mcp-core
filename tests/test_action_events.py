"""Tests for the event system in DCC-MCP-Core.

This module contains tests for the Event and EventBus classes.
"""

# Import built-in modules
import asyncio
from unittest.mock import MagicMock

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core.actions.events import Event
from dcc_mcp_core.actions.events import EventBus
from dcc_mcp_core.actions.events import event_bus


def test_event_init():
    """Test Event initialization."""
    # Create a new Event instance
    event = Event("test_event", param1="value1", param2=42)

    # Check that the event has the correct name and data
    assert event.name == "test_event"
    assert event.data == {"param1": "value1", "param2": 42}


def test_event_str():
    """Test Event string representation."""
    # Create a new Event instance
    event = Event("test_event", param1="value1")

    # Check string representation
    assert str(event) == "Event(name=test_event, data={'param1': 'value1'})"


def test_event_bus_init():
    """Test EventBus initialization."""
    # Create a new EventBus instance
    bus = EventBus()

    # Check that the subscribers dictionaries are empty
    assert bus._subscribers == {}
    assert bus._async_subscribers == {}


def test_event_bus_subscribe():
    """Test EventBus subscribe method."""
    # Create a new EventBus instance
    bus = EventBus()

    # Create a mock callback
    callback = MagicMock()

    # Subscribe to an event
    bus.subscribe("test_event", callback)

    # Check that the callback was added to the subscribers
    assert "test_event" in bus._subscribers
    assert callback in bus._subscribers["test_event"]


def test_event_bus_unsubscribe():
    """Test EventBus unsubscribe method."""
    # Create a new EventBus instance
    bus = EventBus()

    # Create a mock callback
    callback = MagicMock()

    # Subscribe to an event
    bus.subscribe("test_event", callback)

    # Unsubscribe from the event
    bus.unsubscribe("test_event", callback)

    # Check that the callback was removed from the subscribers
    assert "test_event" not in bus._subscribers


def test_event_bus_unsubscribe_nonexistent():
    """Test EventBus unsubscribe method with nonexistent event or callback."""
    # Create a new EventBus instance
    bus = EventBus()

    # Create a mock callback
    callback = MagicMock()
    another_callback = MagicMock()

    # Subscribe to an event
    bus.subscribe("test_event", callback)

    # Unsubscribe from a nonexistent event
    bus.unsubscribe("nonexistent_event", callback)

    # Unsubscribe a nonexistent callback
    bus.unsubscribe("test_event", another_callback)

    # Check that the original subscription is still there
    assert "test_event" in bus._subscribers
    assert callback in bus._subscribers["test_event"]


def test_event_bus_publish():
    """Test EventBus publish method."""
    # Create a new EventBus instance
    bus = EventBus()

    # Create a mock callback
    callback = MagicMock()

    # Subscribe to an event
    bus.subscribe("test_event", callback)

    # Publish an event
    bus.publish("test_event", "arg1", "arg2", kwarg1="value1")

    # Check that the callback was called with the correct arguments
    callback.assert_called_once_with("arg1", "arg2", kwarg1="value1")


def test_event_bus_publish_multiple_subscribers():
    """Test EventBus publish method with multiple subscribers."""
    # Create a new EventBus instance
    bus = EventBus()

    # Create mock callbacks
    callback1 = MagicMock()
    callback2 = MagicMock()

    # Subscribe to an event
    bus.subscribe("test_event", callback1)
    bus.subscribe("test_event", callback2)

    # Publish an event
    bus.publish("test_event", "arg1", kwarg1="value1")

    # Check that both callbacks were called with the correct arguments
    callback1.assert_called_once_with("arg1", kwarg1="value1")
    callback2.assert_called_once_with("arg1", kwarg1="value1")


def test_event_bus_publish_nonexistent_event():
    """Test EventBus publish method with a nonexistent event."""
    # Create a new EventBus instance
    bus = EventBus()

    # Create a mock callback
    callback = MagicMock()

    # Subscribe to an event
    bus.subscribe("test_event", callback)

    # Publish a nonexistent event
    bus.publish("nonexistent_event")

    # Check that the callback was not called
    callback.assert_not_called()


def test_event_bus_publish_exception_in_subscriber():
    """Test EventBus publish method with an exception in a subscriber."""
    # Create a new EventBus instance
    bus = EventBus()

    # Create a callback that raises an exception
    def callback_with_exception(*args, **kwargs):
        raise ValueError("Test exception")

    # Create a normal callback
    normal_callback = MagicMock()

    # Subscribe to an event
    bus.subscribe("test_event", callback_with_exception)
    bus.subscribe("test_event", normal_callback)

    # Publish an event
    bus.publish("test_event")

    # Check that the normal callback was still called despite the exception
    normal_callback.assert_called_once()


@pytest.mark.asyncio
async def test_event_bus_subscribe_async():
    """Test EventBus subscribe_async method."""
    # Create a new EventBus instance
    bus = EventBus()

    # Create a mock callback
    callback = MagicMock()

    # Subscribe to an event asynchronously
    await bus.subscribe_async("test_event", callback)

    # Check that the callback was added to the async subscribers
    assert "test_event" in bus._async_subscribers
    assert callback in bus._async_subscribers["test_event"]


@pytest.mark.asyncio
async def test_event_bus_unsubscribe_async():
    """Test EventBus unsubscribe_async method."""
    # Create a new EventBus instance
    bus = EventBus()

    # Create a mock callback
    callback = MagicMock()

    # Subscribe to an event asynchronously
    await bus.subscribe_async("test_event", callback)

    # Unsubscribe from the event asynchronously
    await bus.unsubscribe_async("test_event", callback)

    # Check that the callback was removed from the async subscribers
    assert "test_event" not in bus._async_subscribers


@pytest.mark.asyncio
async def test_event_bus_publish_async():
    """Test EventBus publish_async method."""
    # Create a new EventBus instance
    bus = EventBus()

    # Create a mock callback
    callback = MagicMock()

    # Subscribe to an event asynchronously
    await bus.subscribe_async("test_event", callback)

    # Publish an event asynchronously
    await bus.publish_async("test_event", "arg1", kwarg1="value1")

    # Check that the callback was called with the correct arguments
    callback.assert_called_once_with("arg1", kwarg1="value1")


@pytest.mark.asyncio
async def test_event_bus_publish_async_with_coroutine():
    """Test EventBus publish_async method with a coroutine callback."""
    # Create a new EventBus instance
    bus = EventBus()

    # Create a result holder
    result = {"value": None}

    # Create an async callback
    async def async_callback(*args, **kwargs):
        await asyncio.sleep(0.1)  # Simulate async work
        result["value"] = (args, kwargs)

    # Subscribe to an event asynchronously
    await bus.subscribe_async("test_event", async_callback)

    # Publish an event asynchronously
    await bus.publish_async("test_event", "arg1", kwarg1="value1")

    # Check that the callback was executed and set the result
    assert result["value"] == (("arg1",), {"kwarg1": "value1"})


def test_global_event_bus():
    """Test the global event_bus instance."""
    # Check that the global event_bus is an instance of EventBus
    assert isinstance(event_bus, EventBus)
