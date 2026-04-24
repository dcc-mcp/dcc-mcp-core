# Remote-First MCP 服务器设计指南

> **[English version](../guide/remote-server.md)**

本指南解释何时应选择远程 MCP 服务器而非本地 socket，
如何部署 `create_skill_server()` 使其可被云端托管的
智能体（Claude.ai、Cursor、ChatGPT、VS Code）访问，以及需要什么
CORS / 认证配置。

---

## 为什么需要远程？

远程 MCP 服务器是唯一能够同时在 **Web、移动端和云端托管智能体**
工作的配置。

| 路径 | 最适合 | 限制 |
|------|--------|------|
| 直接 API 调用 | 小型、非复用集成 | 规模扩大后出现 M×N 集成问题 |
| CLI / 本地 socket | 开发者工作站、嵌入式插件 | 无法从移动端/Web/云端访问，除非有 shell |
| **远程 MCP 服务器** | 云端托管智能体、最大覆盖范围 | 一次性的小设置投入 |

部署后，远程服务器成为一个**复利层**：每一个新的
MCP 规范能力（Elicitation、MCP Apps、OAuth、Tool Search…）都会
自动落地到每一个兼容客户端，无需更改服务器的
工具实现。

---

## 快速开始：最小远程服务器

```python
from dcc_mcp_core import create_skill_server, McpHttpConfig

cfg = McpHttpConfig(
    port=8765,
    host="0.0.0.0",           # 绑定到所有接口 —— 远程访问必需
    server_name="maya-mcp",
    enable_cors=True,          # Web / 浏览器客户端必需
)

server = create_skill_server("maya", cfg)
handle = server.start()
print(handle.mcp_url())        # "http://0.0.0.0:8765/mcp"
```

> **提示**：任何非 localhost 客户端都需要 `host="0.0.0.0"`。
> 暴露到互联网时，请将其置于防火墙后或使用认证（见下文）。

---

## McpHttpConfig 远程部署选项

| 属性 | 远程默认值 | 说明 |
|------|-----------|------|
| `host` | `"0.0.0.0"` | 绑定到所有接口（默认 `"127.0.0.1"` = 仅 localhost） |
| `port` | `8765` | 必须可通过防火墙 / NAT 访问 |
| `enable_cors` | `True` | 浏览器和 Claude.ai Web 客户端必需 |
| `allowed_origins` | `["*"]` | 生产环境限制为特定客户端来源 |
| `spawn_mode` | `"dedicated"` | PyO3 嵌入宿主始终使用 `"dedicated"` |
| `api_key` | 环境变量 | 可选 Bearer token 认证（见 [Auth](#auth)） |
| `enable_oauth` | `False` | OAuth 2.1 + CIMD 认证（见 [OAuth](#oauth--cimd)） |

```python
cfg = McpHttpConfig(
    host="0.0.0.0",
    port=8765,
    enable_cors=True,
    allowed_origins=["https://claude.ai", "https://cursor.sh"],
    spawn_mode="dedicated",    # DCC 嵌入宿主始终使用
)
```

---

## Auth

### API Key（最简单）

对于 OAuth 不实用的工作室环境：

```python
import os
cfg = McpHttpConfig(port=8765, host="0.0.0.0")
cfg.api_key = os.environ.get("DCC_MCP_API_KEY")  # Bearer token
```

客户端在每次请求中携带 `Authorization: Bearer <key>`。

### OAuth 2.1 + CIMD（生产环境推荐）

见下方的 [CIMD OAuth 指南](remote-server.md#oauth--cimd) 和 issue #408。
使用 `McpHttpConfig(enable_oauth=True)` 启用。

---

## CORS 配置

当 MCP 客户端运行在浏览器中（Claude.ai、任何基于 Web 的智能体 UI）
或在 Cursor / VS Code 中时，都需要 CORS 头。

```python
cfg = McpHttpConfig(enable_cors=True)

# 生产环境：限制为已知来源
cfg.allowed_origins = [
    "https://claude.ai",
    "https://cursor.sh",
    "https://vscode.dev",
]
```

当 `enable_cors=True` 且 `allowed_origins` 为空（默认）时，服务器
发送 `Access-Control-Allow-Origin: *` —— 开发方便但
**不推荐用于生产环境**。

---

## 容器 / VPS 部署

公开 MCP 服务器的最小 Docker 设置：

```dockerfile
FROM python:3.11-slim
RUN pip install dcc-mcp-core
COPY skills/ /opt/skills/
ENV DCC_MCP_SKILL_PATHS=/opt/skills
ENV DCC_MCP_API_KEY=change-me
EXPOSE 8765
CMD ["python", "-c", "
from dcc_mcp_core import create_skill_server, McpHttpConfig
import os, time
cfg = McpHttpConfig(host='0.0.0.0', port=8765, enable_cors=True)
cfg.api_key = os.environ.get('DCC_MCP_API_KEY')
server = create_skill_server('generic', cfg)
handle = server.start()
print(handle.mcp_url())
while True: time.sleep(60)
"]
```

构建并运行：

```bash
docker build -t my-mcp-server .
docker run -p 8765:8765 -e DCC_MCP_API_KEY=secret my-mcp-server
```

---

## 示例：最小可远程访问 Skill 服务器

见 [`examples/remote-server/`](https://github.com/loonghao/dcc-mcp-core/tree/main/examples/remote-server) 获取一个
完整的、可部署的示例，包含：

- 在 `0.0.0.0:8765` 启动可公开访问的 MCP 服务器
- 从环境变量启用 CORS 和 API-key 认证
- 包含一个最小的 `hello-world` skill
- 提供 `Dockerfile` 和 `docker-compose.yml`

---

## Remote-First 检查清单

为 DCC 适配器部署远程访问时使用此检查清单：

- [ ] 服务器绑定到 `0.0.0.0`（而非仅 `127.0.0.1`）
- [ ] 认证已配置：API key (`cfg.api_key`) 或 OAuth (`cfg.enable_oauth = True`)
- [ ] CORS 已启用 (`cfg.enable_cors = True`)，且生产环境限制 `allowed_origins`
- [ ] Tool 描述遵循 3 层行为结构（issue #341）
- [ ] Tools 按用户意图分组，而非 1:1 对应 API 端点
- [ ] DCC 嵌入宿主（Maya、Blender…）使用 `McpHttpConfig.spawn_mode = "dedicated"`
- [ ] 防火墙 / 安全组中端口 8765 已开放
- [ ] TLS 在反向代理（nginx、Caddy、AWS ALB）处终止，面向互联网的部署
- [ ] `DCC_MCP_API_KEY` 设置为环境变量 —— 切勿硬编码
- [ ] 文件日志已启用（`enable_file_logging=True`，默认值）用于审计追踪

---

## OAuth / CIMD

> 完整指南：issue #408 —— CIMD OAuth 支持计划在未来版本中推出。

当 `McpHttpConfig.enable_oauth = True` 时，服务器将暴露：

```
GET /.well-known/oauth-client-metadata
```

返回 CIMD 文档，实现无需手动客户端 ID 设置的自动客户端注册。
这是生产环境云端部署的推荐方案。

通过 Claude Managed Agents Vaults 注入 Token：在 Vault 中注册一次 OAuth token；
平台会为每个 MCP 连接自动注入和刷新凭证。

---

## TLS / HTTPS

`McpHttpServer` 绑定纯 HTTP。在反向代理处终止 TLS：

```nginx
server {
    listen 443 ssl;
    server_name mcp.example.com;

    ssl_certificate     /etc/letsencrypt/live/mcp.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/mcp.example.com/privkey.pem;

    location /mcp {
        proxy_pass http://127.0.0.1:8765;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        # SSE 需要禁用缓冲
        proxy_buffering off;
        proxy_cache off;
    }
}
```

---

## 连接 MCP 客户端

部署后，将服务器 URL 添加到你的 MCP 客户端：

**Claude Desktop** (`claude_desktop_config.json`)：
```json
{
  "mcpServers": {
    "maya-mcp": {
      "url": "https://mcp.example.com/mcp",
      "headers": { "Authorization": "Bearer YOUR_API_KEY" }
    }
  }
}
```

**Cursor** (`~/.cursor/mcp.json`)：
```json
{
  "mcpServers": {
    "maya-mcp": {
      "url": "https://mcp.example.com/mcp",
      "headers": { "Authorization": "Bearer YOUR_API_KEY" }
    }
  }
}
```

---

## 另见

- [生产环境部署](production-deployment.md) —— Docker、systemd、k8s HA 拓扑
- [网关选举机制](gateway-election.md) —— 多实例网关故障转移
- [快速开始](getting-started.md) —— 本地开发设置
- [`docs/api/http.md`](../api/http.md) —— 完整的 `McpHttpConfig` 参考
