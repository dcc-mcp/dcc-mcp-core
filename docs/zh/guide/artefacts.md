# 制品传递 (Artefact Hand-Off)

> **Issue**: [#349](https://github.com/loonghao/dcc-mcp-core/issues/349)
> **Crate**: `dcc-mcp-artefact`
> **Scheme**: `artefact://sha256/<hex>`

流水线在工具之间以及工作流步骤之间传递*文件*：导入的场景、
质检报告、暂存的 `.uasset`、烘焙的模拟。内联传递原始字节会
膨胀 MCP 传输；传递绝对路径会跨机器中断并丢失
MIME / 大小 / 校验和元数据。

`dcc-mcp-artefact` 提供一个小型值类型（`FileRef`）加上一个
内容寻址存储后端，并将一个 `artefact://` URI scheme 接入
MCP Resources 原语，使 MCP 客户端可以通过 URI `resources/read`
文件传递。

## 概念

- **`FileRef`** — 对已存储文件的可序列化引用：
  - `uri` — 规范形式，例如 `artefact://sha256/<hex>`
  - `mime`, `size_bytes`, `digest` (`sha256:<hex>`)
  - `producer_job_id`（可选，用于工作流步骤输出）
  - `created_at` (RFC-3339)
  - `metadata`（工具定义的 JSON — 宽/高、帧号等）

- **`ArtefactStore`** — trait，带有 `put` / `get` / `head` / `delete` /
  `list`。存储是内容寻址的：提交相同的字节两次会返回相同的 URI。

- **`FilesystemArtefactStore`** — 默认持久化后端。每个制品存放在
  `<root>/<sha256>.bin` 下，并带有一个携带 `FileRef` 的
  `<sha256>.json` sidecar。`McpHttpServer` 将其存储锚定在
  `<registry_dir>/dcc-mcp-artefacts`（或当未配置注册表时的 OS 临时目录）。

- **`InMemoryArtefactStore`** — 非持久化后端，用于测试。

## 启用 Artefact Resources

```python
from dcc_mcp_core import McpHttpConfig, McpHttpServer, ToolRegistry

cfg = McpHttpConfig(port=8765)
cfg.enable_artefact_resources = True    # 默认关闭
server = McpHttpServer(ToolRegistry(), cfg)
handle = server.start()
```

一旦启用：

- `resources/list` 将每个已存储的制品作为 `artefact://` URI 包含在内。
- `resources/read` 以正确的 MIME 类型返回 body，作为 base64 blob。
- 未知 URI 的 `resources/read` 返回 MCP 错误 `-32002`。

禁用（默认）路径仍然识别该 scheme 并以 `-32002` "not enabled"
错误响应，因此客户端可以区分 "scheme unknown" 和
"scheme recognized but backing store off"。

## Python 辅助函数

```python
from dcc_mcp_core import (
    FileRef,
    artefact_put_bytes,
    artefact_put_file,
    artefact_get_bytes,
    artefact_list,
)

# 将磁盘上的文件放入并获取 FileRef。
ref = artefact_put_file("/tmp/render.png", mime="image/png")
print(ref.uri)           # artefact://sha256/<hex>
print(ref.digest)        # sha256:<hex>
print(ref.size_bytes)    # 1024

# 往返一个字节缓冲区。
bref = artefact_put_bytes(b"hello", mime="text/plain")
assert artefact_get_bytes(bref.uri) == b"hello"

# 清单。
for entry in artefact_list():
    print(entry.uri, entry.mime, entry.size_bytes)
```

辅助函数目标是一个进程级默认存储
（`<temp_dir>/dcc-mcp-artefacts`）。在服务器进程内部，
`McpHttpServer` 会自动将辅助函数指向它自己的存储。

## Rust API

```rust
use dcc_mcp_artefact::{
    ArtefactBody, ArtefactFilter, ArtefactStore,
    FilesystemArtefactStore, InMemoryArtefactStore,
    put_bytes, put_file,
};

// 持久化存储 — 真实服务器的默认选项。
let store = FilesystemArtefactStore::new_in("/var/cache/dcc/artefacts")?;
let fr = put_bytes(&store, b"payload".to_vec(), Some("text/plain".into()))?;
assert!(fr.uri.starts_with("artefact://sha256/"));

// 通过 URI 查找。
let body = store.get(&fr.uri)?.unwrap();
assert_eq!(body.into_bytes()?, b"payload");

// 按生产作业筛选列出制品。
let refs = store.list(ArtefactFilter {
    producer_job_id: Some(job_id),
    ..Default::default()
})?;
```

## 工作流集成

工作流运行器通过步骤输出传播 `FileRef`：

1. 一个步骤发出一个 `ToolResult`，其 `context` 包含
   `{"file_refs": [{"uri": "artefact://sha256/...", "mime": "image/png"}]}`。
2. 运行器将 `FileRef` 存储在步骤记录上，并将其替换到下一步骤的
   参数上下文中。
3. 下游步骤通过 `artefact_get_bytes(uri)` 获取字节 — 或者当从
   运行器进程外部使用 MCP resources 原语时通过 `resources/read` 获取。

本 PR 落地了类型、存储和 resource 接线。运行器集成超出范围。

## 注意事项

- **重复内容 → 相同 URI。** 不要依赖 URI 的唯一性进行逻辑排序 —
  改用 `producer_job_id` 和 `metadata`。
- **`put_file` 会复制。** 源路径保持不动；存储拥有规范副本。
- **Sidecar 对元数据是权威的。** 手动编辑 JSON sidecar 是支持的；
  编辑 `.bin` 文件则不支持（digest 会不匹配）。
- **尚无 GC。** 存储永远不会自动删除。TTL / 引用计数 GC 在一个
  未来的 issue 中追踪。
- **尚无远程后端。** S3 / SFTP / HTTP 指针已声明在路线图中但尚未
  实现 — 一个未来的 issue。

## 参见

- [`docs/api/resources.md`](../api/resources.md) — 承载 `artefact://` 的
  更广泛的 Resources 原语。
- Issue [#348](https://github.com/loonghao/dcc-mcp-core/issues/348) —
  消费 `FileRef` 的工作流运行器。
- Issue [#350](https://github.com/loonghao/dcc-mcp-core/issues/350) —
  Resources 原语（已合并）。
