# dcc-mcp-core

![dcc-mcp-core logo](docs/assets/brand/dcc-mcp-logo.png)

[![Core PyPI](https://img.shields.io/pypi/v/dcc-mcp-core?label=core%20PyPI)](https://pypi.org/project/dcc-mcp-core/)
[![Server PyPI](https://img.shields.io/pypi/v/dcc-mcp-server?label=server%20PyPI)](https://pypi.org/project/dcc-mcp-server/)
[![CI](https://img.shields.io/github/actions/workflow/status/dcc-mcp/dcc-mcp-core/ci.yml?branch=main&label=CI)](https://github.com/dcc-mcp/dcc-mcp-core/actions/workflows/ci.yml)
[![GitHub Release](https://img.shields.io/github/v/release/dcc-mcp/dcc-mcp-core?label=release)](https://github.com/dcc-mcp/dcc-mcp-core/releases)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](https://opensource.org/licenses/MIT)

[中文](README_zh.md) | English

**Rust-first control plane for connecting AI agents to live DCC sessions.**

dcc-mcp-core turns Maya, Blender, Houdini, Photoshop, Godot, RenderDoc, and
custom studio hosts into discoverable MCP and REST capabilities. It provides
the gateway, skills, structured results, main-thread dispatch, diagnostics,
IPC, workflows, and packaged CLI/server binaries needed to operate real
desktop sessions.

## Choose your entry point

| You want to… | Start with |
|---|---|
| Control a running DCC from an agent or CI job | [dcc-mcp-cli](docs/guide/cli-reference.md) |
| Expose a DCC adapter over MCP/REST | [create_skill_server](docs/guide/getting-started.md) |
| Add tools without Python registration code | [SKILL.md + tools.yaml](docs/guide/skills.md) |
| Build a new DCC adapter | [new-adapter-onboarding.md](docs/guide/new-adapter-onboarding.md) |
| Understand routing and multi-instance behavior | [gateway.md](docs/guide/gateway.md) |
| Integrate from any HTTP client | [rest-api-surface.md](docs/guide/rest-api-surface.md) |

## The current contract

The default agent path is CLI + REST:

~~~text
dcc-mcp-cli dcc-types (when adapter support is unclear)
  -> list
  -> search
  -> describe
  -> load-skill (only when needed)
  -> call
~~~

The gateway keeps tools/list bounded. It advertises the canonical
discovery/dispatch wrappers (search, describe, load_skill, and call) instead of
fanning every backend tool into one large list. Backend capabilities are
discovered by search, inspected by describe, and invoked through the wrapper or
the REST twin (POST /v1/search, /v1/describe, /v1/call).

When a concrete DCC session is needed, read gateway://instances; each entry
already includes its mcp_url. Do not use the removed legacy instance tools
(list_dcc_instances, get_dcc_instance, or connect_to_dcc). Wrapper inputs
belong inside arguments; do not put backend fields beside tool_slug.

For direct per-DCC MCP connections, the compatibility discovery names
(search_tools, describe_tool, and call_tool) remain available where the server
exposes them. New gateway integrations should use the canonical names above.

## Quick start: operate a DCC

`dcc-mcp-cli` is the preferred control path for every shell-capable agent.
If it is missing, obtain the user's consent, install the public `dcc-mcp`
Skill below, and run its bundled verified helper from the Skill directory:

~~~bash
python scripts/check_cli.py --ensure-cli --pretty
~~~

Without the Skill, download the official installer to a local file, inspect it,
and only then execute that file:

~~~bash
# Linux/macOS
curl -fL https://raw.githubusercontent.com/dcc-mcp/dcc-mcp-core/main/scripts/install-cli.sh -o install-cli.sh
cat install-cli.sh
# After reviewing the file:
sh ./install-cli.sh
~~~

~~~powershell
# Windows PowerShell
Invoke-WebRequest https://raw.githubusercontent.com/dcc-mcp/dcc-mcp-core/main/scripts/install-cli.ps1 -OutFile .\install-cli.ps1
Get-Content -Raw .\install-cli.ps1
# After reviewing the file and subject to the current execution policy:
& .\install-cli.ps1
~~~

Both paths accept only the official `dcc-mcp/dcc-mcp-core` release, validate
the platform update manifest and CLI SHA-256, and leave any existing binary
untouched if validation or download fails. This is an integrity check, not a
digital signature. Never pipe a remote installer directly into a shell or
bypass the machine's script execution policy.

### Install the agent Skill suite

Install the three public Skills directly from ClawHub; cloning this repository
is not required:

~~~bash
openclaw skills install @loonghao/dcc-mcp
openclaw skills install @loonghao/dcc-mcp-skills-creator
openclaw skills install @loonghao/dcc-mcp-creator
~~~

Add `--global` to each command when every local OpenClaw agent should see the
suite. Other ClawHub-compatible workspaces can use the registry CLI directly:

~~~bash
npx --yes clawhub@0.23.1 install @loonghao/dcc-mcp
npx --yes clawhub@0.23.1 install @loonghao/dcc-mcp-skills-creator
npx --yes clawhub@0.23.1 install @loonghao/dcc-mcp-creator
~~~

| Skill | Agent role |
|---|---|
| [`dcc-mcp`](skills/dcc-mcp/) | Default live DCC control and marketplace discovery; Skill-store requests begin with `dcc-mcp-cli marketplace search` |
| [`dcc-mcp-skills-creator`](skills/dcc-mcp-skills-creator/) | Create, validate, package, and review DCC-MCP Skill packages |
| [`dcc-mcp-creator`](skills/dcc-mcp-creator/) | Create or modernize a complete DCC adapter and its runtime wiring |

All three packages carry Codex `agents/openai.yaml` metadata while preserving
their DCC-MCP and ClawHub contracts. Their immutable ClawHub releases are
versioned independently in
[`.github/clawhub-skills.json`](.github/clawhub-skills.json), so Skill updates
do not collide with an already-published core version. The sync workflow
publishes the manifest versions and verifies their packaged files. Bump both
the manifest entry and the matching `SKILL.md` metadata version for every new
immutable Skill release.

Keep an official build current through the release manifest:

~~~bash
dcc-mcp-cli update check
dcc-mcp-cli update apply
~~~

`update apply` downloads and stages the latest CLI for the next launch. It does
not update a running `dcc-mcp-server`; update that server in its own environment.

Then discover a live capability before calling it:

~~~bash
dcc-mcp-cli dcc-types
dcc-mcp-cli list
dcc-mcp-cli search --query "create sphere" --dcc-type maya --limit 20
dcc-mcp-cli describe <tool-slug>
dcc-mcp-cli call <tool-slug> --json '{"radius": 2.0}' \
  --meta-json '{"agent_context":{"session_id":"task-42"}}'
~~~

`--query "create sphere"` is compatible with released CLI builds. Builds that
contain the unified search parser also accept unquoted positional words.

`dcc-types` is an offline, catalog-backed capability query. It reports
canonical adapter identifiers and install-plan availability; `list` remains
the source of truth for live instances.

Replace the placeholder with the slug returned by search. For remote workstations,
register a gateway profile and select it:

~~~bash
dcc-mcp-cli gateway register https://workstation.example:19293 --name pcA
dcc-mcp-cli gateway set pcA
dcc-mcp-cli list
~~~

Use dcc-mcp-cli doctor when startup or readiness is unclear. Open the local
Admin UI at [http://127.0.0.1:9765/admin](http://127.0.0.1:9765/admin) after the
gateway is available.

After a task is accepted, agents can inspect bounded gateway evidence with
`dcc-mcp-cli stats --range 24h --session-id task-42`. A zero call count means
no telemetry evidence, and direct local calls may not be represented. Feed the
result plus short task and validation summaries to the
[`review_skill_improvement` prompt](skills/dcc-mcp-skills-creator/prompts.yaml).
The prompt defaults to no change, prefers improving an existing skill, and
never grants authority to edit or publish outside the task scope.

## Quick start: expose skills from Python

~~~bash
pip install dcc-mcp-core
~~~

Point the server at a skill directory and start the MCP endpoint:

~~~python
import os

from dcc_mcp_core import McpHttpConfig, create_skill_server

os.environ["DCC_MCP_MAYA_SKILL_PATHS"] = "/path/to/skills"

server = create_skill_server("maya", McpHttpConfig())
handle = server.start()
print(handle.mcp_url())
~~~

Local DCC instances bind an OS-assigned port by default. The resolved URL is
registered automatically, so the gateway and CLI discover it without a
hardcoded per-instance port. Pass `port=<number>` only for an explicit fixed
listener requirement.

For manual handler registration, use McpHttpServer and ToolRegistry. For
adapter lifecycle, readiness, gateway registration, and hot reload, use
DccServerBase as described in the adapter guides.

## Add a skill without framework glue

The project follows the agentskills.io frontmatter contract. Put dcc-mcp-core
extensions under metadata.dcc-mcp and keep tool declarations in a sibling file:

~~~text
my-skill/
├── SKILL.md
├── tools.yaml
└── scripts/
    └── do_thing.py
~~~

~~~yaml
# SKILL.md
---
name: my-skill
description: "Does a useful Maya task. Use when the user asks for it."
metadata:
  dcc-mcp:
    dcc: maya
    tools: tools.yaml
    search-hint: "geometry, scene task"
---
~~~

~~~yaml
# tools.yaml
tools:
  - name: do_thing
    description: Do the task in the active Maya scene.
    source_file: scripts/do_thing.py
    execution: sync
    affinity: main
    annotations:
      read_only_hint: false
      destructive_hint: true
~~~

Run the same production validator used by CI with
dcc-mcp-cli lint path/to/skills. See [Skills](docs/guide/skills.md) for schemas,
groups, dependencies, testing, and migration rules.

## DCC UI Control

Desktop application automation for cases where native DCC APIs cannot observe
or drive the interface state directly. Agents use `ui_control__snapshot`,
`ui_control__find`, `ui_control__act`, `ui_control__wait_for`,
`ui_control__record_clip`, and `ui_control__stop_computer_use` to observe,
find, act on, and record exact target windows.

Use UI Control as a bounded fallback, not as the default DCC integration:

- Prefer structured adapter tools for scene, asset, render, and project work.
- Use UI Control for controls that have no API, incomplete adapter coverage,
  and end-to-end GUI acceptance tests.
- Do not treat every image in an agent task as UI Control evidence. DCC-native
  captures, RenderDoc exports, ImageGen output, and video frames have different
  provenance.

![Controlled window with corner brackets and capsule overlay](docs/assets/ui-control/corner-brackets-capsule.png)

### How screenshots and clicks work

`ui_control__snapshot` captures only the bound window. It preserves native
resolution until either the longest edge exceeds 1600 pixels or the image
exceeds 1.5 million pixels, then downsizes while preserving aspect ratio. A
1280×720 window stays 1280×720; a 1920×1080 window becomes 1600×900. Smaller
text and custom-drawn icons are therefore easier to recognize when the target
window is large and unobstructed.

The agent receives three complementary signals:

1. PNG pixels for visual recognition and spatial understanding.
2. A Windows UI Automation (UIA) tree with labels, roles, and stable control
   identifiers when the application exposes them.
3. Observation metadata for the exact PID, HWND, DPI, source rectangle,
   desktop generation, session, and snapshot.

Clicks prefer a semantic `control_id`. For custom-drawn UI, screenshot-relative
coordinates are mapped back through the source rectangle and DPI. Every action
must cite the latest `snapshot_id`; the host revalidates the window, desktop,
geometry, and generation before sending input, and rejects stale observations.

Successful snapshots and recordings include `capture_provenance`, including
the backend, session, target PID/HWND, output and source dimensions, scaling,
and native capture backend. Preserve that block with screenshots used as
evidence. Older images without provenance cannot be reliably attributed to UI
Control after the fact.

### Capabilities

- **Scoped window targeting** — snapshots and actions are bound to a single
  process or window handle, never the whole desktop.
- **Multi-instance sessions** — the gateway selects the DCC `instance_id`,
  while the native host namespaces each adapter connection. Separate DCC
  instances may therefore reuse a logical `session_id` such as `default`
  without sharing capabilities or cleanup state.
- **Semantic UIA + raw input fallback** — prefer stable semantic controls
  (button, text field, checkbox) resolved by `ui_control__find`, then fall
  back to screenshot-relative coordinates when custom-drawn controls have no
  semantic node.
- **Bounded security model** — every action is scoped by the
  adapter/operator-bound PID/HWND. Raw input requires an explicit opt-in
  (`DCC_MCP_COMPUTER_USE_ALLOW_RAW_INPUT=true`). Hard-denied: passwords,
  authentication controls, LockApp, Windows Security, terminals, and
  credential manager windows.
- **Visible capsule overlay** — while a native DCC UI Control session is active,
  click-through corner brackets mark the target window and a bottom-center
  capsule reads `DCC UI Control · <app> | Esc to stop`.
  The user stops control at any time with `Esc`.
- **One shared input safety owner** — multiple exact-window sessions may remain
  active in one Windows logon session. Native input is still serialized through
  one process coordinator and one cross-process owner; `Esc` latches every
  session, while an ordinary stop only releases the selected session.
- **Exact-window recording** — records a bounded, constant-frame-rate JPEG
  sequence from the operator-bound PID/HWND. The host owns the directory,
  hashes every frame, commits the manifest last, and deletes partial captures.
- **Audit trail** — every snapshot, recording, action, wait, stop, and rejected operation
  appends a redacted `ui_control_operation` event to the shared log directory,
  visible in the Admin Logs panel without exposing entered text or screenshot
  coordinates.

### Tool reference

| Tool | Description |
|------|-------------|
| `ui_control__snapshot` | Capture a bounded PNG plus UIA tree from the scoped window |
| `ui_control__find` | Locate semantic controls by query, role, label, or object name |
| `ui_control__act` | Perform one scoped semantic or coordinate-based action |
| `ui_control__record_clip` | Record a host-owned, hash-verified JPEG sequence from the exact window |
| `ui_control__wait_for` | Poll until a UI condition becomes true or times out |
| `ui_control__stop_computer_use` | Release the capsule, hotkey, and global input owner |
| `ui_control__system_operation` | Ensure a named Windows configuration item (operator-granted) |

For detailed skill reference and agent workflows, see the
[ui-control skill](python/dcc_mcp_core/skills/ui-control/SKILL.md).

![Admin Logs panel with redacted ui_control_operation events](docs/assets/ui-control/admin-logs-audit.png)

## Architecture

![dcc-mcp-core architecture](docs/assets/architecture/current-stack.svg)

The runtime has four useful layers:

1. **DCC service** — owns skills and executes tools inside one DCC process.
2. **Sidecar/supervisor** — bridges host RPC, readiness, process lifetime, and
   gateway registration when an adapter uses the packaged runtime.
3. **Gateway daemon** — aggregates live instances and owns discovery, routing,
   REST, Admin UI, audit, and diagnostics.
4. **Client surfaces** — CLI, MCP clients, REST clients, and marketplace tools.

The Rust workspace and package membership are defined by the root
[Cargo.toml](Cargo.toml). The Python package is a PyO3 extension with pure
Python helpers and supports Python 3.7–3.14. Build-from-source requirements come
from [rust-toolchain.toml](rust-toolchain.toml) and package metadata; the
repository does not duplicate version or package counts in this README.

## Documentation map

- [Getting started](docs/guide/getting-started.md) — install the package and
  start a first server.
- [CLI reference](docs/guide/cli-reference.md) — complete operator commands and
  flags.
- [Gateway guide](docs/guide/gateway.md) — daemon, registry, routing, relay, and
  multi-instance behavior.
- [REST API surface](docs/guide/rest-api-surface.md) — request envelopes,
  tool_slug, readiness, and error contracts.
- [Skills guide](docs/guide/skills.md) — authoring, loading, validation, and
  persistence.
- [Adapter onboarding](docs/guide/new-adapter-onboarding.md) — the supported
  adapter implementation and release path.
- [Documentation index](docs/guide/INDEX.md) — the complete guide/API map.
- [AI agent guide](AI_AGENT_GUIDE.md) and [AGENTS.md](AGENTS.md) — agent workflow
  and repository rules.

## Development

Use the repository-pinned toolchain where possible:

~~~bash
vx just install
vx just dev
vx just test
vx just test-rust
vx just lint
vx just docs-check
~~~

docs-check builds the VitePress site and catches documentation links and syntax
errors. Markdown lint runs in the docs CI workflow. See
[CONTRIBUTING.md](CONTRIBUTING.md) for coding, testing, and release rules.

## License

MIT — see [LICENSE](LICENSE).
