# Skills 技能包系统

Skills 系统允许你将任何脚本零代码注册为 MCP 可发现的工具，直接复用 [OpenClaw Skills](https://docs.openclaw.ai/tools) 生态格式。

## 快速上手

### 1. 创建 Skill 目录

```
maya-geometry/
├── SKILL.md
└── scripts/
    ├── create_sphere.py
    ├── batch_rename.mel
    └── export_fbx.bat
```

### 2. 编写 SKILL.md

```yaml
---
name: maya-geometry
description: "Maya 几何体创建和修改工具"
tools: ["Bash", "Read"]
tags: ["maya", "geometry"]
dcc: maya
---
# Maya Geometry Skill

使用这些工具在 Maya 中创建和修改几何体。
```

### 3. 设置环境变量

```bash
# Linux/macOS
export DCC_MCP_SKILL_PATHS="/path/to/my-skills"

# Windows
set DCC_MCP_SKILL_PATHS=C:\path\to\my-skills

# 多路径（使用平台路径分隔符）
export DCC_MCP_SKILL_PATHS="/path/skills1:/path/skills2"
```

### 4. 使用

脚本会被自动发现并注册为 MCP 工具：

```python
from dcc_mcp_core import create_action_manager

manager = create_action_manager("maya")
# DCC_MCP_SKILL_PATHS 中的 Skills 会被自动加载

# 调用 Skill Action
result = manager.call_action("maya_geometry__create_sphere", radius=2.0)
```

## 支持的脚本类型

| 扩展名 | 类型 | 执行方式 |
|--------|------|---------|
| `.py` | Python | `subprocess` 使用系统 Python |
| `.mel` | MEL (Maya) | 通过上下文中的 DCC 适配器 |
| `.ms` | MaxScript | 通过上下文中的 DCC 适配器 |
| `.bat`, `.cmd` | Batch | `cmd /c` |
| `.sh`, `.bash` | Shell | `bash` |
| `.ps1` | PowerShell | `powershell -File` |
| `.js`, `.jsx` | JavaScript | `node` |

## 工作原理

1. **SkillScanner** 扫描目录查找 `SKILL.md` 文件
2. **SkillLoader** 解析 YAML 前置元数据并枚举 `scripts/` 目录
3. **ScriptAction 工厂** 为每个脚本生成 Action 子类
4. Action 注册到现有的 **ActionRegistry** 中
5. MCP Server 层可通过 **EventBus** 订阅 `skill.loaded` 事件

## 编程式用法

```python
from dcc_mcp_core.skills import SkillScanner, load_skill, scan_skill_paths
from dcc_mcp_core.actions.registry import ActionRegistry

# 扫描技能包
scanner = SkillScanner()
skill_dirs = scanner.scan(extra_paths=["/my/skills"], dcc_name="maya")

# 加载特定技能包
registry = ActionRegistry()
actions = load_skill("/path/to/maya-geometry", registry=registry, dcc_name="maya")

# 便捷函数
dirs = scan_skill_paths(extra_paths=["/my/skills"])
```
