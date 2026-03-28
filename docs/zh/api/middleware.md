# 中间件 API

详见 [英文 API 文档](/api/middleware) 获取完整参考。

```python
from dcc_mcp_core.actions.middleware import MiddlewareChain, LoggingMiddleware, PerformanceMiddleware

chain = MiddlewareChain()
chain.add(LoggingMiddleware)
chain.add(PerformanceMiddleware, threshold=0.5)
first_middleware = chain.build()
```
