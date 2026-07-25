# UI — Introspection

How to explore and query DCC user interfaces.

## UI tool stack

```
Task-oriented (this skill):
  ui__inspect_ui          → full snapshot
  ui__find_widget         → search widget tree
  ui__interact_widget     → click/type/select
  ui__wait_for_ui         → poll for element
  ui__capture_ui_state    → evidence bundle

Infrastructure (composed):
  qt_ui_inspector__list_windows      → enumerate windows (Qt)
  qt_ui_inspector__snapshot_tree     → widget hierarchy (Qt)
  qt_ui_inspector__describe_widget   → widget properties (Qt)
  qt_ui_inspector__find_widgets      → search widgets (Qt)
  qt_ui_inspector__wait_for_widget   → poll for widget (Qt)

  ui_control__snapshot        → screenshot capture
  ui_control__find            → visual element search
  ui_control__act             → coordinate/semantic action
  ui_control__stop_computer_use → end session
  ui_control__wait_for        → visual polling
```

## Widget tree anatomy (Qt)

```
QMainWindow "Maya 2026"
├── QMenuBar
│   ├── QMenu "File"
│   │   ├── QAction "New Scene"
│   │   ├── QAction "Open Scene..."
│   │   └── QAction "Save Scene"
│   ├── QMenu "Edit"
│   └── QMenu "Help"
├── QToolBar "Main Toolbar"
│   ├── QToolButton "Select"
│   ├── QToolButton "Move"
│   └── QToolButton "Rotate"
├── QWidget "Central"
│   └── QStackedWidget "Viewport"
└── QStatusBar
```

Key widget properties:
- `objectName` — Qt object name (often the control_id)
- `text` / `windowTitle` — displayed text
- `className` — Python/C++ class (QPushButton, QLineEdit, etc.)
- `accessibleName` — accessibility label
- `visible` / `enabled` — interaction eligibility

## Querying widget state

```python
# Via dcc_introspect__eval (if Qt inspector not loaded)
# Maya:
import maya.cmds as cmds
# Qt widgets in Maya
from maya import OpenMayaUI as omui
from PySide2 import QtWidgets
ptr = omui.MQtUtil.mainWindow()
main_window = shiboken2.wrapInstance(int(ptr), QtWidgets.QMainWindow)
# Find children
for btn in main_window.findChildren(QtWidgets.QPushButton):
    if btn.isVisible():
        print(btn.objectName(), btn.text())
```

## Non-Qt DCCs

For DCCs without Qt UIs (e.g., native Windows, Cocoa, custom toolkits):

1. Use `ui_control__snapshot` for visual capture
2. Use `ui_control__find` with visual descriptions
3. Use `ui_control__act` with coordinates when semantic targeting unavailable
4. Always take a snapshot before and after each action

## State that matters for UI interaction

| What to check | Why |
|--------------|-----|
| `visible` | Hidden widgets can't be clicked |
| `enabled` | Disabled widgets reject input |
| `geometry` | Know the clickable region |
| `windowState` | Minimized/maximized/fullscreen |
| `focusWidget` | Where keyboard input goes |
