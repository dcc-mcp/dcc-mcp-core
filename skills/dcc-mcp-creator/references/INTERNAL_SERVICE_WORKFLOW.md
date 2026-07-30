# Internal Standalone Service Workflow

Use this path when the target is a private API, command-line service, asset
database, render farm, review system, or another non-DCC system. The source may
live in a local folder, intranet monorepo, Perforce workspace, or private Git
server. GitHub and the public DCC catalog are optional delivery choices, not
prerequisites.

## 1. Inspect Before Scaffolding

Read the supplied project and reuse its language, package manager, service
entry point, authentication, tests, and deployment path. Create a new project
only when no owning codebase exists. Do not add an adapter bridge when typed
tools can call an existing library or bounded HTTP/CLI client directly.

Choose one boundary:

| Need | Build |
|---|---|
| Expose an existing private service | Standalone `DccServerBase` composition root plus typed Skills |
| Add tools to an existing DCC-MCP runtime | A Skill package with `dcc-mcp-skills-creator` |
| Control a GUI or host-thread-only API | A DCC adapter with `HostExecutionBridge` |

## 2. Start With the Standalone Runtime

Use a stable custom service id that describes ownership, such as
`studio-assets`. Do not reuse a DCC name to bypass routing.

```python
from pathlib import Path

from dcc_mcp_core import DccServerBase, DccServerOptions

skills_dir = Path(__file__).parent / "skills"
options = DccServerOptions.from_env(
    "studio-assets",
    skills_dir,
    server_name="studio-assets-mcp",
    instance_type="standalone",
)
server = DccServerBase(options)
server.register_builtin_actions(include_bundled=False)
handle = server.start()
print(handle.mcp_url())
```

Leave `dcc_pid` unset. `instance_type="standalone"` describes the service
lifetime; it does not mean `standalone_main_thread=True`. Keep default inline
execution for ordinary library, file, and network operations. Add a dispatcher
or bridge only when a real thread/process boundary requires one.

The complete runnable example is
[`examples/remote-server`](../../../examples/remote-server). Its historical
directory name is retained for stable links, but its default is a loopback,
standalone internal service.

## 3. Put Business Operations in Typed Skills

Load `dcc-mcp-skills-creator` and create one small Skill around a user workflow,
not one tool per private API endpoint. Every tool must have explicit input and
output schemas, safety annotations, bounded timeouts, and actionable errors.
Keep credentials in the owner's secret store or process environment; never put
them in `SKILL.md`, `tools.yaml`, examples, logs, or result payloads.

Validate before starting the service:

```bash
dcc-mcp-cli lint skills
```

For hermetic tests, set `DCC_MCP_DISABLE_DEFAULT_SKILL_PATHS=1` so operator
Skill directories cannot change discovery results.

When the owner needs a reproducible development environment, prefer the open
Development Container specification and its open-source CLI over a bespoke
sandbox. Reuse an existing `.devcontainer/devcontainer.json`; the runnable
example includes one under `examples/remote-server`. Keep credentials outside
the image and do not mount a host container socket unless the workflow truly
needs nested container control.

## 4. Play and Debug Locally

Start on loopback and print the resolved MCP URL. Then run the official
open-source [MCP Inspector](https://github.com/modelcontextprotocol/inspector):

```bash
npx @modelcontextprotocol/inspector@latest
```

Connect with Streamable HTTP to the printed `/mcp` URL. Verify this ladder in
order:

1. `tools/list` exposes only discovery/control tools before the Skill loads.
2. List or search confirms the owning Skill and its exact slug.
3. Load the Skill and inspect its schema.
4. Call one read-only tool with a valid example.
5. Call it once with invalid input and confirm the error is safe and actionable.
6. Stop the service and confirm its registry row disappears.

Use `dcc-mcp-cli list`, `load-skill`, `describe`, and `call --wait` as
the agent smoke once the local service is registered. Keep the returned slug
and `request_id`; do not guess names or retry before diagnosis.

For a hosted multi-user teaching portal, Educates is the open-source upgrade
path: it provides per-user isolated sessions, Markdown instructions, browser
terminals, and an embedded editor. Treat its Kubernetes, ingress, identity,
resource quota, image registry, and session-cleanup requirements as an
operator-owned deployment project. A local Dev Container remains the default
until that operational need exists.

## 5. Expose and Deliver Privately

Loopback is the development default. Before binding to an intranet interface,
require the operator-owned security boundary: TLS termination, authentication,
origin/network allow-lists, secret management, audit retention, and shutdown
ownership. Never expose the Inspector proxy to an untrusted network.

Use the existing internal delivery path: wheel, signed archive, container, Rez
package, shared deployment system, or private package registry. A studio-owned
`DCC_MCP_CATALOG_PATH` is useful only when operators need catalog-backed CLI
install plans. Local execution and direct MCP testing do not require a public
catalog or GitHub repository.

## Acceptance

- The service starts from the owner's documented command on a clean environment.
- One typed read-only tool succeeds through MCP Inspector and the agent path.
- Invalid input and unavailable dependency failures are structured and redacted.
- The service is marked `standalone`, has no fake DCC PID, and shuts down cleanly.
- Packaging uses the owner's private delivery channel with no accidental public publication.
