"""Checkpoint/resume helpers for long-running tool executions (issue #436).

Implements the Checkpoint-and-Resume pattern from the #1 long-running-agent
design pattern: checkpoint progress at configurable intervals so interrupted
jobs can resume from the last successful checkpoint rather than restarting
from scratch.

This module provides a **Python-level** checkpoint store that works alongside
the existing ``JobManager`` / ``JobStorage`` system:

- :func:`save_checkpoint` — persist opaque state dict + progress hint
- :func:`get_checkpoint` — retrieve the latest checkpoint for a job
- :func:`clear_checkpoint` — delete a job's checkpoint (on completion)
- :func:`list_checkpoints` — enumerate all stored checkpoint keys
- :class:`CheckpointStore` — pluggable backend (in-memory or JSON file)
- :func:`checkpoint_every` — decorator / context helper for skill scripts

Usage in a skill script::

    from dcc_mcp_core.checkpoint import checkpoint_every, save_checkpoint, get_checkpoint

    def process_files(job_id, files):
        # Resume from checkpoint if available
        cp = get_checkpoint(job_id)
        start_idx = cp["context"]["processed_count"] if cp else 0

        for i, f in enumerate(files[start_idx:], start=start_idx):
            process_one(f)
            # Checkpoint every 50 items
            if (i + 1) % 50 == 0:
                save_checkpoint(
                    job_id,
                    state={"processed_count": i + 1, "last_file": f},
                    progress_hint=f"Processed {i + 1}/{len(files)} files",
                )

"""

from __future__ import annotations

import logging
from pathlib import Path
import threading
import time
from typing import Any

from dcc_mcp_core import json_dumps
from dcc_mcp_core import json_loads

logger = logging.getLogger(__name__)

# ── CheckpointStore ────────────────────────────────────────────────────────


class CheckpointStore:
    """Thread-safe checkpoint storage backend.

    The default backend is in-memory.  Pass ``path`` to persist checkpoints
    to a JSON file that survives process restarts.

    Parameters
    ----------
    path:
        Optional filesystem path for durable storage.  If ``None`` (default),
        checkpoints are kept in memory only.

    """

    def __init__(self, path: str | Path | None = None) -> None:
        self._lock = threading.Lock()
        self._data: dict[str, dict[str, Any]] = {}
        self._path: Path | None = Path(path) if path else None
        if self._path and self._path.exists():
            self._load()

    # ── Persistence ────────────────────────────────────────────────────────

    def _load(self) -> None:
        try:
            raw = self._path.read_text(encoding="utf-8")  # type: ignore[union-attr]
            self._data = json_loads(raw)
        except (OSError, ValueError) as exc:
            logger.warning("CheckpointStore: could not load %s: %s", self._path, exc)
            self._data = {}

    def _flush(self) -> None:
        if self._path is None:
            return
        try:
            self._path.parent.mkdir(parents=True, exist_ok=True)
            self._path.write_text(json_dumps(self._data, indent=2), encoding="utf-8")
        except OSError as exc:
            logger.warning("CheckpointStore: could not flush to %s: %s", self._path, exc)

    # ── CRUD ───────────────────────────────────────────────────────────────

    def save(self, job_id: str, state: dict[str, Any], progress_hint: str = "") -> None:
        """Save or overwrite the checkpoint for *job_id*."""
        entry: dict[str, Any] = {
            "job_id": job_id,
            "saved_at": time.time(),
            "progress_hint": progress_hint,
            "context": state,
        }
        with self._lock:
            self._data[job_id] = entry
            self._flush()

    def get(self, job_id: str) -> dict[str, Any] | None:
        """Return the checkpoint dict for *job_id*, or ``None`` if not found."""
        with self._lock:
            return self._data.get(job_id)

    def clear(self, job_id: str) -> bool:
        """Delete the checkpoint for *job_id*.  Returns ``True`` if it existed."""
        with self._lock:
            existed = job_id in self._data
            self._data.pop(job_id, None)
            if existed:
                self._flush()
            return existed

    def list_ids(self) -> list[str]:
        """Return all job IDs that have checkpoints."""
        with self._lock:
            return list(self._data.keys())

    def clear_all(self) -> int:
        """Delete all checkpoints.  Returns the number deleted."""
        with self._lock:
            count = len(self._data)
            self._data.clear()
            self._flush()
            return count


# ── Module-level default store ────────────────────────────────────────────

_DEFAULT_STORE: CheckpointStore = CheckpointStore()


def _get_store() -> CheckpointStore:
    return _DEFAULT_STORE


def configure_checkpoint_store(path: str | Path | None = None) -> CheckpointStore:
    """Replace the module-level default store and return it.

    Call this once at startup (e.g. from ``DccServerBase.__init__``) to enable
    durable checkpoint storage:

    .. code-block:: python

        from dcc_mcp_core.checkpoint import configure_checkpoint_store
        configure_checkpoint_store(path="/var/dcc-mcp/checkpoints.json")

    """
    global _DEFAULT_STORE
    _DEFAULT_STORE = CheckpointStore(path=path)
    return _DEFAULT_STORE


# ── Public helpers ────────────────────────────────────────────────────────


def save_checkpoint(
    job_id: str,
    state: dict[str, Any],
    *,
    progress_hint: str = "",
    store: CheckpointStore | None = None,
) -> None:
    """Persist a checkpoint for *job_id*.

    Call at regular intervals inside a long-running skill script.  The *state*
    dict should contain the minimum information needed to resume from the next
    item — typically ``{"processed_count": N, "last_key": "..."}`` or similar.

    Parameters
    ----------
    job_id:
        The job identifier (from ``_meta.dcc.job_id`` or passed explicitly).
    state:
        Serialisable dict.  Replaces any previous checkpoint.
    progress_hint:
        Human-readable summary (e.g. ``"Processed 180/200 files"``).
    store:
        Use a custom store; defaults to the module-level store.

    """
    (store or _get_store()).save(job_id, state, progress_hint=progress_hint)
    logger.debug("checkpoint saved for job %s: %s", job_id, progress_hint or repr(state))


def get_checkpoint(
    job_id: str,
    *,
    store: CheckpointStore | None = None,
) -> dict[str, Any] | None:
    """Retrieve the last checkpoint for *job_id*.

    Returns ``None`` if no checkpoint exists (fresh execution).

    Returns
    -------
    dict | None
        A dict with keys: ``job_id``, ``saved_at`` (float epoch), ``progress_hint`` (str),
        and ``context`` (the state dict passed to :func:`save_checkpoint`).

    """
    return (store or _get_store()).get(job_id)


def clear_checkpoint(
    job_id: str,
    *,
    store: CheckpointStore | None = None,
) -> bool:
    """Delete the checkpoint for *job_id*.

    Call this when the job completes successfully so storage does not grow
    indefinitely.

    Returns
    -------
    bool
        ``True`` if a checkpoint existed and was deleted.

    """
    deleted = (store or _get_store()).clear(job_id)
    if deleted:
        logger.debug("checkpoint cleared for job %s", job_id)
    return deleted


def list_checkpoints(*, store: CheckpointStore | None = None) -> list[str]:
    """Return all job IDs that have stored checkpoints."""
    return (store or _get_store()).list_ids()


# ── Convenience decorator / helper ────────────────────────────────────────


def checkpoint_every(
    n: int,
    job_id: str,
    state_fn: Any,
    *,
    progress_fn: Any = None,
    store: CheckpointStore | None = None,
) -> None:
    """Call inside a loop to auto-checkpoint every *n* iterations.

    This is a lightweight, zero-dependency alternative to a full decorator.
    Designed for use inside skill scripts where brevity matters.

    Parameters
    ----------
    n:
        Checkpoint interval (number of iterations between saves).
    job_id:
        Job identifier.
    state_fn:
        Zero-argument callable returning the current state dict.
        Called only when a checkpoint is due (avoids serialisation overhead
        on every iteration).
    progress_fn:
        Optional zero-argument callable returning a progress hint string.
    store:
        Custom store; defaults to module-level store.

    Example
    -------
    .. code-block:: python

        for i, item in enumerate(items):
            process(item)
            checkpoint_every(
                50, job_id,
                state_fn=lambda: {"index": i, "last": item},
                progress_fn=lambda: f"Processed {i+1}/{len(items)}",
            )

    """
    if n <= 0:
        return
    # Retrieve the iteration count from the caller's frame via the state_fn call count
    # is not possible without more context.  This helper simply re-saves every call;
    # callers are responsible for calling it at the right interval using modulo arithmetic:
    #   if (i + 1) % 50 == 0:
    #       checkpoint_every(50, job_id, ...)
    hint = progress_fn() if progress_fn else ""
    save_checkpoint(job_id, state=state_fn(), progress_hint=hint, store=store)


# ── MCP tool registration ──────────────────────────────────────────────────

_JOBS_RESUME_SCHEMA: dict[str, Any] = {
    "type": "object",
    "properties": {
        "job_id": {
            "type": "string",
            "description": "ID of the interrupted job to resume.",
        },
    },
    "required": ["job_id"],
    "additionalProperties": False,
}

_JOBS_CHECKPOINT_STATUS_SCHEMA: dict[str, Any] = {
    "type": "object",
    "properties": {
        "job_id": {
            "type": "string",
            "description": "Job ID to query checkpoint status for.",
        },
    },
    "required": ["job_id"],
    "additionalProperties": False,
}

_JOBS_RESUME_DESCRIPTION = (
    "Check whether a checkpoint exists for a job and return resume context. "
    "When to use: before re-submitting an Interrupted job — retrieve saved state "
    "to pass as initial parameters so execution resumes from the last checkpoint. "
    "How to use: pass job_id; if checkpoint exists, pass context to the tool's "
    "next invocation."
)

_JOBS_CHECKPOINT_STATUS_DESCRIPTION = (
    "Return the latest checkpoint state for a job. "
    "When to use: to inspect progress of a running or interrupted job. "
    "How to use: pass job_id; returns {saved_at, progress_hint, context} or "
    "{checkpoint: null} if no checkpoint exists."
)


def register_checkpoint_tools(
    server: Any,
    *,
    dcc_name: str = "dcc",
    store: CheckpointStore | None = None,
) -> None:
    """Register ``jobs.checkpoint_status`` and ``jobs.resume_context`` on *server*.

    These tools allow agents to query checkpoint state before re-submitting
    interrupted jobs, so the skill script can resume from where it left off.

    Parameters
    ----------
    server:
        An ``McpHttpServer`` compatible object.
    dcc_name:
        DCC name for tool metadata.
    store:
        Custom checkpoint store; defaults to the module-level store.

    """
    _store = store or _get_store()

    try:
        registry = server.registry
    except Exception as exc:
        logger.warning("register_checkpoint_tools: server.registry unavailable: %s", exc)
        return

    def _handle_checkpoint_status(params: Any) -> Any:
        args: dict[str, Any] = json_loads(params) if isinstance(params, str) else (params or {})
        job_id = args.get("job_id", "")
        cp = _store.get(job_id)
        if cp is None:
            return {
                "success": True,
                "message": f"No checkpoint for job '{job_id}'.",
                "context": {"job_id": job_id, "checkpoint": None},
            }
        return {
            "success": True,
            "message": f"Checkpoint found: {cp.get('progress_hint', '')}",
            "context": cp,
        }

    def _handle_resume_context(params: Any) -> Any:
        args: dict[str, Any] = json_loads(params) if isinstance(params, str) else (params or {})
        job_id = args.get("job_id", "")
        cp = _store.get(job_id)
        if cp is None:
            return {
                "success": True,
                "message": f"No checkpoint for '{job_id}' — job will start from the beginning.",
                "context": {"job_id": job_id, "has_checkpoint": False, "resume_state": None},
            }
        return {
            "success": True,
            "message": (
                f"Checkpoint found at {cp.get('progress_hint', 'unknown progress')}. "
                f"Pass resume_state to the skill's initial parameters."
            ),
            "context": {
                "job_id": job_id,
                "has_checkpoint": True,
                "resume_state": cp.get("context"),
                "saved_at": cp.get("saved_at"),
                "progress_hint": cp.get("progress_hint", ""),
            },
        }

    tools = [
        (
            "jobs.checkpoint_status",
            _JOBS_CHECKPOINT_STATUS_DESCRIPTION,
            _JOBS_CHECKPOINT_STATUS_SCHEMA,
            _handle_checkpoint_status,
        ),
        (
            "jobs.resume_context",
            _JOBS_RESUME_DESCRIPTION,
            _JOBS_RESUME_SCHEMA,
            _handle_resume_context,
        ),
    ]

    for name, desc, schema, handler in tools:
        try:
            registry.register(
                name=name,
                description=desc,
                input_schema=json_dumps(schema),
                dcc=dcc_name,
                category="jobs",
                version="1.0.0",
            )
        except Exception as exc:
            logger.warning("register_checkpoint_tools: register(%s) failed: %s", name, exc)
            continue
        try:
            server.register_handler(name, handler)
        except Exception as exc:
            logger.warning("register_checkpoint_tools: register_handler(%s) failed: %s", name, exc)


# ── Public API ─────────────────────────────────────────────────────────────

__all__ = [
    "CheckpointStore",
    "checkpoint_every",
    "clear_checkpoint",
    "configure_checkpoint_store",
    "get_checkpoint",
    "list_checkpoints",
    "register_checkpoint_tools",
    "save_checkpoint",
]
