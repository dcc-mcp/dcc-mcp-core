# Weak DCC Execution Guardrails

`DccWeakSandbox` is an opt-in, scoped helper for blocking known-dangerous host
operations while executing generated DCC code.

It is **not** a security sandbox. It only replaces adapter-selected Python
attributes/functions for the duration of a `with` block and restores them when
the block exits.

```python
import sys
from dcc_mcp_core import DccBlockedCall, DccWeakSandbox

with DccWeakSandbox(
    blocked_calls=[
        DccBlockedCall(
            "sys.exit",
            "terminates the embedded DCC Python process",
            target=sys,
            attribute="exit",
        ),
    ],
):
    exec(code, namespace)
```

Adapters can also provide explicit attribute overrides:

```python
with DccWeakSandbox(
    attr_overrides={
        sys: {
            "exit": DccWeakSandbox.blocked_callable(
                "sys.exit",
                "terminates the host process",
            ),
        },
    },
):
    exec(code, namespace)
```

Blocked calls raise `DccGuardrailError` with the blocked call name and reason.
Use this for practical adapter guardrails such as quit/factory-reset/preference
reset operations; do not treat it as protection from malicious code.
