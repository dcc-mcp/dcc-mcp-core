# Action Manager

`ActionManager` 是动作生命周期的中央协调器 — 负责发现、加载和执行动作。

## 创建 ActionManager

```python
from dcc_mcp_core import create_action_manager, get_action_manager

# 创建新实例
manager = create_action_manager(
    dcc_name="maya",
    context={"cmds": maya.cmds},
    load_env_paths=True,
    load_skill_paths=True,
)

# 获取缓存的单例
manager = get_action_manager("maya")
```

## 发现动作

```python
manager.discover_actions_from_path("/path/to/actions.py")
manager.discover_actions_from_package("my_actions_package")
manager.refresh_actions(force=True)
```

## 执行动作

```python
# 同步执行
result = manager.call_action("create_sphere", radius=2.0)

# 异步执行
result = await manager.call_action_async("create_sphere", radius=2.0)

if result.success:
    print(f"成功: {result.message}")
else:
    print(f"错误: {result.error}")
```

## 查询信息

```python
info = manager.get_actions_info()
names = manager.list_available_actions()
```

## ActionRegistry

```python
from dcc_mcp_core.actions.registry import ActionRegistry

registry = ActionRegistry()  # 单例
registry.register(MyAction)
action_cls = registry.get_action("my_action", dcc_name="maya")
```
