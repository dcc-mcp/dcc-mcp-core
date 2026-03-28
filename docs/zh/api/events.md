# 事件 API

详见 [英文 API 文档](/api/events) 获取完整参考。

```python
from dcc_mcp_core.actions.events import event_bus

event_bus.subscribe("action.after_execute.create_sphere", handler)
event_bus.unsubscribe("action.after_execute.create_sphere", handler)
```
