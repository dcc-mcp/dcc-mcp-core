# 工具函数 API

`dcc_mcp_core.utils` — 文件系统、常量、类型包装器、日志。

## 类型包装器（RPyC）

用于在 RPyC 连接上安全传输 Python 值的类型包装器。

```python
from dcc_mcp_core import wrap_value, unwrap_value, unwrap_parameters
from dcc_mcp_core import BooleanWrapper, IntWrapper, FloatWrapper, StringWrapper

# 包装原生值
wrapped = wrap_value(True)         # BooleanWrapper(True)
wrapped = wrap_value(42)           # IntWrapper(42)
wrapped = wrap_value(3.14)         # FloatWrapper(3.14)
wrapped = wrap_value("hello")     # StringWrapper("hello")

# 还原为原生值
original = unwrap_value(wrapped)   # True, 42, 3.14, 或 "hello"

# 批量还原字典中的所有值
params = {"visible": BooleanWrapper(True), "count": IntWrapper(5)}
native = unwrap_parameters(params)  # {"visible": True, "count": 5}
```

### 包装器类

| 类 | Python 类型 | 特殊方法 |
|----|-------------|----------|
| `BooleanWrapper(value)` | `bool` | `__bool__`、`__eq__`、`__hash__` |
| `IntWrapper(value)` | `int` (`i64`) | `__int__`、`__index__`、`__eq__`、`__hash__` |
| `FloatWrapper(value)` | `float` (`f64`) | `__float__`、`__eq__`（相对容差），**不可哈希** |
| `StringWrapper(value)` | `str` | `__str__`、`__eq__`、`__hash__` |

::: warning
`FloatWrapper` 故意不可哈希（`__hash__` 会抛出 `TypeError`），因为 `f64` 不实现 `Eq`/`Hash`（NaN ≠ NaN）。不要将其用作字典键或放入集合中。
:::

## 文件系统

使用 `dirs` crate 进行平台特定目录解析。

```python
from dcc_mcp_core import (
    get_platform_dir, get_config_dir, get_data_dir, get_log_dir,
    get_actions_dir, get_skills_dir, get_skill_paths_from_env,
)

config = get_config_dir()           # ~/.config/dcc-mcp (Linux)
data = get_data_dir()               # ~/.local/share/dcc-mcp (Linux)
log = get_log_dir()                 # ~/.local/share/dcc-mcp/log (Linux)
actions = get_actions_dir("maya")   # {data}/actions/maya/
skills = get_skills_dir("maya")     # {data}/skills/maya/
skills = get_skills_dir()           # {data}/skills/ (全局)

# 通用平台目录（config、data、cache、log、documents）
dir = get_platform_dir("cache")

# 从环境变量获取
paths = get_skill_paths_from_env()  # 分割 DCC_MCP_SKILL_PATHS
```

::: tip
所有 `get_*_dir()` 函数在目录不存在时会自动创建（`get_skills_dir` 除外，仅返回路径）。
:::

## 常量

模块级别属性：

| 常量 | 值 | 说明 |
|------|------|------|
| `APP_NAME` | `"dcc-mcp"` | 平台目录使用的应用名 |
| `APP_AUTHOR` | `"dcc-mcp"` | 应用作者 |
| `DEFAULT_DCC` | `"python"` | 默认 DCC 名称 |
| `DEFAULT_LOG_LEVEL` | `"DEBUG"` | 默认日志级别 |
| `ENV_LOG_LEVEL` | `"MCP_LOG_LEVEL"` | 日志级别环境变量 |
| `ENV_SKILL_PATHS` | `"DCC_MCP_SKILL_PATHS"` | 技能路径环境变量 |
| `SKILL_METADATA_FILE` | `"SKILL.md"` | 技能元数据文件名 |
| `SKILL_SCRIPTS_DIR` | `"scripts"` | 脚本子目录名 |

## 环境变量

| 变量 | 说明 |
|------|------|
| `DCC_MCP_SKILL_PATHS` | 技能搜索路径（Windows 使用 `;`，Unix 使用 `:` 分隔） |
| `MCP_LOG_LEVEL` | 日志级别覆盖（DEBUG、INFO、WARN、ERROR） |
