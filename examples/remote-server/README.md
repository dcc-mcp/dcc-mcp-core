# Standalone Internal MCP Service Example

Minimal non-DCC service using `dcc-mcp-core`. It discovers the bundled
`hello-world` Skill, marks the runtime as `standalone`, and requires no DCC
process, public catalog entry, or GitHub repository.

The `remote-server` directory name is retained for stable links. Local
development binds to loopback; remote or intranet exposure is an explicit
operator choice.

## Quick Start

```bash
pip install dcc-mcp-core
python server.py
```

Expected output includes:

```text
MCP service listening at http://127.0.0.1:8765/mcp
  identity: studio-service
  lifetime: standalone
```

Validate the bundled Skill before starting:

```bash
dcc-mcp-cli lint skills
```

## Run In The Open Dev Container

The example includes a
[Development Container](https://containers.dev/) configuration with Python,
Node.js, `dcc-mcp-core`, and forwarded MCP Inspector ports. Open this directory
in a compatible editor and choose **Reopen in Container**, then run the Quick
Start commands above. The container runs as a non-root user and does not mount
the host container socket.

Agents can use the open-source reference CLI from the repository root:

```bash
npm install --global @devcontainers/cli
devcontainer up --workspace-folder examples/remote-server
devcontainer exec --workspace-folder examples/remote-server python server.py
```

In another terminal, exercise the running service without a browser:

```bash
devcontainer exec --workspace-folder examples/remote-server \
  npx --yes @modelcontextprotocol/inspector@latest --cli \
  http://127.0.0.1:8765/mcp --transport http --method tools/list
```

The Dev Container is the zero-infrastructure lab. If a team later needs a
self-hosted browser classroom with one isolated session per learner,
step-by-step instructions, terminals, and an embedded editor, use the
Apache-2.0 [Educates](https://docs.educates.dev/en/stable/) platform. Do not add
its Kubernetes/ingress operational surface until multi-user hosting is an
actual requirement.

## Play With The Open-Source Inspector

Start the official [MCP Inspector](https://github.com/modelcontextprotocol/inspector):

```bash
npx @modelcontextprotocol/inspector@latest
```

Choose Streamable HTTP and connect to `http://127.0.0.1:8765/mcp`. Search for
`hello-world`, load it, then call `hello_world__greet` with:

```json
{"name":"Agent"}
```

The response message is `Hello, Agent! (from the internal MCP service)`.

Agents can exercise the same registered service without guessing tool slugs:

```bash
dcc-mcp-cli list --output toon
dcc-mcp-cli load-skill hello-world --dcc-type studio-service --output toon
dcc-mcp-cli describe <tool-slug-returned-by-load> --output toon
dcc-mcp-cli call <tool-slug-returned-by-load> --json '{"name":"Agent"}' --wait --output toon
```

Follow the returned `next_step`; do not guess the instance-qualified tool slug.

## Intranet Or Container Use

Set `DCC_MCP_HOST=0.0.0.0` only inside a trusted network boundary. The example
does not implement application-layer authentication. Put intranet or public
traffic behind operator-owned TLS, authentication, firewall/origin policy, and
audit controls.

```bash
docker build -t internal-mcp .
docker run --rm -p 127.0.0.1:8765:8765 internal-mcp
```

Do not expose the MCP Inspector proxy to an untrusted network.

## Environment Variables

| Variable | Default | Description |
|---|---|---|
| `DCC_MCP_HOST` | `127.0.0.1` | Bind address; keep loopback for local development |
| `DCC_MCP_PORT` | `8765` | MCP service port |
| `DCC_MCP_SKILL_PATHS` | _(none)_ | Optional additional private Skill directories |

## See Also

- [Internal standalone service workflow](../../skills/dcc-mcp-creator/references/INTERNAL_SERVICE_WORKFLOW.md)
- [Remote-first deployment guide](../../docs/guide/remote-server.md)
- [Production deployment](../../docs/guide/production-deployment.md)
