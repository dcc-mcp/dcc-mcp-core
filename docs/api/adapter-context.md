# Adapter Context And Policy Helpers

This API gives DCC adapters shared contracts for concise instructions,
post-tool snapshots, visual feedback, response shaping, toolset profiles, and
searchable API docs.

## Instruction Resources

```python
from dcc_mcp_core import AdapterInstructionSet, register_adapter_instruction_resources

register_adapter_instruction_resources(
    server,
    AdapterInstructionSet(
        dcc="maya",
        instructions="Use screenshots after visual scene changes.",
        capabilities={"screenshots": True, "in_process_execution": True},
        troubleshooting="If execution stalls, check for modal dialogs.",
        adapter_version="0.3.0",
    ),
)
```

This registers:

- `docs://adapter/<dcc>/instructions`
- `docs://adapter/<dcc>/capabilities`
- `docs://adapter/<dcc>/troubleshooting` when provided

`DccServerBase.register_adapter_instructions(...)` is a thin wrapper for
embedded adapters.

## Context Snapshots

Adapters can attach a small, bounded scene/document snapshot after mutating
tools:

```python
from dcc_mcp_core import DccContextSnapshot, append_context_snapshot

result = append_context_snapshot(
    {"success": True, "message": "Layer created"},
    DccContextSnapshot(
        dcc="photoshop",
        document={"name": "hero.psd"},
        active_layer={"name": "Glow"},
        counts={"layers": 12},
    ),
)
```

`DccServerBase.set_context_snapshot_provider(callable)` stores a provider, and
`DccServerBase.append_context_snapshot(result)` applies it.

## Response Shaping

`ResponseShapePolicy` truncates large lists, dicts, and strings and adds
`_meta["dcc.response_shape"]` with omitted counts and a `next_query` hint.

```python
from dcc_mcp_core import ResponseShapePolicy, shape_response

payload = shape_response(scene_graph, ResponseShapePolicy(max_items=200, max_bytes=256_000))
```

## Visual Feedback

`VisualFeedbackPolicy` standardizes resource-backed previews:

```python
from dcc_mcp_core import VisualFeedbackPolicy, build_visual_feedback_context

context = build_visual_feedback_context(
    resource="output://viewport.png",
    width=1280,
    height=720,
    policy=VisualFeedbackPolicy(mode="after_mutation", max_size=800),
)
```

## Toolset Profiles

`DccToolsetProfile` and `ToolsetProfileRegistry` provide a high-level layer over
skill groups for adapter modes such as `modeling-basic`, `rendering`, or
`photoshop-layer-editing`.

## API Docs

Adapters can expose bundled or generated host API docs without arbitrary runtime
introspection:

```python
from dcc_mcp_core import DccApiDocEntry, DccApiDocIndex, register_dcc_api_docs

index = DccApiDocIndex(
    "blender",
    [DccApiDocEntry("bpy.ops.mesh.primitive_cube_add", "Add a cube")],
    version="4.0",
)
register_dcc_api_docs(server, index)
```

This registers `docs://adapter/<dcc>/api/index` and one resource per symbol.
