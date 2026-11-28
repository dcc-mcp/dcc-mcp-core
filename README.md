# dcc-mcp-core

Connect an AI agent or pipeline to the creative applications already running in your studio. With dcc-mcp-core, a client can search for a tool in Maya, Blender, or Photoshop and run it in the session that owns the scene or document. The shared runtime is written in Rust and Python and speaks the Model Context Protocol (MCP) and REST.

[![PyPI](https://img.shields.io/pypi/v/dcc-mcp-core?label=PyPI)](https://pypi.org/project/dcc-mcp-core/) [![CI](https://img.shields.io/github/actions/workflow/status/dcc-mcp/dcc-mcp-core/ci.yml?branch=main&label=CI)](https://github.com/dcc-mcp/dcc-mcp-core/actions/workflows/ci.yml) [![MIT](https://img.shields.io/badge/license-MIT-green.svg)](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/LICENSE)

**English** · [简体中文](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/README_zh.md) · [日本語](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/README_ja.md) · [한국어](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/README_ko.md) · [Español](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/README_es.md) · [Français](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/README_fr.md) · [Deutsch](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/README_de.md) · [Português (Brasil)](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/README_pt-BR.md) · [Русский](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/README_ru.md) · [Türkçe](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/README_tr.md)

## Skills over MCP

**dcc-mcp-core supports the official Skills over MCP extension** (`io.modelcontextprotocol/skills`) and MCP `2026-07-28` stateless discovery. A compatible client can discover Agent Skills on a per-DCC endpoint that advertises the extension, read `SKILL.md` and supporting files on demand, and verify each file against its SHA-256 digest. See the [Skills over MCP integration guide](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/docs/guide/mcp-skills-integration.md).

Existing clients and scripts can keep using **`dccmcp` over the gateway's REST API**. Per-DCC MCP servers also accept legacy session clients using `2025-11-25`, `2025-06-18`, or `2025-03-26`. See the [protocol migration guide](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/docs/guide/mcp-2026-07-28-migration.md).

## Find a tool in a running application

Install the [`dccmcp` CLI](https://github.com/dcc-mcp/dcc-mcp-cli), then open the application with its [DCC-MCP adapter](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/docs/adapter-coverage.md) enabled. If you're working through an agent, the [`dcc-mcp` Skill](https://clawhub.ai/loonghao/skills/dcc-mcp) teaches it to use the same CLI. For example, to find a tool that lists objects in Maya, start with these two read-only commands.

~~~bash
dccmcp list
dccmcp search --query "list objects" --dcc-type maya
~~~

`list` shows the sessions registered with the gateway. A search hit identifies the tool by `tool_slug`. Follow the hit's `next_step` to load the relevant Skill, inspect the accepted arguments, or call the tool directly. Searching alone leaves the scene untouched.

If Maya is missing from the list, check that its adapter is running and that the CLI is using the intended gateway. Run `dccmcp doctor` to inspect the connection. If a request fails, keep its `request_id` so you can trace it in the gateway logs. See the [CLI reference](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/docs/guide/cli-reference.md) for gateway profiles and the [remote workstation guide](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/docs/guide/remote-server.md) for deployment.

## Serve a Skill from Python

You can try the server without opening a DCC application. From the root of a checkout, install the Python package in a virtual environment.

~~~bash
python -m pip install --upgrade "dcc-mcp-core>=0.20.34"
~~~

Run the following as a Python script. It serves the repository's [Photoshop layer example](https://github.com/dcc-mcp/dcc-mcp-core/tree/main/examples/rez-skills/commercial_2d_layers/skills/commercial-2d-layers) on an available local port. Connect an MCP client to the URL it prints. The server keeps listening until you press Ctrl+C.

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

Use the printed per-DCC URL to try `server/discover`, check for `io.modelcontextprotocol/skills`, then find `commercial-2d-layers` with `skills/list`. A legacy MCP client can find the same package with the `search_skills` tool.

The example shows how to package a tool. [`SKILL.md`](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/examples/rez-skills/commercial_2d_layers/skills/commercial-2d-layers/SKILL.md) identifies the host and points to [`tools.yaml`](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/examples/rez-skills/commercial_2d_layers/skills/commercial-2d-layers/tools.yaml), where `summarize_layers` declares an optional `document` argument and read-only access. Its [Python script](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/examples/rez-skills/commercial_2d_layers/skills/commercial-2d-layers/scripts/summarize_layers.py) returns a fixed sample summary. To work on an open document, implement the handler in the host's adapter. Check the package declaration with the CLI.

~~~bash
dccmcp lint examples/rez-skills/commercial_2d_layers/skills
~~~

Use the [Skills guide](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/docs/guide/skills.md) to define and validate your own tools, or browse existing packages in the [Marketplace](https://dcc-mcp.github.io/marketplace). For a studio that ships tools with Rez, the [Rez guide](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/docs/guide/rez-skill-packages.md) shows how to start a server from a resolved package.

## How a call reaches the right session

![Path from a client through the gateway to a DCC host](https://raw.githubusercontent.com/dcc-mcp/dcc-mcp-core/main/docs/assets/architecture/current-stack.svg)

A client sends its request to the gateway. The gateway finds the registered session and routes the call to its adapter. The supervisor tracks whether that adapter is ready. The adapter runs the tool inside the application and dispatches work to the main thread when the host API requires it.

- **Choose a session.** If two copies of the same application are open, read `gateway://instances` and check each entry's `mcp_url` and dispatch status before choosing a target. See the [gateway guide](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/docs/guide/gateway.md).
- **Write a client.** Over HTTP, use `POST /v1/search` to find a tool, then send its `tool_slug` and `arguments` to `POST /v1/call` when the tool is ready. To retrieve Agent Skill files over MCP, use the selected session's `mcp_url` and check its extension declaration. See the [REST API](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/docs/guide/rest-api-surface.md) and [Skills over MCP reference](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/docs/api/skills-extension.md).
- **Repeat a task.** A workflow can record a multi-step job and resume it when persistent storage is configured. For a step that only the desktop interface exposes, UI Control binds input to a specific window. See [workflows](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/docs/guide/workflows.md) and [UI Control](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/docs/guide/ui-control-workflows.md).

## Supported applications

Selected hosts include Maya, Blender, Houdini, Photoshop, Unreal, Unity, Godot, and RenderDoc. Use `dccmcp dcc-types` to see which adapters ship in the release catalog. The [complete generated host list](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/docs/adapter-coverage.md) names the adapter for each host and includes installation details where available.

To check the count in this checkout, run the following from the repository root.

~~~bash
rg -c '^    tags: \[.*"adapter".*\]' dcc-mcp-catalog.yml
~~~

That reports **49 adapter entries** in [`dcc-mcp-catalog.yml`](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/dcc-mcp-catalog.yml). To connect an in-house application, follow the [adapter onboarding guide](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/docs/guide/new-adapter-onboarding.md).

## Contributing

From a checkout, use the project's pinned toolchain to build the development package and run the Python and Rust tests.

~~~bash
vx just install
vx just dev
vx just test
vx just test-rust
~~~

For documentation changes, run `vx just docs-check`. When a command or response field changes, update the example in each README. Choose wording that reads naturally in the target language and keep the executable examples in sync. See the [contribution guide](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/CONTRIBUTING.md) and [documentation index](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/docs/guide/INDEX.md).

dcc-mcp-core is available under the [MIT license](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/LICENSE). [Project website](https://dcc-mcp.github.io/) · [Showcase](https://dcc-mcp.github.io/showcase) · [Releases](https://github.com/dcc-mcp/dcc-mcp-core/releases) · [MCP Registry](https://registry.modelcontextprotocol.io/?q=io.github.dcc-mcp%2Fdcc-mcp-core)
