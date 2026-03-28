# 中间件

DCC-MCP-Core 包含中间件系统，使用责任链模式在动作执行前后插入自定义逻辑。

## 使用中间件

```python
from dcc_mcp_core.actions.middleware import (
    LoggingMiddleware, PerformanceMiddleware, MiddlewareChain,
)
from dcc_mcp_core.actions.manager import ActionManager

chain = MiddlewareChain()
chain.add(LoggingMiddleware)
chain.add(PerformanceMiddleware, threshold=0.5)

manager = ActionManager("maya", middleware=chain.build())
result = manager.call_action("create_sphere", radius=2.0)
```

## 内置中间件

- **LoggingMiddleware** — 记录动作执行详情和计时
- **PerformanceMiddleware** — 监控执行时间并警告慢动作

## 自定义中间件

```python
from dcc_mcp_core.actions.middleware import Middleware

class CustomMiddleware(Middleware):
    def process(self, action, **kwargs):
        print(f"执行 {action.name} 之前")
        result = super().process(action, **kwargs)
        print(f"执行 {action.name} 之后")
        if result.success:
            result.context["custom_data"] = "由中间件添加"
        return result
```
