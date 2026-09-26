"""Event-driven waiting helpers for PyProcessWatcher based tests.

``PyProcessWatcher`` pushes events from a background Tokio loop and exposes
them through ``poll_events()``, which **drains** the queue. Sleeping a fixed
amount of time and then draining exactly once only leaves a couple of poll
cycles of headroom, so a single scheduling hiccup on an oversubscribed runner
turns into a false failure. Waiting until the expected event actually arrives
spends latency instead of correctness.
"""

from __future__ import annotations

import time
from typing import Any

#: Upper bound for a single :func:`wait_for_event` call. First-heartbeat latency
#: measures ~125 ms for a 100 ms poll interval, so this leaves roughly forty
#: poll cycles of headroom even on a heavily oversubscribed runner.
DEFAULT_TIMEOUT = 5.0

#: Sleep between drains. Short enough to not dominate the wait, long enough to
#: keep the wait loop from burning CPU on an already busy runner.
DEFAULT_INTERVAL = 0.005


def wait_for_event(
    watcher: Any,
    event_type: str,
    timeout: float = DEFAULT_TIMEOUT,
    interval: float = DEFAULT_INTERVAL,
) -> list[dict[str, Any]]:
    """Drain ``watcher`` until an event of ``event_type`` has been observed.

    Because ``poll_events()`` drains the queue, every batch collected while
    waiting is accumulated and returned, so callers see the same event list a
    single ``poll_events()`` call would have produced after a fixed sleep.

    Args:
        watcher: Object exposing ``poll_events()`` (a ``PyProcessWatcher``).
        event_type: ``"type"`` value to wait for, e.g. ``"heartbeat"``.
        timeout: Seconds to keep waiting before failing the test.
        interval: Seconds to sleep between drains.

    Returns:
        All events drained while waiting, in arrival order.

    Raises:
        AssertionError: If no matching event arrives within ``timeout``.

    """
    deadline = time.monotonic() + timeout
    collected: list[dict[str, Any]] = []
    while True:
        collected.extend(watcher.poll_events())
        if any(e.get("type") == event_type for e in collected):
            return collected
        if time.monotonic() >= deadline:
            seen = sorted({str(e.get("type")) for e in collected})
            raise AssertionError(
                f"no {event_type!r} event from {type(watcher).__name__} "
                f"within {timeout:.1f}s (saw {len(collected)} event(s): {seen})"
            )
        time.sleep(interval)
