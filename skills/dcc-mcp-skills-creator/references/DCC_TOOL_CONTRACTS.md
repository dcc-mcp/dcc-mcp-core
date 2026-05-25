# DCC-MCP Tool Contracts

Use this checklist for every `tools.yaml` entry.

## Required Shape

- `name`: local snake_case tool name, never dotted.
- `description`: concise action description shown to agents.
- `source_file`: script path relative to the skill directory.
- `input_schema`: JSON Schema for parameters.
- `output_schema`: JSON Schema for returned data when practical.
- `execution`: `sync` for quick calls, `async` for long-running work.
- `affinity`: `main` for host API calls, `any` for pure work.
- `timeout_hint_secs`: realistic upper bound for dispatch and UX.
- `annotations`: MCP safety hints.

## Recovery Chains

Domain tools should include `next-tools.on-failure` entries that point to
diagnostic or observation tools, such as screenshots, audit logs, or scene
snapshots. Infrastructure tools can omit failure chains when they are already
the recovery target.

## Core Boundary

Do not parse `SKILL.md`, `tools.yaml`, `groups.yaml`, prompts, or workflows from
adapter runtime code when core exposes a catalog or typed skill object API. If a
needed transform or hook is missing, create a core RFC and keep the adapter
shim narrow until the core API exists.
