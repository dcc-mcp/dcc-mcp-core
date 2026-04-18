# 数据模型

## ToolResult

所有 Action 执行的标准化结果，底层为通过 PyO3 暴露的 Rust 结构体。

### 字段

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `success` | `bool` | `True` | 执行是否成功 |
| `message` | `str` | `""` | 人类可读的结果描述 |
| `prompt` | `Optional[str]` | `None` | 给 AI 的下一步建议 |
| `error` | `Optional[str]` | `None` | `success` 为 `False` 时的错误消息 |
| `context` | `Dict[str, Any]` | `{}` | 附加上下文数据 |

### 方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `with_error(error)` | `ToolResult` | 创建带错误信息的副本（设置 `success=False`） |
| `with_context(**kwargs)` | `ToolResult` | 创建带更新上下文的副本 |
| `to_dict()` | `Dict[str, Any]` | 转换为字典 |
| `to_json()` | `str` | 序列化为 JSON 字符串 |
| `__eq__(other)` | `bool` | 相等比较 |
| `__str__()` | `str` | 人类可读字符串 |
| `__repr__()` | `str` | 无歧义表示 |

::: warning 不支持直接使用 `json.dumps()`
`ToolResult` 是 Rust 后端对象，**不能直接传给 `json.dumps()`**。
请使用 `to_json()` 或先转换为字典：

```python
import json
result = success_result("完成")

# 方式一 — 内置 JSON 序列化（推荐，使用 Rust serde）
json_str = result.to_json()

# 方式二 — 转换为字典后序列化
json_str = json.dumps(result.to_dict())

# 方式三 — serialize_result（支持 JSON 和 MsgPack）
from dcc_mcp_core import serialize_result
json_str = serialize_result(result)
```
:::

### 工厂函数

```python
from dcc_mcp_core import success_result, error_result, from_exception, validate_action_result

# 带上下文的成功结果
result = success_result("创建了 5 个球体", prompt="使用 modify_spheres", count=5)

# 带可能解决方案的错误结果
error = error_result(
    "失败", "文件未找到",
    prompt="检查路径",
    possible_solutions=["确认文件存在", "检查权限"],
    path="/bad/path",
)

# 从异常字符串创建
exc_result = from_exception(
    "ValueError: bad input",
    message="导入失败",
    include_traceback=True,
)

# 验证/规范化任意值为 ToolResult
validate_action_result(result)                          # 直接通过
validate_action_result({"success": True, "message": "OK"})  # dict → ARM
validate_action_result("hello")                         # 包装为成功结果
```

### 工厂函数签名

| 函数 | 签名 | 说明 |
|------|------|------|
| `success_result` | `(message, prompt=None, **context) -> ToolResult` | 创建成功结果 |
| `error_result` | `(message, error, prompt=None, possible_solutions=None, **context) -> ToolResult` | 创建失败结果 |
| `from_exception` | `(error_message, message=None, prompt=None, include_traceback=True, possible_solutions=None, **context) -> ToolResult` | 将异常包装为结果 |
| `validate_action_result` | `(result: Any) -> ToolResult` | 规范化 dict/str/None/ARM → ToolResult |

## SkillMetadata

从 SKILL.md 前置元数据解析的元数据。所有字段可读可写。

### 字段

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `name` | `str` | — | 唯一标识符 |
| `description` | `str` | `""` | 人类可读描述 |
| `tools` | `list[ToolDeclaration]` | `[]` | 从 SKILL.md 解析的声明工具 |
| `dcc` | `str` | `"python"` | 目标 DCC 应用 |
| `tags` | `list[str]` | `[]` | 分类标签 |
| `scripts` | `list[str]` | `[]` | 发现的脚本文件路径 |
| `skill_path` | `str` | `""` | 技能包目录的绝对路径 |
| `version` | `str` | `"1.0.0"` | 技能版本 |
| `depends` | `list[str]` | `[]` | 依赖的技能名称 |
| `metadata_files` | `list[str]` | `[]` | metadata/ 目录中的文件 |
| `license` | `str` | `""` | 许可证标识 |
| `compatibility` | `str` | `""` | 兼容性描述 |
| `allowed_tools` | `list[str]` | `[]` | 允许的工具名称（限制可注册的工具）|
| `groups` | `list[SkillGroup]` | `[]` | 工具分组（渐进式暴露）|
