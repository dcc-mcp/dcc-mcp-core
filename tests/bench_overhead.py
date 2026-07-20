"""Observability collection overhead baseline benchmark (PIP-2766).

Measures the incremental cost of observability instrumentation on tool-call
paths to validate that default configs are safe for DCC main-thread interaction.

Usage:
    python tests/bench_overhead.py [--iterations 1000] [--batch-size 10]
"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import sqlite3
import tempfile
import time
from typing import Any
import uuid

# ---------------------------------------------------------------------------
# Bench helpers
# ---------------------------------------------------------------------------


def now_ms() -> int:
    return int(time.time() * 1000)


class Timer:
    def __init__(self, label: str):
        self.label = label
        self._start = 0.0

    def __enter__(self):
        self._start = time.perf_counter()
        return self

    def __exit__(self, *args):
        elapsed_ms = (time.perf_counter() - self._start) * 1000
        print(f"  {self.label}: {elapsed_ms:.3f} ms")


def make_tool_call_event(session_id: str = "", request_id: str = "") -> dict[str, Any]:
    return {
        "request_id": request_id or uuid.uuid4().hex,
        "session_id": session_id or "bench-session",
        "parent_request_id": None,
        "batch_id": None,
        "tool_name": "bench_tool",
        "skill_name": None,
        "dcc_type": "maya",
        "instance_id": None,
        "agent_id": None,
        "transport": "mcp",
        "via_gateway": True,
        "started_at_ms": now_ms(),
        "duration_ms": 42,
        "success": True,
        "error_message": None,
        "error_kind": None,
        "mcp_method": "call",
        "trace_id": uuid.uuid4().hex,
        "span_id": uuid.uuid4().hex[:16],
    }


# ---------------------------------------------------------------------------
# Bench 1: JSON serialization overhead
# ---------------------------------------------------------------------------


def bench_serialization(iterations: int = 10000) -> None:
    event = make_tool_call_event()
    with Timer(f"serialize {iterations} events"):
        for _ in range(iterations):
            _ = json.dumps(event)


# ---------------------------------------------------------------------------
# Bench 2: SQLite write overhead (single inserts)
# ---------------------------------------------------------------------------


def bench_sqlite_write(iterations: int = 1000) -> None:
    fd, path = tempfile.mkstemp(suffix=".db")
    os.close(fd)
    try:
        conn = sqlite3.connect(path)
        conn.execute("PRAGMA journal_mode=WAL")
        conn.execute(
            """CREATE TABLE tool_calls (
                request_id TEXT PRIMARY KEY,
                session_id TEXT,
                parent_request_id TEXT,
                batch_id TEXT,
                tool_name TEXT,
                skill_name TEXT,
                dcc_type TEXT,
                instance_id TEXT,
                agent_id TEXT,
                transport TEXT,
                via_gateway INTEGER,
                started_at_ms INTEGER,
                duration_ms INTEGER,
                success INTEGER,
                error_message TEXT,
                error_kind TEXT,
                mcp_method TEXT,
                trace_id TEXT,
                span_id TEXT
            )"""
        )

        event = make_tool_call_event()

        # Single inserts
        with Timer(f"INSERT {iterations} events (single)"):
            for i in range(iterations):
                event["request_id"] = f"req-{i}"
                conn.execute(
                    """INSERT OR REPLACE INTO tool_calls
                    (request_id, session_id, parent_request_id, batch_id,
                     tool_name, skill_name, dcc_type, instance_id, agent_id,
                     transport, via_gateway, started_at_ms, duration_ms,
                     success, error_message, error_kind, mcp_method,
                     trace_id, span_id)
                    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
                    (
                        event["request_id"],
                        event["session_id"],
                        event.get("parent_request_id"),
                        event.get("batch_id"),
                        event["tool_name"],
                        event.get("skill_name"),
                        event.get("dcc_type"),
                        event.get("instance_id"),
                        event.get("agent_id"),
                        event.get("transport"),
                        event.get("via_gateway"),
                        event["started_at_ms"],
                        event["duration_ms"],
                        event["success"],
                        event.get("error_message"),
                        event.get("error_kind"),
                        event.get("mcp_method"),
                        event.get("trace_id"),
                        event.get("span_id"),
                    ),
                )
        conn.commit()

        # Batch insert via executemany
        batch = []
        for i in range(iterations):
            event_batch = make_tool_call_event()
            event_batch["request_id"] = f"batch-req-{i}"
            batch.append(
                (
                    event_batch["request_id"],
                    event_batch["session_id"],
                    event_batch.get("parent_request_id"),
                    event_batch.get("batch_id"),
                    event_batch["tool_name"],
                    event_batch.get("skill_name"),
                    event_batch.get("dcc_type"),
                    event_batch.get("instance_id"),
                    event_batch.get("agent_id"),
                    event_batch.get("transport"),
                    event_batch.get("via_gateway"),
                    event_batch["started_at_ms"],
                    event_batch["duration_ms"],
                    event_batch["success"],
                    event_batch.get("error_message"),
                    event_batch.get("error_kind"),
                    event_batch.get("mcp_method"),
                    event_batch.get("trace_id"),
                    event_batch.get("span_id"),
                )
            )
        with Timer(f"INSERT {iterations} events (executemany)"):
            conn.executemany(
                """INSERT OR REPLACE INTO tool_calls
                (request_id, session_id, parent_request_id, batch_id,
                 tool_name, skill_name, dcc_type, instance_id, agent_id,
                 transport, via_gateway, started_at_ms, duration_ms,
                 success, error_message, error_kind, mcp_method,
                 trace_id, span_id)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
                batch,
            )
        conn.commit()

        # Read overhead
        with Timer(f"SELECT {iterations} events"):
            cursor = conn.execute("SELECT * FROM tool_calls LIMIT ?", (iterations,))
            _ = cursor.fetchall()

        conn.close()
    finally:
        Path(path).unlink()


# ---------------------------------------------------------------------------
# Bench 3: Batch dispatch overhead simulation
# ---------------------------------------------------------------------------


def bench_batch_dispatch(num_calls: int = 10, iterations: int = 100) -> None:
    """Simulate call_batch dispatch overhead: N sub-calls with event recording."""
    batch_id = uuid.uuid4().hex
    parent_request_id = uuid.uuid4().hex

    total_serialization_us = 0.0
    total_event_creation_us = 0.0

    for _ in range(iterations):
        for i in range(num_calls):
            # Measure event creation
            t0 = time.perf_counter()
            event = make_tool_call_event(
                request_id=f"{parent_request_id}:batch-{i}",
                session_id="bench-session",
            )
            event["parent_request_id"] = parent_request_id
            event["batch_id"] = batch_id
            total_event_creation_us += (time.perf_counter() - t0) * 1_000_000

            # Measure serialization
            t1 = time.perf_counter()
            _ = json.dumps(event)
            total_serialization_us += (time.perf_counter() - t1) * 1_000_000

    avg_create_us = total_event_creation_us / (num_calls * iterations)
    avg_serialize_us = total_serialization_us / (num_calls * iterations)

    print(f"  Avg event creation: {avg_create_us:.2f} us")
    print(f"  Avg event serialization: {avg_serialize_us:.2f} us")
    print(f"  Total per-call overhead: {avg_create_us + avg_serialize_us:.2f} us")


# ---------------------------------------------------------------------------
# Bench 4: Funnel query overhead
# ---------------------------------------------------------------------------


def bench_funnel_query(iterations: int = 100) -> None:
    """Measure get_funnel_stats query overhead on a populated table."""
    fd, path = tempfile.mkstemp(suffix=".db")
    os.close(fd)
    try:
        conn = sqlite3.connect(path)
        conn.execute("PRAGMA journal_mode=WAL")
        conn.execute(
            """CREATE TABLE tool_calls (
                request_id TEXT PRIMARY KEY,
                session_id TEXT,
                tool_name TEXT,
                dcc_type TEXT,
                mcp_method TEXT,
                success INTEGER,
                error_kind TEXT,
                started_at_ms INTEGER,
                duration_ms INTEGER
            )"""
        )

        # Populate with diverse data
        methods = ["search", "search", "search", "load_skill", "call", "call", "call", "call_batch"]
        success_vals = [1, 1, 1, 1, 1, 1, 0, 0]
        error_kinds = [
            None,
            None,
            "zero_results",
            None,
            None,
            None,
            "script_fallback",
            "ui_control_fallback",
        ]
        for i in range(1000):
            idx = i % len(methods)
            conn.execute(
                """INSERT OR REPLACE INTO tool_calls
                (request_id, session_id, tool_name, dcc_type, mcp_method,
                 success, error_kind, started_at_ms, duration_ms)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)""",
                (
                    f"req-{i}",
                    "bench-session",
                    f"tool_{i % 20}",
                    "maya" if i % 2 == 0 else "blender",
                    methods[idx],
                    success_vals[idx],
                    error_kinds[idx],
                    now_ms() - i * 1000,
                    i % 100 + 10,
                ),
            )
        conn.commit()

        funnel_sql = """
        SELECT
            COUNT(*) AS total_calls,
            SUM(CASE WHEN mcp_method = 'search' THEN 1 ELSE 0 END) AS searches_total,
            SUM(CASE WHEN mcp_method = 'search' AND error_kind = 'zero_results' THEN 1 ELSE 0 END) AS searches_zero_results,
            SUM(CASE WHEN mcp_method = 'load_skill' AND success = 1 THEN 1 ELSE 0 END) AS skills_loaded,
            SUM(CASE WHEN mcp_method IN ('call', 'call_batch') THEN 1 ELSE 0 END) AS skills_called,
            SUM(CASE WHEN mcp_method IN ('call', 'call_batch') AND success = 1 THEN 1 ELSE 0 END) AS skills_succeeded,
            SUM(CASE WHEN error_kind = 'script_fallback' THEN 1 ELSE 0 END) AS script_fallbacks,
            SUM(CASE WHEN error_kind = 'ui_control_fallback' THEN 1 ELSE 0 END) AS ui_control_fallbacks
        FROM tool_calls
        """

        with Timer(f"funnel query x{iterations}"):
            for _ in range(iterations):
                cursor = conn.execute(funnel_sql)
                _ = cursor.fetchone()

        # Single query timing
        t0 = time.perf_counter()
        cursor = conn.execute(funnel_sql)
        row = cursor.fetchone()
        elapsed_us = (time.perf_counter() - t0) * 1_000_000
        print(f"  Single funnel query: {elapsed_us:.2f} us")
        print(f"  Results: searches={row[1]}, loaded={row[3]}, called={row[4]}, succeeded={row[5]}")

        conn.close()
    finally:
        Path(path).unlink()


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------


def main() -> None:
    parser = argparse.ArgumentParser(description="Observability overhead benchmark")
    parser.add_argument("--iterations", type=int, default=1000, help="Number of iterations per bench")
    parser.add_argument("--batch-size", type=int, default=10, help="Number of sub-calls per batch")
    args = parser.parse_args()

    print("=" * 60)
    print("Observability Collection Overhead Baseline (PIP-2766)")
    print("=" * 60)
    print(f"Python version: {os.sys.version}")
    print(f"Iterations: {args.iterations}")
    print()

    print("--- Bench 1: JSON Serialization ---")
    bench_serialization(args.iterations * 2)
    print()

    print("--- Bench 2: SQLite Write Overhead ---")
    bench_sqlite_write(min(args.iterations, 500))
    print()

    print("--- Bench 3: Batch Dispatch Overhead ---")
    bench_batch_dispatch(num_calls=args.batch_size, iterations=min(args.iterations // 10, 100))
    print()

    print("--- Bench 4: Funnel Query Overhead ---")
    bench_funnel_query(min(args.iterations, 100))
    print()

    print("=" * 60)
    print("Baseline complete. See docs/overhead-baseline.md for analysis.")
    print("=" * 60)


if __name__ == "__main__":
    main()
