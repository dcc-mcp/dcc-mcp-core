"""Cancellable-loop skill — iterate while honouring cooperative cancellation.

Reads parameters as JSON from stdin (dcc-mcp-core ``execute_script``
protocol), runs a loop calling :func:`dcc_mcp_core.check_cancelled`
once per iteration, and prints a standard skill result dict to stdout.

Parameters
----------
    iterations: Number of loop iterations (default ``10``).
    sleep_ms: Milliseconds to sleep between iterations (default ``100``).

"""

from __future__ import annotations

# Import built-in modules
import json
import sys
import time

# Import local modules
from dcc_mcp_core import CancelledError
from dcc_mcp_core import check_cancelled
from dcc_mcp_core import skill_entry
from dcc_mcp_core import skill_error
from dcc_mcp_core import skill_success


@skill_entry
def count(iterations: int = 10, sleep_ms: int = 100, **_: object) -> dict:
    """Run a cancellation-aware counting loop.

    Args:
        iterations: How many times to iterate.
        sleep_ms: Milliseconds to sleep per iteration (simulates work).

    Returns:
        A standard skill result dict.  On cancellation, returns a
        failure result carrying the number of iterations completed
        before the cancel took effect.

    """
    completed = 0
    delay = max(0.0, sleep_ms / 1000.0)
    try:
        for _ in range(max(0, iterations)):
            check_cancelled()
            if delay:
                time.sleep(delay)
            completed += 1
    except CancelledError as exc:
        return skill_error(
            f"Cancelled after {completed} iteration(s)",
            repr(exc),
            prompt="The client cancelled the request; no cleanup needed.",
            iterations_completed=completed,
        )
    return skill_success(
        f"Completed {completed} iterations",
        iterations=completed,
    )


def main() -> dict:
    """Entry point: read JSON params from stdin, forward to :func:`count`."""
    params: dict = {}
    try:
        if not sys.stdin.isatty():
            raw = sys.stdin.read()
            if raw.strip():
                params = json.loads(raw)
    except (OSError, ValueError):
        params = {}
    return count(**params)


if __name__ == "__main__":
    result = main()
    print(json.dumps(result, default=str))
