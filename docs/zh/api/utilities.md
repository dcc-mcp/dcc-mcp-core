# 工具函数 API

详见 [英文 API 文档](/api/utilities) 获取完整参考。

## 装饰器

```python
from dcc_mcp_core.utils.decorators import error_handler, with_context

@error_handler
def risky_operation(data):
    return {"processed": True}
```

## 类型包装器

```python
from dcc_mcp_core.utils.type_wrappers import wrap_value, unwrap_value

wrapped = wrap_value(True)
original = unwrap_value(wrapped)
```

## 文件系统

```python
from dcc_mcp_core.utils.filesystem import get_config_dir, get_actions_dir

config = get_config_dir()
actions = get_actions_dir("maya")
```

## 环境变量

| 变量 | 说明 |
|------|------|
| `DCC_MCP_ACTION_PATHS` | 动作搜索路径 |
| `DCC_MCP_ACTION_PATH_{DCC}` | DCC 特定动作路径 |
| `DCC_MCP_SKILL_PATHS` | 技能包搜索路径 |
