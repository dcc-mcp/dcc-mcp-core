# 网关选举与多实例支持

> **[English](../../guide/gateway-election)**

## 什么是网关？

**网关**是一个单一的 Rust HTTP 服务器（默认运行在 `localhost:9999`），负责：

- 发现所有运行中的 DCC 实例（Maya、Blender、Houdini、Photoshop 等）
- 根据会话将 AI 请求路由到正确的实例
- 为每个会话提供有范围的 `tools/list`（防止上下文爆炸）
- 处理请求取消和 SSE 通知

**每台机器只有一个网关**。当第一个 DCC 实例注册时自动启动。

## 问题：先到先得

没有版本感知时，最旧的 DCC 赢得网关角色：

```
Maya v0.12.6 启动 → 绑定端口 9999 → 成为网关
Maya v0.12.29 启动 → 端口 9999 已占用 → 成为从属
❌ 旧版本控制路由；新功能被忽略
```

## 我们的解决方案：版本感知选举

```
Maya v0.12.6（网关）              Maya v0.12.29（新实例）
         │                                │
         │                   端口 9999 已占用
         │                                │
         │         读取 __gateway__ 哨兵
         │         自己版本 0.12.29 > 网关 0.12.6
         │                                │
         │ ←── POST /gateway/yield {"challenger_version": "0.12.29"}
         │
         │（支持 yield → 优雅关闭）
         │ yield_tx.send(true)
         │ 释放端口 9999
         │
                          每 10 秒重试
                          端口空闲 → 绑定
                          注册新哨兵
                          ✅ v0.12.29 现在是网关
```

### 工作原理

**1. `__gateway__` 哨兵**

网关启动时，在 FileRegistry 中写入一个特殊条目：
```json
{"dcc_type": "__gateway__", "version": "0.12.29"}
```

新实例读取此条目以了解当前网关及其版本。

**2. 语义版本比较**

版本按数值比较（非字母序）：
```
0.12.6  对比  0.12.29
↓              ↓
[0, 12, 6]  [0, 12, 29]
                 29 > 6 → v0.12.29 更新 ✓
```

**3. 主动让位**

清理任务（每 15 秒）检查是否有更新的挑战者。如果发现，立即优雅关闭。

**4. 挑战者重试循环**

新实例每 10 秒轮询端口，最多 120 秒。一旦端口空闲，立即接管。

## 多实例注册

同一类型的多个 DCC 实例可以同时存在：

```python
from dcc_mcp_core import TransportManager
import os

mgr = TransportManager("/tmp/dcc-mcp")

# Maya #1：动画工作
iid_anim = mgr.register_service(
    "maya", "127.0.0.1", 18812,
    pid=os.getpid(),
    display_name="Maya-Animation",
    scene="shot_001.ma",
    documents=["shot_001.ma", "shot_002.ma"],
    version="2025",
)

# Maya #2：绑定工作
iid_rig = mgr.register_service(
    "maya", "127.0.0.1", 18813,
    pid=12345,
    display_name="Maya-Rigging",
    scene="character_rig.ma",
    documents=["character_rig.ma"],
    version="2025",
)

# 列出所有 Maya 实例
instances = mgr.list_instances("maya")
# → [Maya-Animation, Maya-Rigging]

# 查找最佳实例（AVAILABLE > BUSY；IPC > TCP）
best = mgr.find_best_service("maya")

# 按优先级排列所有实例
ranked = mgr.rank_services("maya")
```

## 文档追踪

对于多文档 DCC（Photoshop、After Effects），追踪所有打开的文件：

```python
# Photoshop 以初始文档注册
iid = mgr.register_service(
    "photoshop", "127.0.0.1", 18820,
    pid=55001,
    display_name="PS-Marketing",
    scene="logo.psd",
    documents=["logo.psd", "banner.psd"],
)

# 用户打开新文档
mgr.update_documents(
    "photoshop", iid,
    active_document="icon.psd",
    documents=["logo.psd", "banner.psd", "icon.psd"],
)

# 用户切换活跃文档
mgr.update_documents(
    "photoshop", iid,
    active_document="banner.psd",
    documents=["logo.psd", "banner.psd", "icon.psd"],
)

entry = mgr.get_service("photoshop", iid)
print(entry.scene)      # "banner.psd"（活跃文档）
print(entry.documents)  # ["logo.psd", "banner.psd", "icon.psd"]
```

## 会话隔离

每个 AI 会话**绑定到一个实例**：

```python
# AI 智能体 A 只与 Maya-Animation 通信
session_a = mgr.get_or_create_session("maya", iid_anim)

# AI 智能体 B 只与 Maya-Rigging 通信
session_b = mgr.get_or_create_session("maya", iid_rig)

# 会话不同——无上下文混淆
assert session_a != session_b

# tools/list 按每个会话的实例过滤
# 智能体 A 看到：maya_anim__set_keyframe, ...
# 智能体 B 看到：maya_rig__mirror_joints, ...
```

## 实例健康检查

```python
# 通过心跳保持实例存活
mgr.heartbeat("maya", iid)  # → True 表示存活，False 表示未找到

# 更新实例状态
from dcc_mcp_core import ServiceStatus
mgr.update_service_status("maya", iid, ServiceStatus.BUSY)

# DCC 退出时清理
mgr.deregister_service("maya", iid)
```

## 向后兼容性

不支持 `/gateway/yield` 的旧版 DCC 会返回 404——这没问题。挑战者进入轮询重试循环，等待端口自然释放（当旧 DCC 退出或崩溃时）。无硬性失败，优雅降级。
