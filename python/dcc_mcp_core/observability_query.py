"""Versioned aggregation and query API for observability data.

Provides a Python-level query interface for aggregated metrics, sessions,
tool-call events, and statistics.  Compatible with Python 3.7.

The API version is embedded in responses so consumers can detect schema
changes without breaking.
"""

from __future__ import annotations

import json
import logging
import time
from typing import Any
from typing import Callable

logger = logging.getLogger(__name__)

# Current API version. Increment when the response schema changes in a
# backward-incompatible way.
API_VERSION = "v1"

__all__ = [
    "API_VERSION",
    "ObservabilityQuery",
    "build_query_response",
]


def _now_ms() -> int:
    """Return current wall-clock time in milliseconds."""
    return int(time.time() * 1000)


def build_query_response(
    query_type: str,
    data: dict[str, Any],
    *,
    api_version: str = API_VERSION,
    query_params: dict[str, Any] | None = None,
    warnings: list[str] | None = None,
) -> dict[str, Any]:
    """Wrap query results in a versioned response envelope.

    Args:
        query_type: Short machine-readable query name (e.g. ``"session_stats"``).
        data: The query result payload.
        api_version: API schema version (default ``API_VERSION``).
        query_params: The parameters used for this query (for debugging).
        warnings: Any non-fatal warnings for the consumer.

    Returns:
        A versioned response dict::

            {
                "api_version": "v1",
                "query_type": "session_stats",
                "timestamp_ms": 1700000000000,
                "data": { ... },
                "query_params": { ... },
                "warnings": [ ... ],
            }

    """
    envelope: dict[str, Any] = {
        "api_version": api_version,
        "query_type": query_type,
        "timestamp_ms": _now_ms(),
        "data": data,
    }
    if query_params is not None:
        envelope["query_params"] = query_params
    if warnings:
        envelope["warnings"] = warnings
    return envelope


class ObservabilityQuery:
    """Query interface for the observability storage backend.

    Provides methods to read aggregated metrics, session data, tool-call
    events, and statistics from the SQLite observability store.

    This is the Python-level query API consumed by the Admin UI, CLI, and
    automation.  The backing store is the gateway admin SQLite database
    (shared across all DCC instances on a machine).

    """

    def __init__(
        self,
        read_json_fn: Callable[[str, dict[str, Any]], list[dict[str, Any]]] | None = None,
        db_path: str | None = None,
    ) -> None:
        """Initialize the query interface.

        Args:
            read_json_fn: A callable that takes a SQL query string and a
                params dict, and returns a list of dicts.  This allows the
                caller to inject a real SQLite connection or a mock for testing.
            db_path: Path to the gateway admin SQLite database.  Used for
                informational purposes only.

        """
        self._read_fn = read_json_fn
        self._db_path = db_path

    def _query(self, sql: str, params: dict[str, Any] | None = None) -> list[dict[str, Any]]:
        """Execute a SQL query via the injected read function."""
        if self._read_fn is None:
            return []
        try:
            return self._read_fn(sql, params or {})
        except Exception as exc:
            logger.warning("ObservabilityQuery query failed: %s", exc)
            return []

    def get_session_stats(
        self,
        *,
        dcc_type: str | None = None,
        since_ms: int | None = None,
        until_ms: int | None = None,
    ) -> dict[str, Any]:
        """Get aggregated session statistics.

        Args:
            dcc_type: Filter by DCC type (e.g. ``"maya"``, ``"blender"``).
            since_ms: Start of time range (milliseconds since epoch).
            until_ms: End of time range (milliseconds since epoch).

        Returns:
            Versioned response with ``SessionStats`` data.

        """
        conditions = []
        params: dict[str, Any] = {}
        if dcc_type:
            conditions.append("dcc_type = :dcc_type")
            params["dcc_type"] = dcc_type
        if since_ms is not None:
            conditions.append("started_at_ms >= :since_ms")
            params["since_ms"] = since_ms
        if until_ms is not None:
            conditions.append("started_at_ms <= :until_ms")
            params["until_ms"] = until_ms
        where = ("WHERE " + " AND ".join(conditions)) if conditions else ""

        sql = f"""
        SELECT
            COUNT(*) AS total_sessions,
            SUM(CASE WHEN status = 'active' THEN 1 ELSE 0 END) AS active_sessions,
            SUM(CASE WHEN status IN ('ended', '"Ended"') THEN 1 ELSE 0 END) AS ended_normally,
            SUM(CASE WHEN status IN (
                'crashed', '"Crashed"', 'disconnected', '"Disconnected"',
                'gpu_crashed', '"GpuCrashed"', 'timed_out', '"TimedOut"',
                'cancelled', '"Cancelled"', 'thread_affinity_failure',
                '"ThreadAffinityFailure"'
            ) THEN 1 ELSE 0 END) AS ended_abnormally,
            COALESCE(AVG(ended_at_ms - started_at_ms), 0) AS avg_duration_ms,
            COALESCE(SUM(tool_call_count), 0) AS total_tool_calls,
            COALESCE(SUM(error_count), 0) AS total_errors
        FROM sessions
        {where}
        """
        rows = self._query(sql, params)
        data: dict[str, Any] = {
            "total_sessions": 0,
            "active_sessions": 0,
            "ended_normally": 0,
            "ended_abnormally": 0,
            "avg_duration_ms": 0.0,
            "total_tool_calls": 0,
            "total_errors": 0,
        }
        if rows:
            row = rows[0]
            data = {
                "total_sessions": int(row.get("total_sessions", 0)),
                "active_sessions": int(row.get("active_sessions", 0)),
                "ended_normally": int(row.get("ended_normally", 0)),
                "ended_abnormally": int(row.get("ended_abnormally", 0)),
                "avg_duration_ms": float(row.get("avg_duration_ms", 0.0)),
                "total_tool_calls": int(row.get("total_tool_calls", 0)),
                "total_errors": int(row.get("total_errors", 0)),
            }
        return build_query_response("session_stats", data, query_params=params)

    def get_tool_call_stats(
        self,
        *,
        tool_name: str | None = None,
        dcc_type: str | None = None,
        session_id: str | None = None,
        since_ms: int | None = None,
        until_ms: int | None = None,
        limit: int = 100,
    ) -> dict[str, Any]:
        """Get aggregated tool-call statistics.

        Returns:
            Versioned response with ``ToolCallStats`` data and a list of
            individual ``ToolCallEvent`` records.

        """
        conditions = []
        params: dict[str, Any] = {}
        if tool_name:
            conditions.append("tool_name = :tool_name")
            params["tool_name"] = tool_name
        if dcc_type:
            conditions.append("dcc_type = :dcc_type")
            params["dcc_type"] = dcc_type
        if session_id:
            conditions.append("session_id = :session_id")
            params["session_id"] = session_id
        if since_ms is not None:
            conditions.append("started_at_ms >= :since_ms")
            params["since_ms"] = since_ms
        if until_ms is not None:
            conditions.append("started_at_ms <= :until_ms")
            params["until_ms"] = until_ms
        where = ("WHERE " + " AND ".join(conditions)) if conditions else ""

        # Aggregated stats
        agg_sql = f"""
        SELECT
            COUNT(*) AS total_calls,
            SUM(CASE WHEN success = 1 THEN 1 ELSE 0 END) AS success_count,
            SUM(CASE WHEN success = 0 THEN 1 ELSE 0 END) AS failure_count,
            COALESCE(AVG(duration_ms), 0) AS avg_duration_ms,
            COALESCE(AVG(CASE WHEN success = 1 THEN duration_ms END), 0) AS avg_success_duration_ms
        FROM tool_calls
        {where}
        """
        rows = self._query(agg_sql, params)
        stats: dict[str, Any] = {
            "total_calls": 0,
            "success_count": 0,
            "failure_count": 0,
            "success_rate": 0.0,
            "avg_duration_ms": 0.0,
        }
        if rows:
            row = rows[0]
            total = int(row.get("total_calls", 0))
            success = int(row.get("success_count", 0))
            stats = {
                "total_calls": total,
                "success_count": success,
                "failure_count": int(row.get("failure_count", 0)),
                "success_rate": success / total if total > 0 else 0.0,
                "avg_duration_ms": float(row.get("avg_duration_ms", 0.0)),
            }

        # Recent individual events
        events_sql = f"""
        SELECT request_id, session_id, parent_request_id, batch_id,
               tool_name, skill_name, dcc_type, instance_id, agent_id,
               transport, via_gateway, started_at_ms, duration_ms,
               success, error_message, error_kind, mcp_method, trace_id, span_id
        FROM tool_calls
        {where}
        ORDER BY started_at_ms DESC
        LIMIT :limit
        """
        params["limit"] = max(1, min(limit, 1000))
        events = self._query(events_sql, params)

        return build_query_response(
            "tool_call_stats",
            {"stats": stats, "events": events},
            query_params=params,
        )

    def get_repeated_scripts(
        self,
        *,
        min_repeats: int = 3,
        since_ms: int | None = None,
        until_ms: int | None = None,
        dcc_type: str | None = None,
        tool_name: str | None = None,
        limit: int = 100,
    ) -> dict[str, Any]:
        """Return repeated materialized scripts and review-only promotion candidates.

        Script source is never selected. Evidence comes from the redaction-safe
        ``script_execution`` identity stored in durable audit JSON rows.
        """
        if isinstance(min_repeats, bool) or min_repeats < 2:
            raise ValueError("min_repeats must be an integer greater than or equal to 2")
        if isinstance(limit, bool) or limit < 1:
            raise ValueError("limit must be a positive integer")

        params: dict[str, Any] = {
            "min_repeats": int(min_repeats),
            "limit": min(int(limit), 1000),
        }
        conditions = [
            "json_valid(audit_json)",
            "json_extract(audit_json, '$.script_execution.sha256') IS NOT NULL",
        ]
        if since_ms is not None:
            conditions.append("ts_ms >= :since_ms")
            params["since_ms"] = int(since_ms)
        if until_ms is not None:
            conditions.append("ts_ms <= :until_ms")
            params["until_ms"] = int(until_ms)
        if dcc_type:
            conditions.append("json_extract(audit_json, '$.dcc_type') = :dcc_type")
            params["dcc_type"] = dcc_type
        if tool_name:
            conditions.append("json_extract(audit_json, '$.action') = :tool_name")
            params["tool_name"] = tool_name

        where = " AND ".join(conditions)
        sql = f"""
            SELECT
                json_extract(audit_json, '$.script_execution.sha256') AS sha256,
                json_extract(audit_json, '$.script_execution.reuse_key') AS reuse_key,
                json_extract(audit_json, '$.dcc_type') AS dcc_type,
                json_extract(audit_json, '$.action') AS tool_name,
                COUNT(*) AS execution_count,
                COUNT(DISTINCT json_extract(audit_json, '$.session_id')) AS session_count,
                SUM(CASE
                    WHEN json_extract(audit_json, '$.script_execution.reused') = 1 THEN 1
                    ELSE 0 END) AS reused_count,
                SUM(CASE
                    WHEN COALESCE(json_extract(audit_json, '$.script_execution.reused'), 0) = 0
                    THEN 1 ELSE 0 END) AS rematerialized_count,
                MIN(ts_ms) AS first_seen_ms,
                MAX(ts_ms) AS last_seen_ms
            FROM audits
            WHERE {where}
            GROUP BY sha256, reuse_key, dcc_type, tool_name
            HAVING COUNT(*) >= :min_repeats
            ORDER BY execution_count DESC, last_seen_ms DESC
            LIMIT :limit
            """
        if self._read_fn is None:
            rows = []
        else:
            try:
                rows = self._read_fn(sql, params)
            except Exception as exc:
                if "no such function: json" in str(exc).lower():
                    logger.warning("SQLite JSON1 unavailable; using portable audit aggregation")
                    try:
                        rows = _repeated_script_rows_without_json1(
                            self._read_fn,
                            min_repeats=int(min_repeats),
                            limit=params["limit"],
                            since_ms=since_ms,
                            until_ms=until_ms,
                            dcc_type=dcc_type,
                            tool_name=tool_name,
                        )
                    except Exception as fallback_exc:
                        logger.warning(
                            "ObservabilityQuery portable audit aggregation failed: %s",
                            fallback_exc,
                        )
                        rows = []
                else:
                    logger.warning(
                        "ObservabilityQuery repeated-script query failed: %s",
                        exc,
                    )
                    rows = []

        scripts = [_repeated_script_evidence(row) for row in rows]
        candidates = [
            {
                "candidate_id": f"script:{row['sha256']}",
                "decision": "manual_review",
                "recommended_action": "review_skill_improvement",
                "evidence": row,
            }
            for row in scripts
        ]
        return build_query_response(
            "repeated_scripts",
            {"scripts": scripts, "promotion_candidates": candidates},
            query_params=params,
        )

    def get_session_tree(
        self,
        root_session_id: str | None = None,
        *,
        dcc_type: str | None = None,
        max_depth: int = 10,
    ) -> dict[str, Any]:
        """Get a session parent-child tree.

        Args:
            root_session_id: The root session to start from. If None, returns
                all root sessions (those with no parent).
            dcc_type: Filter by DCC type.
            max_depth: Maximum depth of the tree.

        Returns:
            Versioned response with a nested session tree structure.

        """
        if root_session_id:
            sql = """
            SELECT session_id, parent_session_id, dcc_type, instance_id,
                   status, started_at_ms, last_activity_at_ms, ended_at_ms,
                   tool_call_count, error_count, core_version
            FROM sessions
            WHERE session_id = :root_id
               OR parent_session_id = :root_id
            ORDER BY started_at_ms ASC
            """
            rows = self._query(sql, {"root_id": root_session_id})
        else:
            conditions = ["parent_session_id IS NULL"]
            params: dict[str, Any] = {}
            if dcc_type:
                conditions.append("dcc_type = :dcc_type")
                params["dcc_type"] = dcc_type
            where = " AND ".join(conditions)
            sql = f"""
            SELECT session_id, parent_session_id, dcc_type, instance_id,
                   status, started_at_ms, last_activity_at_ms, ended_at_ms,
                   tool_call_count, error_count, core_version
            FROM sessions
            WHERE {where}
            ORDER BY started_at_ms DESC
            LIMIT 50
            """
            rows = self._query(sql, params)

        # Build tree from flat rows
        tree = _build_session_tree(rows, max_depth)
        return build_query_response("session_tree", {"tree": tree})

    def get_coverage_stats(
        self,
        *,
        since_ms: int | None = None,
        until_ms: int | None = None,
    ) -> dict[str, Any]:
        """Get observability coverage statistics.

        Computes the ratio of observed (gateway) to total requests.

        """
        conditions = []
        params: dict[str, Any] = {}
        if since_ms is not None:
            conditions.append("started_at_ms >= :since_ms")
            params["since_ms"] = since_ms
        if until_ms is not None:
            conditions.append("started_at_ms <= :until_ms")
            params["until_ms"] = until_ms
        where = ("WHERE " + " AND ".join(conditions)) if conditions else ""

        sql = f"""
        SELECT
            SUM(CASE WHEN via_gateway = 1 THEN 1 ELSE 0 END) AS observed,
            SUM(CASE WHEN via_gateway = 0 OR via_gateway IS NULL THEN 1 ELSE 0 END) AS unobserved,
            COUNT(*) AS total
        FROM tool_calls
        {where}
        """
        rows = self._query(sql, params)
        data: dict[str, Any] = {
            "observed_requests": 0,
            "unobserved_requests": 0,
            "coverage_ratio": 0.0,
        }
        if rows:
            row = rows[0]
            observed = int(row.get("observed", 0))
            unobserved = int(row.get("unobserved", 0))
            total = int(row.get("total", 0))
            data = {
                "observed_requests": observed,
                "unobserved_requests": unobserved,
                "coverage_ratio": observed / total if total > 0 else 0.0,
            }
        return build_query_response("coverage_stats", data, query_params=params)

    def get_crash_stats(
        self,
        *,
        dcc_type: str | None = None,
        since_ms: int | None = None,
    ) -> dict[str, Any]:
        """Get crash and stability statistics."""
        conditions = [
            "status IN ('crashed', '\"Crashed\"', 'gpu_crashed', '\"GpuCrashed\"', 'disconnected', '\"Disconnected\"')",
        ]
        params: dict[str, Any] = {}
        if dcc_type:
            conditions.append("dcc_type = :dcc_type")
            params["dcc_type"] = dcc_type
        if since_ms is not None:
            conditions.append("started_at_ms >= :since_ms")
            params["since_ms"] = since_ms
        where = " AND ".join(conditions)

        sql = f"""
        SELECT
            COUNT(*) AS total_crashes,
            SUM(CASE WHEN status IN ('crashed', '\"Crashed\"') THEN 1 ELSE 0 END) AS host_crashes,
            SUM(CASE WHEN status IN ('gpu_crashed', '\"GpuCrashed\"') THEN 1 ELSE 0 END) AS gpu_crashes
        FROM sessions
        WHERE {where}
        """
        rows = self._query(sql, params)
        data: dict[str, Any] = {
            "total_crashes": 0,
            "host_crashes": 0,
            "gpu_crashes": 0,
        }
        if rows:
            row = rows[0]
            data = {
                "total_crashes": int(row.get("total_crashes", 0)),
                "host_crashes": int(row.get("host_crashes", 0)),
                "gpu_crashes": int(row.get("gpu_crashes", 0)),
            }
        return build_query_response("crash_stats", data, query_params=params)

    def get_funnel_stats(
        self,
        *,
        dcc_type: str | None = None,
        since_ms: int | None = None,
    ) -> dict[str, Any]:
        """Get capability funnel statistics.

        Queries the tool_calls table for real funnel data: search → load →
        call → success, plus fallback paths.  Falls back to zeroes when the
        underlying instrumentation has not yet recorded matching events.

        """
        params: dict[str, Any] = {}
        if dcc_type:
            params["dcc_type"] = dcc_type
        if since_ms is not None:
            params["since_ms"] = since_ms

        conditions = []
        if dcc_type:
            conditions.append("dcc_type = :dcc_type")
        if since_ms is not None:
            conditions.append("started_at_ms >= :since_ms")
        where = ("WHERE " + " AND ".join(conditions)) if conditions else ""

        # Aggregate all funnel stages in one query.
        funnel_sql = f"""
        SELECT
            COUNT(*) AS total_calls,
            SUM(CASE WHEN mcp_method = 'search' THEN 1 ELSE 0 END) AS searches_total,
            SUM(CASE
                WHEN mcp_method = 'search'
                AND error_kind = 'zero_results'
                THEN 1 ELSE 0 END) AS searches_zero_results,
            SUM(CASE
                WHEN mcp_method = 'load_skill'
                AND success = 1
                THEN 1 ELSE 0 END) AS skills_loaded,
            SUM(CASE
                WHEN mcp_method IN ('call', 'call_batch')
                THEN 1 ELSE 0 END) AS skills_called,
            SUM(CASE
                WHEN mcp_method IN ('call', 'call_batch')
                AND success = 1
                THEN 1 ELSE 0 END) AS skills_succeeded,
            SUM(CASE
                WHEN error_kind = 'script_fallback'
                THEN 1 ELSE 0 END) AS script_fallbacks,
            SUM(CASE
                WHEN error_kind = 'ui_control_fallback'
                THEN 1 ELSE 0 END) AS ui_control_fallbacks,
            COALESCE(MIN(CASE
                WHEN mcp_method IN ('call', 'call_batch')
                AND success = 1
                THEN duration_ms END), 0) AS first_success_duration_ms
        FROM tool_calls
        {where}
        """
        rows = self._query(funnel_sql, params)

        data: dict[str, Any] = {
            "searches_total": 0,
            "searches_zero_results": 0,
            "skills_loaded": 0,
            "skills_called": 0,
            "skills_succeeded": 0,
            "script_fallbacks": 0,
            "ui_control_fallbacks": 0,
            "first_success_duration_ms": 0,
        }
        if rows:
            row = rows[0]
            data = {
                "searches_total": int(row.get("searches_total", 0)),
                "searches_zero_results": int(row.get("searches_zero_results", 0)),
                "skills_loaded": int(row.get("skills_loaded", 0)),
                "skills_called": int(row.get("skills_called", 0)),
                "skills_succeeded": int(row.get("skills_succeeded", 0)),
                "script_fallbacks": int(row.get("script_fallbacks", 0)),
                "ui_control_fallbacks": int(row.get("ui_control_fallbacks", 0)),
                "first_success_duration_ms": int(row.get("first_success_duration_ms", 0)),
            }
        return build_query_response("funnel_stats", data, query_params=params)


def _build_session_tree(
    rows: list[dict[str, Any]],
    max_depth: int,
) -> list[dict[str, Any]]:
    """Build a nested tree from flat session rows."""
    if not rows:
        return []

    # Index by session_id
    lookup: dict[str, dict[str, Any]] = {}
    for row in rows:
        sid = row.get("session_id", "")
        if sid:
            lookup[sid] = {
                "session_id": sid,
                "parent_session_id": row.get("parent_session_id"),
                "dcc_type": row.get("dcc_type"),
                "instance_id": row.get("instance_id"),
                "status": row.get("status"),
                "started_at_ms": row.get("started_at_ms"),
                "last_activity_at_ms": row.get("last_activity_at_ms"),
                "ended_at_ms": row.get("ended_at_ms"),
                "tool_call_count": row.get("tool_call_count", 0),
                "error_count": row.get("error_count", 0),
                "core_version": row.get("core_version"),
                "children": [],
            }

    roots: list[dict[str, Any]] = []
    for _sid, node in lookup.items():
        parent = node.get("parent_session_id")
        if parent and parent in lookup:
            lookup[parent]["children"].append(node)
        else:
            roots.append(node)

    return roots


def _repeated_script_evidence(row: dict[str, Any]) -> dict[str, Any]:
    """Normalize the allowlisted, source-free promotion evidence shape."""
    return {
        "sha256": str(row.get("sha256", "")),
        "reuse_key": row.get("reuse_key"),
        "dcc_type": row.get("dcc_type"),
        "tool_name": row.get("tool_name"),
        "execution_count": int(row.get("execution_count", 0)),
        "session_count": int(row.get("session_count", 0)),
        "reused_count": int(row.get("reused_count", 0)),
        "rematerialized_count": int(row.get("rematerialized_count", 0)),
        "first_seen_ms": int(row.get("first_seen_ms", 0)),
        "last_seen_ms": int(row.get("last_seen_ms", 0)),
    }


def _repeated_script_rows_without_json1(
    read_fn: Callable[[str, dict[str, Any]], list[dict[str, Any]]],
    *,
    min_repeats: int,
    limit: int,
    since_ms: int | None,
    until_ms: int | None,
    dcc_type: str | None,
    tool_name: str | None,
) -> list[dict[str, Any]]:
    """Aggregate source-free script identities on SQLite builds without JSON1."""
    conditions = []
    params: dict[str, Any] = {}
    if since_ms is not None:
        conditions.append("ts_ms >= :since_ms")
        params["since_ms"] = int(since_ms)
    if until_ms is not None:
        conditions.append("ts_ms <= :until_ms")
        params["until_ms"] = int(until_ms)
    where = (" WHERE " + " AND ".join(conditions)) if conditions else ""
    audit_rows = read_fn(
        f"SELECT ts_ms, audit_json FROM audits{where} ORDER BY ts_ms ASC",
        params,
    )

    grouped: dict[tuple[Any, ...], dict[str, Any]] = {}
    for row in audit_rows:
        raw_json = row.get("audit_json")
        if not isinstance(raw_json, (str, bytes, bytearray)):
            continue
        try:
            audit = json.loads(raw_json)
        except (TypeError, ValueError):
            continue
        if not isinstance(audit, dict):
            continue
        script = audit.get("script_execution")
        if not isinstance(script, dict):
            continue
        sha256 = script.get("sha256")
        if not isinstance(sha256, str) or not sha256:
            continue
        audit_dcc = audit.get("dcc_type")
        audit_tool = audit.get("action")
        if dcc_type and audit_dcc != dcc_type:
            continue
        if tool_name and audit_tool != tool_name:
            continue
        reuse_key = script.get("reuse_key")
        if not isinstance(reuse_key, str):
            reuse_key = None
        if not isinstance(audit_dcc, str):
            audit_dcc = None
        if not isinstance(audit_tool, str):
            audit_tool = None
        try:
            timestamp_ms = int(row.get("ts_ms", 0))
        except (TypeError, ValueError):
            continue

        key = (sha256, reuse_key, audit_dcc, audit_tool)
        evidence = grouped.setdefault(
            key,
            {
                "sha256": sha256,
                "reuse_key": reuse_key,
                "dcc_type": audit_dcc,
                "tool_name": audit_tool,
                "execution_count": 0,
                "_sessions": set(),
                "reused_count": 0,
                "rematerialized_count": 0,
                "first_seen_ms": timestamp_ms,
                "last_seen_ms": timestamp_ms,
            },
        )
        evidence["execution_count"] += 1
        session_id = audit.get("session_id")
        if isinstance(session_id, str):
            evidence["_sessions"].add(session_id)
        if script.get("reused") is True:
            evidence["reused_count"] += 1
        else:
            evidence["rematerialized_count"] += 1
        evidence["first_seen_ms"] = min(evidence["first_seen_ms"], timestamp_ms)
        evidence["last_seen_ms"] = max(evidence["last_seen_ms"], timestamp_ms)

    rows = []
    for evidence in grouped.values():
        if evidence["execution_count"] < min_repeats:
            continue
        evidence["session_count"] = len(evidence.pop("_sessions"))
        rows.append(evidence)
    rows.sort(
        key=lambda item: (item["execution_count"], item["last_seen_ms"]),
        reverse=True,
    )
    return rows[:limit]
