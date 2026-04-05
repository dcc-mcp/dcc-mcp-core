# Skills API

`dcc_mcp_core.SkillScanner`、`dcc_mcp_core.parse_skill_md`、`dcc_mcp_core.scan_skill_paths`

## SkillScanner

扫描目录以发现 Skill 技能包。缓存文件修改时间以支持高效的重复扫描。

```python
from dcc_mcp_core import SkillScanner

scanner = SkillScanner()
```

### 方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `scan(extra_paths=None, dcc_name=None, force_refresh=False)` | `List[str]` | 扫描路径以查找技能包目录 |
| `clear_cache()` | — | 清除修改时间缓存和已发现列表 |

### 属性

| 属性 | 类型 | 说明 |
|------|------|------|
| `discovered_skills` | `List[str]` | 已发现的技能包目录路径 |

### Dunder 方法

| 方法 | 说明 |
|------|------|
| `__repr__` | `SkillScanner(cached=N, discovered=N)` |

## 函数

### parse_skill_md

```python
parse_skill_md(skill_dir: str) -> Optional[SkillMetadata]
```

从技能目录中解析 SKILL.md 文件。如果文件缺失或无效则返回 `None`。

- 提取 `---` 分隔符之间的 YAML 前置元数据
- 枚举 `scripts/` 子目录中的脚本文件
- 发现 `metadata/` 子目录中的 `.md` 文件
- 从 `metadata/depends.md` 合并依赖项

### scan_skill_paths

```python
scan_skill_paths(extra_paths: Optional[List[str]] = None, dcc_name: Optional[str] = None) -> List[str]
```

便捷函数：创建一个新的 `SkillScanner` 并扫描所有路径。

## 搜索路径优先级

1. `extra_paths` 参数（最高优先级）
2. `DCC_MCP_SKILL_PATHS` 环境变量
3. 平台特定的技能目录（DCC 特定）
4. 平台特定的技能目录（全局）

## 环境变量

| 变量 | 说明 |
|------|------|
| `DCC_MCP_SKILL_PATHS` | 技能搜索路径（Windows 使用 `;`，Unix 使用 `:` 分隔） |
