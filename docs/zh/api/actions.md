# Actions API

详见 [英文 API 文档](/api/actions) 获取完整参考。

## Action 基类

- `setup(**kwargs)` — 验证输入
- `process()` — 同步执行
- `process_async()` — 异步执行
- `_execute()` — **必须实现**

## ActionManager

```python
from dcc_mcp_core import create_action_manager, get_action_manager

manager = create_action_manager("maya")
result = manager.call_action("create_sphere", radius=2.0)
info = manager.get_actions_info()
names = manager.list_available_actions()
```

## ActionRegistry

```python
from dcc_mcp_core.actions.registry import ActionRegistry

registry = ActionRegistry()
registry.register(MyAction)
action_cls = registry.get_action("my_action", dcc_name="maya")
```
