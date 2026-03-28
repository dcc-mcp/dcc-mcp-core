# 数据模型

## ActionResultModel

所有动作执行的标准化结果。

### 字段

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `success` | `bool` | `True` | 执行是否成功 |
| `message` | `str` | — | 可读的结果描述 |
| `prompt` | `Optional[str]` | `None` | 给 AI 的下一步建议 |
| `error` | `Optional[str]` | `None` | 错误消息 |
| `context` | `Dict[str, Any]` | `{}` | 附加上下文数据 |

### 工厂函数

```python
from dcc_mcp_core import success_result, error_result, from_exception

result = success_result("创建了 5 个球体", prompt="使用 modify_spheres", count=5)
error = error_result("失败", "文件未找到", prompt="检查路径")
exc_result = from_exception(e, message="导入失败", include_traceback=True)
```

## SkillMetadata

从 SKILL.md frontmatter 解析的元数据。

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `name` | `str` | — | 唯一标识符 |
| `description` | `str` | `""` | 描述 |
| `tools` | `List[str]` | `[]` | 所需工具权限 |
| `dcc` | `str` | `"python"` | 目标 DCC |
| `tags` | `List[str]` | `[]` | 分类标签 |
| `scripts` | `List[str]` | `[]` | 脚本文件路径 |
| `version` | `str` | `"1.0.0"` | 版本 |
