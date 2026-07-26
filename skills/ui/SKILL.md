---
name: ui
description: >-
  Task-oriented domain skill — inspect, find, and interact with DCC user
  interface elements. Composess qt-ui-inspector (widget tree inspection) and
  ui-control (snapshot + find + act computer-use fallback) into single-call
  UI workflows. Use this skill when you need to read UI state, find widgets,
  trigger actions, or automate UI interactions that structured tools don't
  cover. Not for structured DCC tool operations — use domain skills (asset,
  etc.) or direct MCP tools first; UI interaction is the fallback.
license: MIT
allowed-tools: ["Bash", "Read"]
metadata:
  dcc-mcp:
    dcc: python
    layer: domain
    version: "1.0.0"
    compatibility: "Python 3.7+, dcc-mcp-core 0.19+"
    tags: [ui, ui-control, qt, widget, inspect, interact, read-only]
    search-hint: >-
      inspect UI, find widget, click button, type text, read window, UI element,
      qt widget, window title, dialog, menu, toolbar, button, text field,
      select widget, interact with UI, control DCC window
    search-aliases: [inspect ui, find widget, click button, type text, read window, UI interaction, control window]
    intent: "Inspect, find, and interact with DCC user interface elements using qt-ui-inspector and ui-control."
    recall-context:
      app_type: python
      domain: ui
      workflow_stage: interaction
      task_category: control
    preconditions:
      - qt-ui-inspector skill available (for Qt-based DCCs)
      - ui-control skill available (for non-Qt or computer-use fallback)
    side-effects:
      creates: false
      modifies: true
      file_output: true
      targets: [screenshot_file]
    produces: [ui_state, widget_info, action_result]
    requires: [qt-ui-inspector, ui-control]
    tools: tools.yaml
    depends: [qt-ui-inspector, ui-control]
---

# UI — Task-Oriented UI Inspection & Interaction

> **Domain skill**: Use this when structured tools don't cover your UI needs.

UI composes `qt-ui-inspector` and `ui-control` tools into task-oriented
workflows. Use it as the fallback after structured DCC tools — not as the
first choice.

## When to use

| Scenario | Tool |
|----------|------|
| Find a widget by name/type/text | `find_widget` — search the widget tree |
| Read UI state (checkboxes, fields, lists) | `inspect_ui` — snapshot full UI state |
| Click a semantic UI Control node | `interact_widget` — scoped snapshot/action/verification/stop loop |
| Wait for a dialog or window | `wait_for_ui` — poll until element appears |
| Debug UI layout issues | `capture_ui_state` — full screenshot + widget dump |

## When NOT to use

- **Structured tool exists for the operation** — use that tool first
- **Verifying instance health** — use the `verify` skill
- **Diagnosing tool failures** — use the `debug` skill

## Usage

**Prerequisites**: `dcc-mcp`, `qt-ui-inspector`, and `ui-control` skills loaded.
A DCC instance must be running with its window visible. On Windows, the operator
must set `DCC_MCP_UI_CONTROL_UIA_PROCESS_ID` or `DCC_MCP_UI_CONTROL_UIA_WINDOW_HANDLE`.

**⚠️ Policy**: Structured tools first. Only load `ui` after `dcc-mcp` search
returns `unsupported` or `capability_missing`. Never start with UI automation.

### MCP-native agent (IDE)

```
search_skills("ui")
load_skill("ui")
call("ui__inspect_ui", {"window_title": "Maya", "include_screenshot": true})
call("ui_control__snapshot", {"session_id": "save-scene"})
call("ui_control__find", {"session_id": "save-scene", "query": "Save"})
call("ui__interact_widget", {"instance": "maya.abcd1234", "session_id": "save-scene", "widget": {"control_id": "saveButton"}, "action": "click"})
```

### Shell/CLI agent

```bash
dcc-mcp-cli search-skills --query ui
dcc-mcp-cli load-skill ui
dcc-mcp-cli call <instance>.ui__inspect_ui --json '{"window_title":"Maya"}'
dcc-mcp-cli call <instance>.ui__capture_ui_state --json '{"instance":"<instance>","session_id":"ui-debug","output_dir":"/tmp/ui-debug"}'
```

### Availability

Ships with `dcc-mcp-core` wheel. Windows-only for UIA-based operations.
Depends on `qt-ui-inspector` (Qt DCCs) and `ui-control` (all DCCs via
computer-use fallback). Both must be loadable before `ui` tools resolve.

### Hard stops (do NOT retry)

- `desktop_unavailable` — lock screen / RDP disconnect
- `user_interrupted` — user pressed Esc
- `elevation_required` — needs admin
- `approval_required` — operator grant missing

## UI interaction policy

1. **Structured tools first** — always try MCP tools before UI fallback
2. **Inspect before acting** — run `find_widget` or `inspect_ui` before clicking
3. **One action per step** — snapshot → act → snapshot verification loop
4. **Stop on interrupt** — never retry after `user_interrupted` or `desktop_unavailable`

## Tools

| Tool | Category | Description |
|------|----------|-------------|
| `inspect_ui` | Read | Full widget tree snapshot + window list |
| `find_widget` | Read | Search for widgets by type, text, or role |
| `interact_widget` | Write | Click, type, or select on a found widget |
| `wait_for_ui` | Read | Poll until a widget appears or disappears |
| `capture_ui_state` | Read | Screenshot + widget dump bundle for debugging |

## Progressive disclosure

- **Quick start**: [RECIPES.md](references/RECIPES.md) — copy-pasteable UI interaction flows
- **Exploring UI**: [INTROSPECTION.md](references/INTROSPECTION.md) — how to query widget trees
- **Troubleshooting**: [ERRORS.md](references/ERRORS.md) — common UI automation failures
