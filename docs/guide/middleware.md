# Middleware

DCC-MCP-Core includes a middleware system for inserting custom logic before and after action execution using the chain-of-responsibility pattern.

## Using Middleware

```python
from dcc_mcp_core.actions.middleware import (
    LoggingMiddleware,
    PerformanceMiddleware,
    MiddlewareChain,
)
from dcc_mcp_core.actions.manager import ActionManager

# Create a middleware chain
chain = MiddlewareChain()

# Add middleware (order matters — first added is executed first)
chain.add(LoggingMiddleware)
chain.add(PerformanceMiddleware, threshold=0.5)

# Create an action manager with the middleware chain
manager = ActionManager("maya", middleware=chain.build())

# Execute actions through the middleware chain
result = manager.call_action("create_sphere", radius=2.0)

# Result includes performance data
print(f"Execution time: {result.context['performance']['execution_time']:.2f}s")
```

## Built-in Middleware

### LoggingMiddleware

Logs action execution details and timing information.

### PerformanceMiddleware

Monitors execution time and warns about slow actions.

```python
chain.add(PerformanceMiddleware, threshold=0.5)  # warn if > 0.5s
```

Adds `context["performance"]["execution_time"]` to the result.

## Custom Middleware

Create custom middleware by inheriting from the `Middleware` base class:

```python
from dcc_mcp_core.actions.middleware import Middleware
from dcc_mcp_core.actions.base import Action
from dcc_mcp_core.models import ActionResultModel

class CustomMiddleware(Middleware):
    def process(self, action: Action, **kwargs) -> ActionResultModel:
        # Pre-processing
        print(f"Before executing {action.name}")

        # Call next middleware in chain (or the action itself)
        result = super().process(action, **kwargs)

        # Post-processing
        print(f"After executing {action.name}: {'Success' if result.success else 'Failed'}")

        # Modify result if needed
        if result.success:
            result.context["custom_data"] = "Added by middleware"

        return result
```

## MiddlewareChain

```python
from dcc_mcp_core.actions.middleware import MiddlewareChain

chain = MiddlewareChain()
chain.add(LoggingMiddleware)
chain.add(PerformanceMiddleware, threshold=0.5)
chain.add(CustomMiddleware)
first_middleware = chain.build()  # Returns the first middleware in chain
```
