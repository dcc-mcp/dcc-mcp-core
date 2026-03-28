# 函数适配器

将 Action 类转换为普通可调用函数，适用于 RPyC/API 暴露。

## 单个 Action

```python
from dcc_mcp_core.actions import create_function_adapter

create_sphere = create_function_adapter("create_sphere", dcc_name="maya")
result = create_sphere(radius=2.0)
```

## 所有 Action

```python
from dcc_mcp_core.actions import create_function_adapters

funcs = create_function_adapters(dcc_name="maya", manager=manager)
result = funcs["create_sphere"](radius=2.0)
```
