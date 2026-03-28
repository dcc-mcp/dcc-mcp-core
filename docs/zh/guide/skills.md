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
```

### 4. 使用

```python
from dcc_mcp_core import create_action_manager

manager = create_action_manager("maya")
result = manager.call_action("maya_geometry__create_sphere", radius=2.0)
```

## 支持的脚本类型

| 扩展名 | 类型 | 执行方式 |
|--------|------|---------|
| `.py` | Python | `subprocess` |
| `.mel` | MEL (Maya) | DCC 适配器 |
| `.ms` | MaxScript | DCC 适配器 |
| `.bat`, `.cmd` | Batch | `cmd /c` |
| `.sh`, `.bash` | Shell | `bash` |
| `.ps1` | PowerShell | `powershell -File` |
| `.js`, `.jsx` | JavaScript | `node` |
