---
name: unity-scene
description: >-
  Domain skill — Inspect and edit the active Unity scene with typed, undoable
  GameObject operations. Use for hierarchy, object creation, transforms, and
  scene saves. Not for asset imports — use unity-project.
license: MIT
compatibility: "Unity 2018.4.25f1+ (.NET 4.x); dcc-mcp-core 0.19.90+"
allowed-tools: "python"
metadata:
  dcc-mcp:
    dcc: unity
    layer: domain
    version: "0.13.0"  # x-release-please-version
    search-hint: "Unity scene hierarchy GameObject transform save Undo"
    tags: "unity,scene,gameobject,transform,game-development"
    tools: tools.yaml
    depends: "dcc-diagnostics"
---

# Unity Scene

Inspect the hierarchy immediately before using instance IDs. Treat every Unity instance ID as an
opaque, session-scoped value and return it unchanged; Unity 6000.5+ emits decimal strings while
older Editors retain integer output for compatibility. Both forms are accepted as input. Object
creation and transform edits register with Unity Undo.

Do not automatically retry a timed-out mutation. Inspect the project and scene first because the
Editor may have completed the original request near the timeout boundary.

## Component authoring

Call `list_components` with a fresh hierarchy ID. Its `object_id` and `component_id`
handles expire on domain reload or object destruction. Add by fully qualified type
name; inspect the component before setting exact serialized property paths. Supply
`expected_fingerprint` to reject concurrent edits. Values support booleans, integers,
floats, strings, enum indices, vectors, colors, rectangles, bounds, and compatible
object references (`object_id` or an exact `Assets/` path). Existing nested fields
and array elements use Unity property paths; array resizing, curves, managed
references, event wiring, and direct prefab asset edits are unsupported in this
iteration. Unsupported fields are marked non-editable.

Batches validate in a SerializedObject before applying and use the command Undo
transaction for failures. Project component lifecycle callbacks can have their own
external side effects; Undo cannot roll those back. Results include normalized
readback and scene/prefab override state. No operation saves content. Review dirty
scenes before the existing `save_scene`, which saves all open scenes.
