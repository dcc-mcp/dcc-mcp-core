# Rez Skill Packages

Rez is a good fit for distributing DCC MCP skills because it already resolves
project, department, task, asset, and DCC-specific package contexts before an
application starts. `dcc-mcp-core` does not replace Rez; it reads the resolved
environment and exposes only the active skill surface.

## Package Layout

Keep each package scoped to one production concern instead of shipping a single
studio-wide package with every tool.

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

`SKILL.md` extensions should stay under `metadata.dcc-mcp.*` and point to
sibling files such as `tools.yaml`, `groups.yaml`, `prompts.yaml`, and recipes.
Package-level files can live beside `skills/` and be added to the environment
when the context is resolved.

## Environment Contract

Use the generic path variables for shared packages and DCC-specific variables
when a package should load only in one host.

| Variable | Purpose |
|----------|---------|
| `DCC_MCP_SKILL_PATHS` | Shared skill directories, using the platform path separator |
| `DCC_MCP_<DCC>_SKILL_PATHS` | DCC-specific skill directories, for example `DCC_MCP_MAYA_SKILL_PATHS` |
| `DCC_MCP_RESOURCE_PATHS` | Shared MCP resource roots |
| `DCC_MCP_PROMPT_PATHS` | Shared prompt roots |
| `DCC_MCP_CONTEXT_BUNDLE` | Stable bundle id such as `show-a.seq010.shot020.lighting` |
| `DCC_MCP_PRODUCTION_DOMAIN` | Broad domain such as `film`, `advertising`, `game`, or `asset` |
| `DCC_MCP_CONTEXT_KIND` | Context shape such as `shot`, `deliverable`, `level`, or `asset` |
| `DCC_MCP_TOOLSET_PROFILE` | Default profile name used by the adapter or gateway |
| `DCC_MCP_PACKAGE_PROVENANCE` | Semicolon-separated package/version provenance for audit output |

DCC-specific paths are additive. A Maya launch can resolve shared studio skills
through `DCC_MCP_SKILL_PATHS` and add shot lighting tools through
`DCC_MCP_MAYA_SKILL_PATHS`; a Houdini launch in the same shot would resolve a
different DCC-specific path while keeping the same bundle id.

## Rez Example

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

`DccServerBase` copies these context values into `McpHttpConfig.instance_metadata`.
The gateway then surfaces them through the `gateway://instances` MCP resource,
so clients can route to the already-launched instance that matches the
requested bundle.

## Launching the server from the core package

A resolved `dcc_mcp_core` is a library, so the package itself has to answer
"how do I start this MCP server?". `dcc-mcp-core` is that answer. It is a
console script, so a Rez package has something concrete to declare under
`tools` instead of leaving the list empty:

```python
# package.py — the dcc_mcp_core package itself
name = "dcc_mcp_core"
version = "0.20.42"  # pin to the dcc_mcp_core release you package

tools = ["dcc-mcp-core"]

def commands():
    env.PYTHONPATH.prepend("{root}/site-packages")
```

With that package resolved, a server starts without any adapter code:

```bash
rez-env dcc_mcp_core -- dcc-mcp-core serve --dcc maya
# dcc-mcp-core serving maya at http://127.0.0.1:57448/mcp
```

`--port` defaults to `0`, which asks the OS for a free port; the resolved URL is
printed on stdout so scripts can capture it. Add `--json` for one parseable
object, and `--skill-path <dir>` (repeatable) for skill directories the Rez
package does not already contribute through `DCC_MCP_<DCC>_SKILL_PATHS`.

| Command | Use |
|---|---|
| `dcc-mcp-core --version` | Which build is resolved |
| `dcc-mcp-core info` | Print the whole launch contract, including the `tools` line above |
| `dcc-mcp-core serve --dcc <name>` | Start a Skills-First MCP server for one DCC |

`python -m dcc_mcp_core` is the same entry point for environments that only put
the package on `PYTHONPATH`.

The equivalent launch is a documented top-level API, so consumers never have to
import an internal module:

```python
from dcc_mcp_core import McpHttpConfig, create_skill_server

server = create_skill_server("maya", McpHttpConfig(port=8765))
handle = server.start()
print(handle.mcp_url())
```

`create_adapter_server(dcc_name, config, options)` is the lower-level factory
`DccServerBase` builds on; prefer `create_skill_server` unless you own the
`DccServerOptions` already. Both are exported from the package root, so
`dir(dcc_mcp_core)` lists them.

## Provenance

Emit provenance as package identifiers rather than absolute build paths. A
compact value such as
`show_a_lighting_mcp_skills-3.4.1;maya_scene_skills-1.2.0` is easier to search
in audit logs and avoids leaking workstation paths.

Skill provenance should line up with package provenance:

- `SKILL.md` declares the skill version and `metadata.dcc-mcp.layer`.
- `package.py` declares the Rez package version and contributes
  `DCC_MCP_PACKAGE_PROVENANCE`.
- Audit/debug output can show both the loaded skill version and the package set
  that made it available.

## Migration Notes

For existing loose skill directories, create one Rez package per context slice
and move the directory under `skills/`. Start with shared studio primitives,
then split project, department, and task packages only where the tool surface is
meaningfully different. Avoid a catch-all package that every department loads by
default; it recreates MCP context bloat and makes `tools/list` harder for agents
to reason about.

See also [context-bundles.md](context-bundles.md), [gateway.md](gateway.md),
the toolset-profile design in
[#611](https://github.com/dcc-mcp/dcc-mcp-core/issues/611), instruction
resources in [#608](https://github.com/dcc-mcp/dcc-mcp-core/issues/608), and
recipe packs in [#616](https://github.com/dcc-mcp/dcc-mcp-core/issues/616).
