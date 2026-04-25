# Cancellation API

Cooperative cancellation support for DCC-MCP skill scripts (issue #318, #332).

Skill scripts executed inside a `tools/call` request run as regular Python code and cannot be interrupted by the dispatcher. The MCP spec's `notifications/cancelled` message only helps if the running code checks for cancellation at appropriate points.

**Exported symbols:** `CancelToken`, `CancelledError`, `check_cancelled`, `current_cancel_token`, `reset_cancel_token`, `set_cancel_token`

## CancelToken

Thread-safe cancellation flag settable by the request dispatcher.

```python
from dcc_mcp_core import CancelToken

token = CancelToken()
token.cancelled  # False
token.cancel()
token.cancelled  # True
```

### Constructor

| Parameter | Type | Description |
|-----------|------|-------------|
| (none) | | Creates a new un-cancelled token |

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `cancel()` | `None` | Mark the token as cancelled. Idempotent. |
| `cancelled` (property) | `bool` | Whether `cancel()` has been invoked |

## CancelledError

```python
from dcc_mcp_core import CancelledError
```

Raised by `check_cancelled()` when the active request was cancelled. Deliberately a plain `Exception` subclass — the `@skill_entry` decorator's generic `except Exception` branch will convert an unhandled `CancelledError` into a standard skill error dict.

## check_cancelled

```python
check_cancelled() -> None
```

Raise `CancelledError` if the active request has been cancelled. No-op when invoked outside of a request context (e.g. from a REPL or unit test).

| Parameter | Type | Description |
|-----------|------|-------------|
| (none) | | |

**Raises:** `CancelledError` — if a `CancelToken` is installed and its `cancelled` property is `True`.

```python
from dcc_mcp_core import check_cancelled, skill_success

def run(iterations: int = 100) -> dict:
    for _ in range(iterations):
        check_cancelled()  # raises CancelledError when cancelled
        do_one_unit_of_work()
    return skill_success("done")
```

## set_cancel_token

```python
set_cancel_token(token: CancelToken | None) -> contextvars.Token
```

Install a `CancelToken` as the active cancel token for the current context. For **dispatcher** use only — skill authors should call `check_cancelled()` instead.

| Parameter | Type | Description |
|-----------|------|-------------|
| `token` | `CancelToken \| None` | The token to install, or `None` to clear |

**Returns:** A `contextvars.Token` that must be passed to `reset_cancel_token`.

## reset_cancel_token

```python
reset_cancel_token(reset: contextvars.Token) -> None
```

Restore the cancel-token contextvar to its previous value.

| Parameter | Type | Description |
|-----------|------|-------------|
| `reset` | `contextvars.Token` | The token returned by `set_cancel_token` |

## current_cancel_token

```python
current_cancel_token() -> CancelToken | None
```

Return the `CancelToken` installed in the current context, or `None` when no dispatcher has installed one.

::: tip
Use `current_cancel_token()` to poll the cancellation flag without raising, e.g. to flush partial progress before returning.
:::
