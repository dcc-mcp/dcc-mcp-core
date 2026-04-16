# MCP + Skills 集成指南

> **[English](../../guide/mcp-skills-integration)**

## 为什么要结合 MCP 与 Skills？

### 标准 MCP 的问题

标准 MCP 的 `tools/list` 会一次性返回所有工具——没有任何过滤。

当系统中有 3 个 DCC 实例 × 50 个技能包 × 5 个脚本 = **750 个工具同时在上下文中**。AI 的上下文窗口立即被占满，成本飙升，推理质量下降。

```
标准 MCP tools/list：
┌──────────────────────────────────────────────┐
│  maya_geo__create_sphere                      │
│  maya_geo__bevel                              │
│  maya_anim__set_keyframe  ... (共 250 个)     │
│  blender_sculpt__smooth                       │
│  blender_sculpt__grab     ... (共 250 个)     │
│  houdini_vex__compile     ... (共 250 个)     │
└──────────────────────────────────────────────┘
750 个工具每次都在上下文中 → 昂贵、缓慢
```

### 我们的解决方案：会话范围渐进式发现

每个 AI 会话**绑定到一个 DCC 实例**，`tools/list` 只返回该实例的工具。

```
会话 A（Maya 实例 #1）：
  tools/list → 100 个 Maya 工具 + 50 个共享工具 = 150 个工具

会话 B（Houdini 实例 #1）：
  tools/list → 100 个 Houdini 工具 + 50 个共享工具 = 150 个工具

上下文减少 71%，信息零损失
```

### CLI 工具的问题

CLI 工具对 **DCC 状态一无所知**：
- 无法看到当前场景、选中对象或视口内容
- 需要多次往返才能收集上下文
- 返回需要脆弱解析的原始文本
- 执行过程中没有视觉反馈

dcc-mcp-core **直接运行在 DCC 内部**，可以直接访问其完整状态。

## 架构

```
AI 智能体（Claude、GPT 等）
    │
    │ tools/call {"name": "maya_geo__create_sphere", "radius": 2.0}
    │
    ▼
网关服务器（dcc-mcp-server）          ← 每台机器一个
    │
    │ 会话 A → Maya 实例 #1（IPC）
    │ 会话 B → Houdini 实例 #1（IPC）
    │
    ▼
DCC 桥接插件
    │
    │ 执行脚本，捕获结果
    │
    ▼
{success: true, message: "球体已创建", context: {name: "pSphere1"}}
```

## 核心概念

### ServiceEntry — 每个 DCC 注册的信息

```python
from dcc_mcp_core import TransportManager
import os

mgr = TransportManager(registry_dir="/tmp/dcc-mcp")

# Maya 桥接插件在启动时调用：
iid = mgr.register_service(
    "maya", "127.0.0.1", 18812,
    pid=os.getpid(),
    display_name="Maya-Production",
    scene="character.ma",
    documents=["character.ma", "rig.ma"],
    version="2025",
)
```

网关通过这些信息了解：
- 哪个实例可用
- 哪些文件已打开
- 应将 AI 请求路由到哪个实例

### 会话隔离

```python
# 将会话绑定到特定 Maya 实例
session_id = mgr.get_or_create_session("maya", instance_id)

# tools/list 仅过滤该 Maya 实例的技能
# AI 智能体看到 150 个工具，而非 750 个
```

### Skills 系统

```
SKILL.md（元数据）                     scripts/
─────────────────────────────────────────────────
name: maya-geometry                    create_sphere.py
dcc: maya                              bevel.py
scope: repo                            export_fbx.bat
policy:                                ──────────────
  products: ["maya"]                   ↓ 注册为 MCP 工具
  allow_implicit_invocation: false     maya_geometry__create_sphere
                                       maya_geometry__bevel
                                       maya_geometry__export_fbx
```

零 Python 胶水代码。元数据 + 脚本 = MCP 工具。

### 渐进式发现

工具根据上下文逐步展示：

| 过滤器 | 条件 | 结果 |
|--------|------|------|
| **DCC 类型** | 会话绑定到 Maya | 只显示 Maya 工具 |
| **产品** | `policy.products: ["maya"]` | Houdini 工具隐藏 |
| **作用域** | `scope: system` | 不能被 repo 技能覆盖 |
| **隐式调用** | `allow_implicit_invocation: false` | 需先显式调用 `load_skill` |

## 快速开始

### 1. 安装

```bash
pip install dcc-mcp-core
```

### 2. 创建技能包

```
my-tools/
├── SKILL.md
└── scripts/
    └── create_sphere.py
```

**SKILL.md：**
```yaml
---
name: my-tools
dcc: maya
scope: repo
policy:
  allow_implicit_invocation: true
  products: ["maya"]
---
# 我的 Maya 工具
自定义几何体工具。
```

### 3. 启动服务器

```python
import os
from dcc_mcp_core import create_skill_server, McpHttpConfig

os.environ["DCC_MCP_MAYA_SKILL_PATHS"] = "/path/to/my-tools"

server = create_skill_server("maya", McpHttpConfig(port=8765))
handle = server.start()
print(f"MCP 服务器: {handle.mcp_url()}")
# AI 客户端连接到 http://127.0.0.1:8765/mcp
```

### 4. 连接 Claude Desktop

```json
{
  "mcpServers": {
    "maya": {
      "url": "http://127.0.0.1:8765/mcp"
    }
  }
}
```

## 与其他方案的对比

| 特性 | dcc-mcp-core | 通用 MCP | CLI 工具 |
|------|-------------|---------|---------|
| DCC 状态感知 | ✅ 场景、文档、对象 | ❌ 无 | ❌ 无 |
| 上下文范围控制 | ✅ 会话隔离 | ❌ 全局 | ❌ 不适用 |
| 零代码工具 | ✅ SKILL.md | ❌ 需要完整 Python | ✅ 仅脚本 |
| 多实例支持 | ✅ 网关选举 | ❌ 单一端点 | ❌ 无 |
| 结构化结果 | ✅ 始终 | ⚠️ 手动 | ❌ 文本解析 |
