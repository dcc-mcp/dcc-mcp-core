---
name: blender-dev
description: "Blender add-on development diagnostics, reloads, UI metadata, and environment checks"
license: "MIT"
allowed-tools: ["Bash", "Read"]
metadata:
  dcc-mcp:
    dcc: blender
    layer: domain
    stage: diagnostics
    version: "1.0.0"
    tags: [blender, development, diagnostics, addon, reload, ui, debug]
    search-hint: "addon diagnostics, reload modules, run check, development, debug server, UI snapshot, Python environment"
    search-aliases: [developer tools, debug addon, module reload, sys.path, diagnostic check, UI inspector, debugpy, addon state, dev environment]
    intent: "Development-only diagnostics for Blender add-on debugging — inspect state, reload modules, attach debugger, and query environment."
    recall-context:
      app_type: blender
      domain: diagnostics
      workflow_stage: diagnostics
      task_category: query
    preconditions:
      - type: software
        name: blender
        version: ">=4.0"
    side-effects:
      modifies: true
      imports: true
      targets: [addon_state, sys.path, module_cache]
    produces: [diagnostic_report, ui_snapshot, environment_info]
    requires: []
    tools: tools.yaml
---

# blender-dev

Development-only diagnostics for Blender add-ons and adapter debugging.

Prefer typed domain skills for scene authoring. Use this skill when you need to
inspect add-on state, attach a checkout to `sys.path`, reload development
modules, run a named diagnostic check, inspect structured UI metadata, or start
an optional `debugpy` listener. Code execution helpers can run arbitrary Python
inside Blender and should be used only for explicit development or test flows.

## Add-on lifecycle

`list_addons` / `get_addon_status` / `enable_addon` / `disable_addon` cover
add-ons Blender already knows about. These three cover the rest:

| Tool | Description |
|---|---|
| `install_addon` | Install a `.py` single-file add-on or a `.zip` bundle, then optionally enable it |
| `remove_addon` | Disable and uninstall an add-on (works in background mode) |
| `refresh_addons` | Rescan the add-on paths and report the count change |

Two things that otherwise surprise callers:

- **Installing does not refresh the module list.** `install_addon` runs
  `addon_refresh` itself, but if you drop a file into an add-on directory by
  other means, call `refresh_addons` before `list_addons` will show it.
- **Give `addon_module` for archives.** Blender derives a module name from the
  file name, which does not always match the package inside a zip, so the
  install can succeed while the module never appears. Passing `addon_module`
  lets the tool confirm it; if the name is wrong you get an error naming it
  instead of a silent success.
- **`remove_addon` does not use Blender's `addon_remove` operator.** That
  operator calls `context.area.tag_redraw()`, which is `None` under
  `blender --background`, so it raises there. Removal instead disables through
  the operator, deletes the module files directly, refreshes, and then confirms
  the add-on is gone — if it is still registered the call fails rather than
  reporting success. Single-file add-ons delete the `.py`; packages delete the
  whole directory.
