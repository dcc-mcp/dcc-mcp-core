# Skills over MCP 集成指南

[English](../../guide/mcp-skills-integration.md)

**dcc-mcp-core 支持官方 Skills over MCP 扩展**。兼容客户端可以先查看
Agent Skill 的元数据，再按需读取 `SKILL.md` 和配套文件。
[官方 Skills 扩展文档](https://modelcontextprotocol.io/extensions/skills/overview)

## 选择连接入口

| 任务 | 入口 |
| --- | --- |
| 发现并读取 Agent Skill | 连接目标 DCC 的 `mcp_url`，确认它声明了 `io.modelcontextprotocol/skills`。 |
| 在已打开的 DCC 会话里查找和运行工具 | 使用 `dccmcp` 和 `dcc-mcp` Skill，按搜索结果中的 `next_step` 操作。 |
| 继续使用旧 MCP 集成 | 沿用会话握手。DCC 实例端点可协商 `2025-11-25`、`2025-06-18` 或 `2025-03-26`。 |

从网关开始时，先读取 `gateway://instances`，找到打开了目标场景或文档的
会话，再取它的 `mcp_url`。扩展是否可用取决于实际端点、构建和配置。
`dcc-mcp-core` 的原生 Python wheel 已启用 MCP `2026-07-28`。
自行编译时需启用对应的 Cargo feature。
[协议迁移指南](../../guide/mcp-2026-07-28-migration.md)

## 发现并读取 Skill

[README 示例](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/README_zh.md#用-python-启动一个-skill-服务)
无需打开 Photoshop 即可提供示例 Skill。启动服务后，把终端输出的地址填到
下面的 Bash 命令中。

```bash
curl -sS http://127.0.0.1:8765/mcp \
  -H 'Content-Type: application/json' \
  -H 'Accept: application/json, text/event-stream' \
  -H 'MCP-Protocol-Version: 2026-07-28' \
  -H 'Mcp-Method: server/discover' \
  -d '{
    "jsonrpc": "2.0",
    "id": 1,
    "method": "server/discover",
    "params": {
      "_meta": {
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {}
      }
    }
  }'
```

1. 查看响应中的 `supportedVersions` 和
   `capabilities.extensions["io.modelcontextprotocol/skills"]`。
2. 用 `skills/list` 浏览 Skill，或在已知 URI 时用 `skills/get` 查询。
   每条记录包含元数据和文件清单。
3. 用清单里的完整 URI 调用 `resources/read`，再校验返回字节的 SHA-256。
   扩展声明 `directoryRead` 时，可通过 `resources/directory/read` 浏览目录。
4. 需要激活 DCC 工具时，继续按现有 `load_skill` 流程操作。读取 Skill 文件
   不会改变工具的激活状态。

每次请求都要携带新版协议要求的 HTTP 头和元数据。方法细节及文件发布规则见
[Skills over MCP API 参考](../../api/skills-extension.md)。

## 继续通过 CLI 操作

```bash
dccmcp list
dccmcp search --query "list objects" --dcc-type maya
```

按搜索命中的 `next_step` 加载 Skill、检查参数或调用工具。CLI 仍是有 shell
的智能体的默认入口，现有脚本可以继续通过网关 REST 使用。较老的安装可能
仍使用 `dcc-mcp-cli` 这个可执行名称。直连 DCC 的旧 MCP 客户端可以继续用
`search_skills` 查找 Skill，并通过 `load_skill` 激活工具。
[CLI 参考](cli-reference.md)

编写自己的 Skill 时，用 `SKILL.md` 描述工作流，在 `tools.yaml` 中声明工具
和参数，并由配套脚本执行。详见 [Skills 编写指南](skills.md)。
