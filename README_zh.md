# dcc-mcp-core

让 AI 智能体和制作管线用上工作室里的创作软件。通过 dcc-mcp-core，客户端可以在 Maya、Blender 或 Photoshop 中查找工具，再到打开了目标场景或文档的那一个会话中执行。底层运行时由 Rust 和 Python 实现，对外提供 MCP（模型上下文协议）和 REST 接口。

[![PyPI](https://img.shields.io/pypi/v/dcc-mcp-core?label=PyPI)](https://pypi.org/project/dcc-mcp-core/) [![CI](https://img.shields.io/github/actions/workflow/status/dcc-mcp/dcc-mcp-core/ci.yml?branch=main&label=CI)](https://github.com/dcc-mcp/dcc-mcp-core/actions/workflows/ci.yml) [![MIT](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)

[English](README.md) · **简体中文** · [日本語](README_ja.md) · [한국어](README_ko.md) · [Español](README_es.md) · [Français](README_fr.md) · [Deutsch](README_de.md) · [Português (Brasil)](README_pt-BR.md) · [Русский](README_ru.md) · [Türkçe](README_tr.md)

## Skills over MCP

**dcc-mcp-core 支持官方 Skills over MCP 扩展**（`io.modelcontextprotocol/skills`）和 MCP `2026-07-28` 无状态发现。兼容客户端可以在声明该扩展的 DCC 实例端点上发现 Agent Skills，按需读取 `SKILL.md` 和配套文件，并用 SHA-256 校验文件内容。详见 [Skills over MCP 集成指南](docs/zh/guide/mcp-skills-integration.md)。

旧客户端和脚本可继续使用 **`dccmcp` 通过网关 REST API 操作**。DCC 实例的 MCP 服务也兼容 `2025-11-25`、`2025-06-18` 和 `2025-03-26` 会话客户端。详见[协议迁移指南](docs/guide/mcp-2026-07-28-migration.md)。

## 在已打开的软件里查找工具

先安装 [`dccmcp` 命令行工具](https://github.com/dcc-mcp/dcc-mcp-cli)，在目标软件中启用对应的 [DCC-MCP 适配器](docs/adapter-coverage.md)。如果由智能体操作，可以给它安装 [`dcc-mcp` Skill](https://clawhub.ai/loonghao/skills/dcc-mcp)。以 Maya 为例，要找一个能列出场景对象的工具，可以先运行两条只读命令。

~~~bash
dccmcp list
dccmcp search --query "list objects" --dcc-type maya
~~~

第一条列出已向网关注册的会话。第二条在 Maya 的工具中搜索，命中后会返回工具标识 `tool_slug`。接下来按结果中的 `next_step` 操作。它会告诉你需要先加载哪个 Skill、是否要查看参数，或者能否直接调用。搜索本身不会改动场景。

如果列表里没有 Maya，先确认适配器已经启动，再检查 CLI 当前连接的是哪个网关。`dccmcp doctor` 可以帮助定位连接问题。如果某次请求失败，记下它的 `request_id`，方便查找对应的网关日志。网关配置见[命令行参考](docs/zh/guide/cli-reference.md)，跨工作站部署见[远程服务指南](docs/zh/guide/remote-server.md)。

## 用 Python 启动一个 Skill 服务

想先试试服务端，可以直接使用仓库里的示例。克隆仓库后，在根目录激活 Python 虚拟环境，安装核心包。

~~~bash
python -m pip install dcc-mcp-core
~~~

把下面的代码作为 Python 脚本运行。它会在本机选择一个空闲端口，提供 [Photoshop 图层示例](examples/rez-skills/commercial_2d_layers/skills/commercial-2d-layers)。把终端输出的 URL 填入 MCP 客户端即可连接。需要结束服务时，在运行脚本的终端按 Ctrl+C。

~~~python
import time

from dcc_mcp_core import McpHttpConfig, create_skill_server

config = McpHttpConfig(port=0)
config.gateway_port = 0
server = create_skill_server(
    "photoshop",
    config,
    extra_paths=["examples/rez-skills/commercial_2d_layers/skills"],
    accumulated=False,
)
handle = server.start()
try:
    print(handle.mcp_url(), flush=True)
    while True:
        time.sleep(1)
except KeyboardInterrupt:
    pass
finally:
    handle.shutdown()
~~~

在终端输出的 DCC 实例端点上调用 `server/discover`，确认它声明了 `io.modelcontextprotocol/skills`，再通过 `skills/list` 找到 `commercial-2d-layers`。旧 MCP 客户端可以用 `search_skills` 找到同一个包。

这个示例适合用来理解一个工具的组成。[`SKILL.md`](examples/rez-skills/commercial_2d_layers/skills/commercial-2d-layers/SKILL.md) 指明适用的软件和工具清单的位置。[`tools.yaml`](examples/rez-skills/commercial_2d_layers/skills/commercial-2d-layers/tools.yaml) 将 `summarize_layers` 声明为只读工具，并定义可选参数 `document`。随附的 [Python 脚本](examples/rez-skills/commercial_2d_layers/skills/commercial-2d-layers/scripts/summarize_layers.py) 返回一份固定的示例摘要。如果要读取当前打开的文档，需要在适配器里实现相应的处理逻辑。可以用这条命令检查包的声明。

~~~bash
dccmcp lint examples/rez-skills/commercial_2d_layers/skills
~~~

编写自己的工具时，可以对照 [Skills 指南](docs/zh/guide/skills.md)。要复用已有工具，可以在 [Marketplace](https://dcc-mcp.github.io/marketplace) 中查找。如果工作室通过 Rez 分发工具，[Rez 指南](docs/zh/guide/rez-skill-packages.md)说明了如何在已解析的包环境中启动服务。

## 一次调用如何到达目标会话

![客户端经由网关调用 DCC 应用的路径](docs/assets/architecture/current-stack.svg)

客户端先把请求交给网关。网关根据注册信息找到目标会话，将调用转给对应的适配器。监督进程负责跟踪适配器是否就绪。工具最终在应用内执行，如果宿主 API 要求在主线程上运行，适配器负责调度。

- **选定会话。** 同时开着两个相同的软件实例时，读取 `gateway://instances`，用每条记录的 `mcp_url` 和调度状态确认目标。详见[网关指南](docs/zh/guide/gateway.md)。
- **接入客户端。** 使用 HTTP 时，先通过 `POST /v1/search` 查找工具。工具就绪后，把 `tool_slug` 和 `arguments` 发给 `POST /v1/call`。要通过 MCP 读取 Agent Skill 文件，使用目标会话的 `mcp_url`，并确认该端点已声明扩展。详见 [REST API](docs/zh/guide/rest-api-surface.md) 和 [Skills over MCP API](docs/api/skills-extension.md)。
- **重复执行任务。** 多步骤任务可以写成工作流，配置持久化存储后即可从中断处恢复。某一步只能通过桌面界面完成时，UI Control 会把输入限定在指定窗口。详见[工作流](docs/zh/guide/workflows.md)和 [UI Control](docs/zh/guide/ui-control-workflows.md)。

## 支持哪些应用

适配器覆盖 Maya、Blender、Houdini、Photoshop、Unreal、Unity、Godot、RenderDoc 等应用。运行 `dccmcp dcc-types` 可以查看当前发行版收录的适配器。[完整应用清单](docs/adapter-coverage.md)列出了对应的适配器和已有的安装信息。

要核对当前检出版本的条目数，可在仓库根目录运行以下命令。

~~~bash
rg -c '^    tags: \[.*"adapter".*\]' dcc-mcp-catalog.yml
~~~

这份[适配器目录](dcc-mcp-catalog.yml)中共有 **49 条适配器记录**。要让工作室自研的软件接入网关，可以从[适配器接入指南](docs/guide/new-adapter-onboarding.md)开始。

## 参与开发

在仓库根目录使用项目指定的工具链构建开发包，并分别运行 Python 和 Rust 测试。

~~~bash
vx just install
vx just dev
vx just test
vx just test-rust
~~~

修改文档后运行 `vx just docs-check`。如果命令或响应字段有变化，请同步更新各语言 README 中的示例。说明文字按读者的语言习惯来写，可执行示例保持一致。详见[贡献指南](CONTRIBUTING.md)和[文档目录](docs/zh/guide/INDEX.md)。

dcc-mcp-core 使用 [MIT 许可证](LICENSE)。[项目主页](https://dcc-mcp.github.io/) · [案例展示](https://dcc-mcp.github.io/showcase) · [版本发布](https://github.com/dcc-mcp/dcc-mcp-core/releases) · [MCP Registry](https://registry.modelcontextprotocol.io/?q=io.github.dcc-mcp%2Fdcc-mcp-core)
