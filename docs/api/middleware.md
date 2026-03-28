# Middleware API

`dcc_mcp_core.actions.middleware`

## Middleware Base Class

```python
class Middleware:
    def process(self, action: Action, **kwargs) -> ActionResultModel:
        """Process an action. Call super().process() to pass to next in chain."""
```

## Built-in Middleware

### LoggingMiddleware

Logs action name, timing, success/failure.

### PerformanceMiddleware

```python
PerformanceMiddleware(threshold=1.0)
```

Adds `context["performance"]["execution_time"]`, warns if exceeds threshold.

## MiddlewareChain

```python
chain = MiddlewareChain()
chain.add(LoggingMiddleware)
chain.add(PerformanceMiddleware, threshold=0.5)
chain.add(CustomMiddleware)
first_middleware = chain.build()  # Returns first middleware in chain
```
