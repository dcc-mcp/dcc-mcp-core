---
name: unreal-automation
description: >-
  Domain skill - Unreal Engine native Automation Test and MCP health validation.
  Use to inspect typed plugin readiness, list UE Automation tests, queue native
  test runs, and run a safe self-check against the active MCP server.
license: MIT
compatibility: Unreal Engine 5.0+, Python 3.9+
allowed-tools: Bash Read Write
metadata:
  dcc-mcp:
    dcc: unreal
    version: "0.1.0"
    layer: domain
    stage: validation
    search-hint: "unreal automation tests native test framework mcp smoke ci validation"
    tags: "unreal, automation, tests, smoke, ci, validation"
    tools: tools.yaml
---

# Unreal Automation

Tools for validating the active MCP server and driving Unreal Engine's native
Automation Test framework from an MCP client.

## Plugin capability preflight

Call `preflight_plugins` before workflows that depend on optional engine
plugins. The tool accepts `static_groom_import`, `usd_import`,
`movie_render_queue`, or `speedtree_import` and returns `required_plugins`, the enabled required
subset, exact `missing_plugins`, `ready`, and one structured `next_action`.
It is read-only: it never edits the project descriptor or enables plugins.

Use `configure_plugins` with `mode: plan` for the active project's descriptor
SHA256 and installed/default/project-override/runtime-enabled states. An absent
project entry is inherited, not disabled. Runtime enablement is not proof that
every module loaded or that an importer will accept a particular file.

`mode: apply` requires the reviewed `expected_sha256` and independent core
elicitation. Never supply a model-generated confirmation or fallback values.
On core versions where elicitation is unsupported (including 0.20.23), apply
returns `consent_required` without writing. Successful consent is rechecked
against the full plan; writes retain a unique byte-exact backup, preserve other
project fields and replace the descriptor atomically. Hash checks are optimistic
against external writers; the sibling lock serializes this adapter's writers.
The tool never installs plugins, restarts the editor, or claims runtime readiness
from a descriptor edit. Coordinate any eventual restart with the host owner.

## Scripts

- `preflight_plugins`
- `mcp_self_check`
- `list_automation_tests`
- `queue_automation_tests`
