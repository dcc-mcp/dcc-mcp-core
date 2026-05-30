# 弱 DCC 执行防护栏

`DccWeakSandbox` 是一个可选的、有界的辅助工具，用于在执行生成的 DCC 代码时阻止已知危险的宿主操作。

它**不是**安全沙箱。它仅在 `with` 块期间替换适配器选择的 Python 属性/函数，并在块退出时恢复它们。

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

适配器也可以提供显式的属性覆盖：

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

被阻止的调用会引发带有被阻止调用名称和原因的 `DccGuardrailError`。将此用于实际的适配器防护栏，如退出/出厂重置/首选项重置操作；不要将其视为对恶意代码的保护。
