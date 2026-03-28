# Skills System

The Skills system allows you to register any script (Python, MEL, MaxScript, BAT, Shell, etc.) as an MCP-discoverable tool with **zero Python code**. It directly reuses the [OpenClaw Skills](https://docs.openclaw.ai/tools) ecosystem format.

## Quick Start

### 1. Create a Skill Directory

```
maya-geometry/
├── SKILL.md
└── scripts/
    ├── create_sphere.py
    ├── batch_rename.mel
    └── export_fbx.bat
```

### 2. Write the SKILL.md

```yaml
---
name: maya-geometry
description: "Maya geometry creation and modification tools"
tools: ["Bash", "Read"]
tags: ["maya", "geometry"]
dcc: maya
---
# Maya Geometry Skill

Use these tools to create and modify geometry in Maya.
```

### 3. Set Environment Variable

```bash
# Linux/macOS
export DCC_MCP_SKILL_PATHS="/path/to/my-skills"

# Windows
set DCC_MCP_SKILL_PATHS=C:\path\to\my-skills

# Multiple paths (use platform path separator)
export DCC_MCP_SKILL_PATHS="/path/skills1:/path/skills2"
```

### 4. Use It

Scripts are auto-discovered and registered as MCP tools:

```python
from dcc_mcp_core import create_action_manager

manager = create_action_manager("maya")
# Skills from DCC_MCP_SKILL_PATHS are automatically loaded

# Call a skill action
result = manager.call_action("maya_geometry__create_sphere", radius=2.0)
```

## Supported Script Types

| Extension | Type | Execution |
|-----------|------|-----------|
| `.py` | Python | `subprocess` with system Python |
| `.mel` | MEL (Maya) | Via DCC adapter in context |
| `.ms` | MaxScript | Via DCC adapter in context |
| `.bat`, `.cmd` | Batch | `cmd /c` |
| `.sh`, `.bash` | Shell | `bash` |
| `.ps1` | PowerShell | `powershell -File` |
| `.js`, `.jsx` | JavaScript | `node` |

## How It Works

1. **SkillScanner** scans directories for `SKILL.md` files
2. **SkillLoader** parses the YAML frontmatter and enumerates `scripts/`
3. **ScriptAction factory** generates Action subclasses for each script
4. Actions are registered in the existing **ActionRegistry**
5. MCP Server layer can subscribe to `skill.loaded` events via **EventBus**

## Programmatic Usage

```python
from dcc_mcp_core.skills import SkillScanner, load_skill, scan_skill_paths
from dcc_mcp_core.actions.registry import ActionRegistry

# Scan for skills
scanner = SkillScanner()
skill_dirs = scanner.scan(extra_paths=["/my/skills"], dcc_name="maya")

# Load a specific skill
registry = ActionRegistry()
actions = load_skill("/path/to/maya-geometry", registry=registry, dcc_name="maya")

# Convenience function
dirs = scan_skill_paths(extra_paths=["/my/skills"])
```
