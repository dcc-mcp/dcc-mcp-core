# Rez 技能包

Rez 非常适合分发 DCC MCP 技能，因为它在应用程序启动前就已经解析了项目、部门、任务、资产和 DCC 特定的包上下文。`dcc-mcp-core` 不替代 Rez；它读取已解析的环境并仅暴露活跃的技能接口。

## 包布局

将每个包的范围限定于单一制作关注点，而不是将所有工具打包到一个工作室级的巨型包中。

```text
show_a_lighting_mcp_skills/
├── package.py
├── skills/
│   └── show-a-lighting/
│       ├── SKILL.md
│       ├── tools.yaml
│       ├── prompts.yaml
│       ├── resources/
│       └── scripts/
├── resources/
└── prompts/
```

`SKILL.md` 的扩展应放在 `metadata.dcc-mcp.*` 下，并指向同级文件，如 `tools.yaml`、`groups.yaml`、`prompts.yaml` 和 recipes。包级文件可以放在 `skills/` 旁边，在上下文解析时添加到环境中。

## 环境变量契约

共享包使用通用路径变量，DCC 特定的包仅在一个宿主中加载时使用 DCC 特定变量。

| 变量 | 用途 |
|------|------|
| `DCC_MCP_SKILL_PATHS` | 共享技能目录，使用平台路径分隔符 |
| `DCC_MCP_<DCC>_SKILL_PATHS` | DCC 特定技能目录，例如 `DCC_MCP_MAYA_SKILL_PATHS` |
| `DCC_MCP_RESOURCE_PATHS` | 共享 MCP 资源根目录 |
| `DCC_MCP_PROMPT_PATHS` | 共享 Prompt 根目录 |
| `DCC_MCP_CONTEXT_BUNDLE` | 稳定的包标识符，如 `show-a.seq010.shot020.lighting` |
| `DCC_MCP_PRODUCTION_DOMAIN` | 广域领域，如 `film`、`advertising`、`game` 或 `asset` |
| `DCC_MCP_CONTEXT_KIND` | 上下文形状，如 `shot`、`deliverable`、`level` 或 `asset` |
| `DCC_MCP_TOOLSET_PROFILE` | 适配器或网关使用的默认 profile 名称 |
| `DCC_MCP_PACKAGE_PROVENANCE` | 用于审计输出的分号分隔包/版本来源信息 |

DCC 特定路径是累加的。Maya 启动可以通过 `DCC_MCP_SKILL_PATHS` 解析共享的工作室技能，并通过 `DCC_MCP_MAYA_SKILL_PATHS` 添加镜头灯光工具；同一镜头中的 Houdini 启动则会解析不同的 DCC 特定路径，同时保持相同的包标识符。

## Rez 示例

```python
name = "show_a_lighting_mcp_skills"
version = "3.4.1"
requires = ["dcc_mcp_core", "dcc_mcp_maya", "maya_scene_skills-1.2+"]

def commands():
    env.DCC_MCP_CONTEXT_BUNDLE = "show-a.seq010.shot020.lighting"
    env.DCC_MCP_PRODUCTION_DOMAIN = "film"
    env.DCC_MCP_CONTEXT_KIND = "shot"
    env.DCC_MCP_PROJECT = "show-a"
    env.DCC_MCP_SEQUENCE = "seq010"
    env.DCC_MCP_SHOT = "shot020"
    env.DCC_MCP_TASK = "lighting"
    env.DCC_MCP_TOOLSET_PROFILE = "film-shot-lighting"
    env.DCC_MCP_PACKAGE_PROVENANCE.append("{name}-{version}")
    env.DCC_MCP_MAYA_SKILL_PATHS.append("{root}/skills")
    env.DCC_MCP_RESOURCE_PATHS.append("{root}/resources")
```

`DccServerBase` 将这些上下文值复制到 `McpHttpConfig.instance_metadata`。网关随后通过 `gateway://instances` MCP 资源返回它们，客户端可以路由到匹配请求包的已启动实例。

## 从 core 包启动服务

解析出来的 `dcc_mcp_core` 只是一个库，所以包本身必须回答「这个 MCP server 要怎么启动」。答案就是 `dcc-mcp-core`：它是一个 console script，因此 Rez 包可以在 `tools` 里声明一个真实存在的可执行文件，而不是留下空列表：

```python
# package.py —— dcc_mcp_core 包自身
name = "dcc_mcp_core"
version = "0.20.42"  # 固定为你打包的 dcc_mcp_core 版本

tools = ["dcc-mcp-core"]

def commands():
    env.PYTHONPATH.prepend("{root}/site-packages")
```

解析该包后，无需任何 adapter 代码即可启动服务：

```bash
rez-env dcc_mcp_core -- dcc-mcp-core serve --dcc maya
# dcc-mcp-core serving maya at http://127.0.0.1:57448/mcp
```

`--port` 默认为 `0`，即由操作系统分配空闲端口；解析出的 URL 会打印到 stdout，便于脚本捕获。加 `--json` 可输出单个可解析对象；`--skill-path <dir>`（可重复）用于补充 Rez 包没有通过 `DCC_MCP_<DCC>_SKILL_PATHS` 贡献的技能目录。

| 命令 | 用途 |
|---|---|
| `dcc-mcp-core --version` | 当前解析到的是哪个构建 |
| `dcc-mcp-core info` | 打印完整启动契约，包含上面的 `tools` 行 |
| `dcc-mcp-core serve --dcc <name>` | 为某个 DCC 启动 Skills-First MCP 服务 |

只把包放进 `PYTHONPATH` 的环境可以用 `python -m dcc_mcp_core`，入口完全相同。

等价的启动方式也有文档化的顶层 API，消费方不必再 import 内部模块：

```python
from dcc_mcp_core import McpHttpConfig, create_skill_server

server = create_skill_server("maya", McpHttpConfig(port=8765))
handle = server.start()
print(handle.mcp_url())
```

`create_adapter_server(dcc_name, config, options)` 是 `DccServerBase` 依赖的底层工厂；除非你已经持有 `DccServerOptions`，否则优先使用 `create_skill_server`。两者都从包根导出，因此 `dir(dcc_mcp_core)` 会列出它们。

## 来源追踪

以包标识符而非绝对构建路径的形式记录来源。`show_a_lighting_mcp_skills-3.4.1;maya_scene_skills-1.2.0` 这样的紧凑值更易于在审计日志中搜索，并避免泄露工作站路径。

技能来源应与包来源对齐：

- `SKILL.md` 声明技能版本和 `metadata.dcc-mcp.layer`
- `package.py` 声明 Rez 包版本并贡献 `DCC_MCP_PACKAGE_PROVENANCE`
- 审计/调试输出可以同时显示已加载的技能版本和使其可用的包集合

## 迁移说明

对于现有的松散技能目录，为每个上下文切片创建一个 Rez 包，并将目录移动到 `skills/` 下。从共享的工作室原语开始，仅在工具接口有实质性差异时才拆分项目、部门和任务包。避免每个部门默认加载的全包；它会重新造成 MCP 上下文膨胀，使 Agent 更难以理解 `tools/list`。

参见 [context-bundles.md](context-bundles.md)、[gateway.md](gateway.md)，以及
[#611](https://github.com/dcc-mcp/dcc-mcp-core/issues/611) 中的 toolset-profile 设计、
[#608](https://github.com/dcc-mcp/dcc-mcp-core/issues/608) 中的 instruction resources 和
[#616](https://github.com/dcc-mcp/dcc-mcp-core/issues/616) 中的 recipe packs。
