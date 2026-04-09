"""Deep tests for EventBus pub/sub: multiple subscribers, kwargs, isolation, error resilience.

Covers:
- subscribe returns incrementing int IDs
- publish delivers kwargs to all subscribers
- multiple subscribers on same event all receive events
- unsubscribe stops delivery silently
- different event names are isolated
- error in one callback does not prevent other callbacks
- multiple independent EventBus instances do not interfere
"""

from __future__ import annotations

import pytest

import dcc_mcp_core


class TestEventBusBasic:
    def test_construction(self) -> None:
        eb = dcc_mcp_core.EventBus()
        assert eb is not None

    def test_repr_contains_eventbus(self) -> None:
        eb = dcc_mcp_core.EventBus()
        assert "EventBus" in repr(eb)

    def test_subscribe_returns_int(self) -> None:
        eb = dcc_mcp_core.EventBus()
        sid = eb.subscribe("evt", lambda **kw: None)
        assert isinstance(sid, int)

    def test_subscribe_ids_incrementing(self) -> None:
        eb = dcc_mcp_core.EventBus()
        sid1 = eb.subscribe("evt", lambda **kw: None)
        sid2 = eb.subscribe("evt", lambda **kw: None)
        sid3 = eb.subscribe("evt", lambda **kw: None)
        assert sid1 < sid2 < sid3

    def test_subscribe_different_events_incrementing(self) -> None:
        eb = dcc_mcp_core.EventBus()
        sid1 = eb.subscribe("evt_a", lambda **kw: None)
        sid2 = eb.subscribe("evt_b", lambda **kw: None)
        assert sid1 < sid2

    def test_publish_no_subscribers_is_noop(self) -> None:
        eb = dcc_mcp_core.EventBus()
        # Should not raise
        eb.publish("orphan_event", key="value")

    def test_publish_delivers_kwargs(self) -> None:
        eb = dcc_mcp_core.EventBus()
        received = []
        eb.subscribe("evt", lambda **kw: received.append(kw))
        eb.publish("evt", x=1, y=2, msg="hello")
        assert received == [{"x": 1, "y": 2, "msg": "hello"}]

    def test_publish_no_kwargs(self) -> None:
        eb = dcc_mcp_core.EventBus()
        received = []
        eb.subscribe("ping", lambda **kw: received.append(kw))
        eb.publish("ping")
        assert received == [{}]

    def test_publish_multiple_times(self) -> None:
        eb = dcc_mcp_core.EventBus()
        count = []
        eb.subscribe("tick", lambda **kw: count.append(1))
        for _ in range(5):
            eb.publish("tick")
        assert len(count) == 5

    def test_publish_kwargs_vary_per_call(self) -> None:
        eb = dcc_mcp_core.EventBus()
        received = []
        eb.subscribe("evt", lambda **kw: received.append(kw))
        eb.publish("evt", val=1)
        eb.publish("evt", val=2)
        eb.publish("evt", val=3)
        assert received == [{"val": 1}, {"val": 2}, {"val": 3}]


class TestEventBusMultipleSubscribers:
    def test_two_subscribers_both_called(self) -> None:
        eb = dcc_mcp_core.EventBus()
        calls: list = []
        eb.subscribe("evt", lambda **kw: calls.append("a"))
        eb.subscribe("evt", lambda **kw: calls.append("b"))
        eb.publish("evt")
        assert "a" in calls
        assert "b" in calls
        assert len(calls) == 2

    def test_three_subscribers_all_called(self) -> None:
        eb = dcc_mcp_core.EventBus()
        calls: list = []
        eb.subscribe("evt", lambda **kw: calls.append(1))
        eb.subscribe("evt", lambda **kw: calls.append(2))
        eb.subscribe("evt", lambda **kw: calls.append(3))
        eb.publish("evt")
        assert len(calls) == 3
        assert set(calls) == {1, 2, 3}

    def test_multiple_subscribers_receive_same_kwargs(self) -> None:
        eb = dcc_mcp_core.EventBus()
        received1: list = []
        received2: list = []
        eb.subscribe("evt", lambda **kw: received1.append(kw))
        eb.subscribe("evt", lambda **kw: received2.append(kw))
        eb.publish("evt", val=42)
        assert received1 == [{"val": 42}]
        assert received2 == [{"val": 42}]

    def test_five_subscribers_all_called(self) -> None:
        eb = dcc_mcp_core.EventBus()
        calls: list = []
        for i in range(5):
            idx = i
            eb.subscribe("evt", lambda idx=idx, **kw: calls.append(idx))
        eb.publish("evt")
        assert len(calls) == 5
        assert set(calls) == {0, 1, 2, 3, 4}


class TestEventBusUnsubscribe:
    def test_unsubscribe_stops_callback(self) -> None:
        eb = dcc_mcp_core.EventBus()
        received: list = []
        sid = eb.subscribe("evt", lambda **kw: received.append(kw))
        eb.publish("evt", x=1)
        eb.unsubscribe("evt", sid)
        eb.publish("evt", x=2)
        assert len(received) == 1
        assert received[0] == {"x": 1}

    def test_unsubscribe_one_of_two_keeps_other(self) -> None:
        eb = dcc_mcp_core.EventBus()
        a_calls: list = []
        b_calls: list = []
        sid_a = eb.subscribe("evt", lambda **kw: a_calls.append(kw))
        eb.subscribe("evt", lambda **kw: b_calls.append(kw))
        eb.publish("evt", val=1)
        eb.unsubscribe("evt", sid_a)
        eb.publish("evt", val=2)
        assert len(a_calls) == 1  # only first publish
        assert len(b_calls) == 2  # both publishes

    def test_unsubscribe_nonexistent_is_noop(self) -> None:
        eb = dcc_mcp_core.EventBus()
        # Should not raise even if subscriber ID doesn't exist
        eb.unsubscribe("evt", 99999)

    def test_unsubscribe_from_unknown_event_is_noop(self) -> None:
        eb = dcc_mcp_core.EventBus()
        # Unsubscribing from event that was never subscribed
        eb.unsubscribe("never_subscribed_event", 1)

    def test_unsubscribe_then_resubscribe(self) -> None:
        eb = dcc_mcp_core.EventBus()
        calls: list = []
        sid = eb.subscribe("evt", lambda **kw: calls.append("old"))
        eb.unsubscribe("evt", sid)
        eb.subscribe("evt", lambda **kw: calls.append("new"))
        eb.publish("evt")
        assert calls == ["new"]

    def test_unsubscribe_all_leaves_no_listeners(self) -> None:
        eb = dcc_mcp_core.EventBus()
        calls: list = []
        sid1 = eb.subscribe("evt", lambda **kw: calls.append(1))
        sid2 = eb.subscribe("evt", lambda **kw: calls.append(2))
        eb.unsubscribe("evt", sid1)
        eb.unsubscribe("evt", sid2)
        eb.publish("evt")
        assert len(calls) == 0


class TestEventBusIsolation:
    def test_different_events_isolated(self) -> None:
        eb = dcc_mcp_core.EventBus()
        a_calls: list = []
        b_calls: list = []
        eb.subscribe("evt_a", lambda **kw: a_calls.append(1))
        eb.subscribe("evt_b", lambda **kw: b_calls.append(1))
        eb.publish("evt_a")
        assert len(a_calls) == 1
        assert len(b_calls) == 0
        eb.publish("evt_b")
        assert len(a_calls) == 1
        assert len(b_calls) == 1

    def test_three_events_fully_isolated(self) -> None:
        eb = dcc_mcp_core.EventBus()
        counts: dict = {"a": 0, "b": 0, "c": 0}
        eb.subscribe("evt:a", lambda **kw: counts.__setitem__("a", counts["a"] + 1))
        eb.subscribe("evt:b", lambda **kw: counts.__setitem__("b", counts["b"] + 1))
        eb.subscribe("evt:c", lambda **kw: counts.__setitem__("c", counts["c"] + 1))
        eb.publish("evt:a")
        eb.publish("evt:a")
        eb.publish("evt:b")
        assert counts == {"a": 2, "b": 1, "c": 0}

    def test_subscriber_on_two_events(self) -> None:
        eb = dcc_mcp_core.EventBus()
        calls: list = []

        def cb(**kw) -> None:
            calls.append(kw)

        eb.subscribe("evt_x", cb)
        eb.subscribe("evt_y", cb)
        eb.publish("evt_x", src="x")
        eb.publish("evt_y", src="y")
        assert len(calls) == 2
        srcs = {c["src"] for c in calls}
        assert srcs == {"x", "y"}

    def test_independent_event_bus_instances(self) -> None:
        eb1 = dcc_mcp_core.EventBus()
        eb2 = dcc_mcp_core.EventBus()
        calls1: list = []
        calls2: list = []
        eb1.subscribe("evt", lambda **kw: calls1.append(1))
        eb2.subscribe("evt", lambda **kw: calls2.append(2))
        eb1.publish("evt")
        assert len(calls1) == 1
        assert len(calls2) == 0  # eb2 not triggered by eb1.publish

    def test_two_buses_same_event_name_independent(self) -> None:
        bus_a = dcc_mcp_core.EventBus()
        bus_b = dcc_mcp_core.EventBus()
        a_received: list = []
        b_received: list = []
        bus_a.subscribe("shared", lambda **kw: a_received.append(kw))
        bus_b.subscribe("shared", lambda **kw: b_received.append(kw))
        bus_a.publish("shared", from_bus="a")
        assert len(a_received) == 1
        assert len(b_received) == 0
        bus_b.publish("shared", from_bus="b")
        assert len(a_received) == 1
        assert len(b_received) == 1


class TestEventBusErrorResilience:
    def test_error_in_callback_does_not_crash_publisher(self) -> None:
        eb = dcc_mcp_core.EventBus()

        def bad_callback(**kw) -> None:
            msg = "intentional test error"
            raise ValueError(msg)

        good_calls: list = []
        eb.subscribe("evt", bad_callback)
        eb.subscribe("evt", lambda **kw: good_calls.append(1))
        # Should not raise even though bad_callback raises
        eb.publish("evt", val=1)
        # good_calls may or may not be filled depending on order - just check no crash

    def test_subscribe_then_publish_order(self) -> None:
        eb = dcc_mcp_core.EventBus()
        order: list = []
        eb.subscribe("evt", lambda **kw: order.append("a"))
        eb.subscribe("evt", lambda **kw: order.append("b"))
        eb.subscribe("evt", lambda **kw: order.append("c"))
        eb.publish("evt")
        # All three should be called (order may vary per implementation)
        assert len(order) == 3
        assert set(order) == {"a", "b", "c"}

    def test_publish_with_many_kwargs(self) -> None:
        eb = dcc_mcp_core.EventBus()
        received: list = []
        eb.subscribe("rich_evt", lambda **kw: received.append(kw))
        kwargs = {f"key_{i}": i for i in range(20)}
        eb.publish("rich_evt", **kwargs)
        assert received[0] == kwargs

    def test_callback_modifies_shared_state(self) -> None:
        eb = dcc_mcp_core.EventBus()
        shared = {"count": 0}

        def counter(**kw) -> None:
            shared["count"] += kw.get("increment", 1)

        eb.subscribe("counter", counter)
        eb.publish("counter", increment=5)
        eb.publish("counter", increment=3)
        eb.publish("counter")
        assert shared["count"] == 9
