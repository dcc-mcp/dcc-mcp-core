# New Adapter Onboarding Template

Use this template when creating a new DCC-MCP adapter repository. It covers
the minimal wiring required to produce a release-train-compatible adapter.

## 1. Repository Structure

```
dcc-mcp-<dcc>/
├── src/
│   ├── __init__.py          # Public re-exports
│   └── server.py            # Composition root
├── skills/
│   └── <dcc>-scripting/
│       ├── SKILL.md
│       ├── tools.yaml
│       └── scripts/
├── tests/
│   ├── test_server.py
│   └── test_skills.py
├── pyproject.toml
├── README.md
├── release-please-config.json
└── .github/
    └── workflows/
        └── ci.yml
```

## 2. pyproject.toml Dependency Snippet

```toml
[build-system]
requires = ["setuptools>=64", "wheel"]
build-backend = "setuptools.build_meta"

[project]
name = "dcc-mcp-<dcc>"
version = "0.1.0"
description = "DCC-MCP adapter for <DCC>"
authors = [{name = "Your Name", email = "you@studio.com"}]
license = {text = "MIT"}
requires-python = ">=3.9"
dependencies = [
    # Pin core to the latest released minor, never to main.
    "dcc-mcp-core>=0.18.0,<1.0.0",
]

[project.optional-dependencies]
test = [
    "dcc-mcp-core[test]",
    "pytest>=8.3.0",
    "ruff>=0.8.0",
]

[tool.ruff]
target-version = "py39"
line-length = 100
```

## 3. adapter_version & Readiness Boilerplate

Reference implementation (based on the Maya / 3ds Max adapter pattern):

```python
# src/<dcc>/server.py
from pathlib import Path

from dcc_mcp_core import (
    DccServerBase,
    DccServerOptions,
    HostExecutionBridge,
    AdapterReadinessBinder,
    register_all_builtin_skills,
)
from dcc_mcp_core.install_lifecycle import build_sidecar_command

ADAPTER_VERSION = "0.1.0"


class MyDccServer(DccServerBase):
    def __init__(self, port: int = 8765, dispatcher=None, **kwargs):
        bridge = HostExecutionBridge(dispatcher=dispatcher) if dispatcher else None
        options = DccServerOptions.from_env(
            "<dcc>",
            skills_dir=Path(__file__).parent.parent.parent / "skills",
            port=port,
            execution_bridge=bridge,
            adapter_version=ADAPTER_VERSION,
            **kwargs,
        )
        super().__init__(options=options)

    def _version_string(self) -> str:
        return ADAPTER_VERSION


def start_server(port: int = 8765):
    """Adapter entry point — call from the DCC startup script."""
    server = MyDccServer(port=port)
    AdapterReadinessBinder.bind_inline(server)
    register_all_builtin_skills(server, dcc_name="<dcc>", skills=server.skills)
    server.start()
    return server


def stop_server(server):
    """Adapter shutdown."""
    server.stop()
```

### Readiness Binding

| Adapter shape | Use |
|---|---|
| Interactive GUI (Maya, Houdini, 3ds Max, Nuke) | `AdapterReadinessBinder.bind_inline(server)` |
| Headless / batch (mayapy, hython, blender -b) | `AdapterReadinessBinder.bind_headless(server)` |
| Queue dispatcher with pump | `AdapterReadinessBinder.bind_queue_dispatcher(server, dispatcher, require_first_pump=True)` |

See [adapter-runtime-contracts.md](adapter-runtime-contracts.md) for the full
contract.

## 4. Multica Project Binding

When the adapter repository is managed through Multica, bind these resources:

```json
{
  "resources": [
    {
      "type": "github_repo",
      "uri": "https://github.com/<org>/dcc-mcp-<dcc>.git",
      "label": "dcc-mcp-<dcc>"
    },
    {
      "type": "local_directory",
      "label": "dcc-mcp-<dcc>",
      "path": "/home/user/projects/dcc-mcp-<dcc>"
    }
  ]
}
```

## 5. CI/CD Snippet

Minimum CI workflow (`.github/workflows/ci.yml`):

```yaml
name: CI
on:
  pull_request:
  push:
    branches: [main]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-python@v5
        with:
          python-version: "3.11"
      - run: pip install -e ".[test]"
      - run: ruff check src tests
      - run: ruff format --check src tests
      - run: pytest
```

## 6. First Skill Package

Create a minimal `skills/<dcc>-scripting/SKILL.md` ping tool to prove the
skill pipeline works end-to-end:

```yaml
---
name: <dcc>-scripting
description: Core scripting and query tools for <DCC>
metadata:
  dcc-mcp:
    dcc: <dcc>
    layer: infrastructure
    version: "0.1.0"
---
```

Plus a sibling `tools.yaml` with one tool definition and a `scripts/` directory
with the implementation. Refer to the `skills/dcc-mcp-skills-creator/SKILL.md`
skill for detailed authoring guidance.

## 7. VRS Smoke Trace

Add one VRS trace under `tests/vrs/traces/<dcc>-smoke.jsonl` to verify the
gateway can discover the new adapter. Copy the pattern from
`tests/vrs/traces/` and adjust `dcc_type` / expected tool slugs.

## 8. Post-Onboarding

- [ ] Submit a PR to core adding the adapter row to the
      [Adapter Compatibility Matrix](adapter-compatibility-matrix.md).
- [ ] Verify the gateway smoke runs (see [adapter-release-checklist.md](adapter-release-checklist.md#2-gateway-smoke-steps)).
- [ ] File a core issue if any adapter-local code should be escalated
      (see `skills/dcc-mcp-creator/references/CORE_ESCALATION_CHECKLIST.md`).
