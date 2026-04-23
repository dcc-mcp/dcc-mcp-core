# Resources API

`dcc_mcp_core` — 用于实时 DCC 状态的 MCP Resources 原语。

## 概述

Resources 原语使 MCP 客户端（LLM 和主机）能够通过用于工具的
相同 HTTP 端点读取**实时 DCC 状态**。它实现了
[MCP 2025-03-26](https://modelcontextprotocol.io/specification/2025-03-26)
`resources` 能力：

- `resources/list` — 枚举可用 URI
- `resources/read` — 通过 URI 获取内容（文本或 base64 blob）
- `resources/subscribe` / `resources/unsubscribe` — 选择加入推送通知
- `notifications/resources/updated` — 当订阅的 URI 更改时通过 SSE 服务器推送

Resources 在 `initialize` 中通告为：

```json
{
  "capabilities": {
    "resources": { "subscribe": true, "listChanged": true }
  }
}
```

## 内置 Resources

`dcc-mcp-core` 附带四个内置 producer。每个都以 URI scheme 为键。

| URI | MIME | 说明 | 通知 |
|-----|------|------|------|
| `scene://current` | `application/json` | 当前 `SceneInfo` 快照（或占位符） | 调用 `set_scene()` 时触发 |
| `capture://current_window` | `image/png` | 活动 DCC 窗口的 PNG（base64 blob） | 无（按需读取） |
| `audit://recent?limit=N` | `application/json` | `AuditLog` 的尾部（默认 50，最大 500） | 每次 `AuditLog.record()` 时触发 |
| `artefact://sha256/<hex>` | 可变 | 内容寻址的制品存储 (#349)；body 以其声明的 MIME 作为 base64 blob 返回。由 `enable_artefact_resources` 门控。 | 无（轮询模型） |

`capture://current_window` 仅在真实窗口捕获后端可用时列出
（目前为 Windows `HWND PrintWindow`）。在其他平台上它会从
`resources/list` 中隐藏。

## 启用 / 禁用

```python
from dcc_mcp_core import McpHttpConfig

cfg = McpHttpConfig(port=8765)
cfg.enable_resources = True              # 默认: True — 通告能力 + 内置项
cfg.enable_artefact_resources = False    # 默认: False — 启用前 artefact:// 返回 -32002
```

当 `enable_resources = False` 时，服务器不通告该能力，所有四个
`resources/*` 方法返回 `-32601 method not found`。

## 连接外部状态

DCC 适配器通常在调用 `server.start()` **之前**设置场景快照并转发
审计事件到注册表：

```python
from dcc_mcp_core import (
    AuditLog,
    McpHttpServer,
    McpHttpConfig,
    ToolRegistry,
)

registry = ToolRegistry()
# ... registry.register(...) ...

server = McpHttpServer(registry, McpHttpConfig(port=8765))

# 1. 每当 DCC 场景变化时推送场景快照:
server.resources().set_scene({
    "scene_path": "/projects/shot_010/main.ma",
    "fps": 24,
    "frame_range": [1001, 1240],
    "active_camera": "persp",
})

# 2. 挂钩 sandbox AuditLog，使 audit://recent 在每次记录时触发通知:
audit = AuditLog()
server.resources().wire_audit_log(audit)

handle = server.start()
```

### 更新场景快照

`set_scene()` 原子性地替换快照并发出一个
`notifications/resources/updated`，其 `uri = "scene://current"`
到每个已订阅的会话。

```python
server.resources().set_scene(new_snapshot_dict)
```

传递 `None` 以清除快照（读者随后收到一个带有 `status: "no_snapshot"` 的占位符）。

## 示例: 客户端侧

```python
import json, urllib.request

# Initialize + 获取 session id（此处省略）
# 列出 resources
body = {"jsonrpc": "2.0", "id": 1, "method": "resources/list"}
req = urllib.request.Request(
    "http://127.0.0.1:8765/mcp",
    data=json.dumps(body).encode(),
    headers={"Content-Type": "application/json", "Mcp-Session-Id": session_id},
    method="POST",
)
with urllib.request.urlopen(req) as r:
    print(json.loads(r.read())["result"]["resources"])

# 读取审计日志
body = {
    "jsonrpc": "2.0", "id": 2,
    "method": "resources/read",
    "params": {"uri": "audit://recent?limit=10"},
}
# ... POST, 将 result.contents[0].text 作为 JSON 解析 ...

# 订阅场景更新
body = {
    "jsonrpc": "2.0", "id": 3,
    "method": "resources/subscribe",
    "params": {"uri": "scene://current"},
}
# ... POST, 然后打开 GET /mcp SSE 流以接收 notifications/resources/updated ...
```

## 错误码

| 码 | 含义 |
|----|------|
| `-32601` | 方法未找到 — resources 已禁用 (`enable_resources = False`) |
| `-32602` | 无效参数 — 缺失或格式错误的 `uri` |
| `-32002` | Resource 未启用（scheme 已识别，后端已禁用）— 当 `artefact://` URI 语法有效但未存储时也会复用 |
| `-32603` | 内部错误 — producer 失败（捕获后端错误等） |

## `artefact://` Scheme (issue #349)

工具和工作流步骤之间的内容寻址制品传递。
完整指南请参见 [`docs/guide/artefacts.md`](../guide/artefacts.md)。
快速参考：

- URI 形状: `artefact://sha256/<hex>`。
- 默认后端: `FilesystemArtefactStore`，锚定在
  `<registry_dir>/dcc-mcp-artefacts`（或临时目录）。
- `resources/list` 枚举每个已存储的制品；条目携带声明的 MIME 和
  sidecar 元数据。
- `resources/read` 以 base64 blob 返回原始字节。
- Python 辅助函数: `artefact_put_file`, `artefact_put_bytes`,
  `artefact_get_bytes`, `artefact_list`。
- 通过 `McpHttpConfig.enable_artefact_resources = True` 启用。

## 编写自定义 Producer (Rust)

```rust
use dcc_mcp_http::{ProducerContent, ResourceError, ResourceProducer, ResourceResult};
use async_trait::async_trait;

struct PlaybackProducer;

#[async_trait]
impl ResourceProducer for PlaybackProducer {
    fn scheme(&self) -> &str { "playback" }

    async fn list(&self) -> ResourceResult<Vec<McpResource>> {
        Ok(vec![McpResource {
            uri: "playback://current".into(),
            name: "Playback state".into(),
            description: Some("Current playback frame and range".into()),
            mime_type: Some("application/json".into()),
        }])
    }

    async fn read(&self, uri: &str) -> ResourceResult<Vec<ProducerContent>> {
        if uri != "playback://current" {
            return Err(ResourceError::NotFound(uri.into()));
        }
        Ok(vec![ProducerContent::Text {
            uri: uri.into(),
            mime_type: Some("application/json".into()),
            text: r#"{"frame": 42, "range": [1, 100]}"#.into(),
        }])
    }
}
```

然后在 `start()` 之前将其注册到服务器：

```rust
server.resources().add_producer(Arc::new(PlaybackProducer));
```

## 生命周期保证

- 订阅是**每会话**的。当客户端终止会话 (`DELETE /mcp`) 时，
  其所有订阅自动丢弃。
- `notifications/resources/updated` 是尽力而为 — 如果 SSE 通道已满
  或客户端已断开连接，通知会被丢弃（无队列，producer 上无背压）。
- Producer 必须廉价可调用 — 每次 MCP 客户端重连都会调用 `resources/list`。
- Blob 内容 (`capture://current_window`) 在 JSON-RPC 响应中 base64 编码；
  保持 payload 低于约 5 MB 以避免客户端解码问题。

## 参见

- [HTTP API](./http.md) — `McpHttpServer`, `McpHttpConfig`
- [Sandbox API](./sandbox.md) — `AuditLog` (`audit://recent` 的来源)
- [Capture API](./capture.md) — `Capturer` (`capture://current_window` 的来源)
