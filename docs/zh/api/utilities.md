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
    get_app_skill_paths_from_env,
)

config = get_config_dir()           # ~/.config/dcc-mcp (Linux)
data = get_data_dir()               # ~/.local/share/dcc-mcp (Linux)
log = get_log_dir()                 # ~/.local/share/dcc-mcp/log (Linux)
actions = get_actions_dir("maya")   # {data}/actions/maya/
skills = get_skills_dir("maya")     # {data}/skills/maya/
skills = get_skills_dir()           # {data}/skills/ (全局)

# 通用平台目录（config、data、cache、log、documents）
dir = get_platform_dir("cache")

# 从全局环境变量获取
paths = get_skill_paths_from_env()  # 分割 DCC_MCP_SKILL_PATHS

# 从应用专属环境变量获取
paths = get_app_skill_paths_from_env("maya")     # 读取 DCC_MCP_MAYA_SKILL_PATHS
paths = get_app_skill_paths_from_env("blender")  # 读取 DCC_MCP_BLENDER_SKILL_PATHS
```

::: tip
所有 `get_*_dir()` 函数在目录不存在时会自动创建（`get_skills_dir` 除外，仅返回路径）。
:::

### `get_app_skill_paths_from_env`

```python
def get_app_skill_paths_from_env(app_name: str) -> list[str]: ...
```

从 `DCC_MCP_{APP_NAME}_SKILL_PATHS` 环境变量中返回技能路径列表。

| 参数 | 类型 | 说明 |
|------|------|------|
| `app_name` | `str` | DCC 应用名称，如 `"maya"`、`"blender"`。大小写不敏感——自动转为大写形成环境变量名。 |

**返回值**：`list[str]` — 从环境变量中解析的目录路径，未设置时返回 `[]`。

```python
import os
os.environ["DCC_MCP_MAYA_SKILL_PATHS"] = "/studio/maya-skills;/home/user/skills"

from dcc_mcp_core import get_app_skill_paths_from_env

paths = get_app_skill_paths_from_env("maya")
# ["/studio/maya-skills", "/home/user/skills"]

# 大小写不敏感："Maya"、"MAYA"、"maya" 均读取 DCC_MCP_MAYA_SKILL_PATHS
paths = get_app_skill_paths_from_env("Maya")  # 同上
```

## 常量

模块级别属性：

| 常量 | 值 | 说明 |
|------|------|------|
| `APP_NAME` | `"dcc-mcp"` | 平台目录使用的应用名 |
| `APP_AUTHOR` | `"dcc-mcp"` | 应用作者 |
| `DEFAULT_DCC` | `"python"` | 默认 DCC 名称 |
| `DEFAULT_LOG_LEVEL` | `"DEBUG"` | 默认日志级别 |
| `ENV_LOG_LEVEL` | `"MCP_LOG_LEVEL"` | 日志级别环境变量 |
| `ENV_SKILL_PATHS` | `"DCC_MCP_SKILL_PATHS"` | 全局技能路径环境变量 |
| `ENV_APP_SKILL_PATHS` | `"DCC_MCP_{APP}_SKILL_PATHS"` | 应用专属技能路径环境变量模板 |
| `SKILL_METADATA_FILE` | `"SKILL.md"` | 技能元数据文件名 |
| `SKILL_SCRIPTS_DIR` | `"scripts"` | 脚本子目录名 |

## 环境变量

| 变量 | 说明 |
|------|------|
| `DCC_MCP_SKILL_PATHS` | 全局技能搜索路径（Windows 使用 `;`，Unix 使用 `:` 分隔） |
| `DCC_MCP_{APP}_SKILL_PATHS` | 应用专属技能路径，如 Maya 使用 `DCC_MCP_MAYA_SKILL_PATHS` |
| `MCP_LOG_LEVEL` | 日志级别覆盖（DEBUG、INFO、WARN、ERROR） |

::: tip 搜索路径优先级
调用 `create_skill_manager("maya")` 时，技能目录按以下顺序解析：
1. `extra_paths` 参数传入的额外路径（最高优先级）
2. 应用专属环境变量：`DCC_MCP_MAYA_SKILL_PATHS`
3. 全局环境变量：`DCC_MCP_SKILL_PATHS`
4. 平台数据目录：`~/.local/share/dcc-mcp/skills/maya/`
:::
