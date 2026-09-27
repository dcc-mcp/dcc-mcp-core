---
name: houdini-dev
description: >-
  Pipeline skill — development workflow helpers for authoring Houdini Python
  tools in a live session: attach a local project to sys.path, hot-reload
  modules by prefix or name, run module entrypoints or scripts with captured
  stdout/stderr/traceback, start a debugpy listener, introspect HOM
  objects/signatures/node-type categories, and inspect or trigger safe UI
  actions (headless-safe). Not for scene editing — use domain skills first.
license: MIT
compatibility: "dcc-mcp-houdini 0.1+, Houdini 20.5+, dcc-mcp-core 0.20.14+"
allowed-tools: Bash Read
metadata:
  dcc-mcp:
    dcc: houdini
    layer: domain
    stage: pipeline
    version: "1.0.0"
    tags: [houdini, development, debug, hot-reload, debugpy, introspection, hom, ui, diagnostics]
    search-hint: "develop houdini tools attach python project hot reload modules run entrypoint script debugpy attach introspect hom node type categories signatures ui snapshot headless diagnostics"
    tools: tools.yaml
---

# houdini-dev

Development workflow helpers for agents iterating on Houdini Python tools.
Stays **outside** the minimal startup surface (load explicitly with
`load_skill("houdini-dev")`). The intended loop:

1. `attach_project("/path/to/tool")` once for the project root.
2. Edit files in the normal workspace.
3. `reload_modules(prefix="mytool")` → `run_entrypoint("mytool.cli:main")`.
4. Inspect the concise stdout/stderr/traceback summary.

Set `DCC_MCP_HOUDINI_DEV_ROOTS` to a path-list of trusted roots to restrict
which local projects `attach_project` / `run_script` will accept.

## Tool groups

- **`dev-project`:** `attach_project`, `reload_modules`.
- **`dev-run`:** `run_entrypoint`, `run_script` (captured output).
- **`dev-debug`:** `start_debugpy` (safe `127.0.0.1:5678` default,
  already-running detection).
- **`dev-introspect`** (read-only): `introspect_hom` — members/signatures,
  node-type categories, and available node types.
- **`dev-ui`:** `ui_snapshot` (read-only), `ui_action`.

## Context limitations

- **Headless hython:** `ui_snapshot` / `ui_action` return a clean
  `supported=false` result instead of failing when no UI is available.
- **Trusted roots:** when `DCC_MCP_HOUDINI_DEV_ROOTS` is set, project and
  script paths outside those roots are refused with a structured error.
- **debugpy:** requires `debugpy` installed in the Houdini Python env; a
  second call reports `already_running=true` rather than re-binding the port.
- **HOM only with hou:** `introspect_hom` and the UI tools require `hou`;
  they degrade through `skill_error` when it cannot be imported.

## Tracer-bullet flow

1. `attach_project("/projects/mytool")`
2. `introspect_hom(category="Sop", name_filter="attrib")` → discover node types
3. `reload_modules(prefix="mytool", reimport=true)`
4. `run_entrypoint("mytool.build:run", kwargs={"dry_run": true})`
5. `ui_snapshot()` → confirm the active desktop when running interactively
