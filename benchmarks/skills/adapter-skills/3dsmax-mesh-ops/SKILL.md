---
name: 3dsmax-mesh-ops
description: >-
  Domain skill - inspect and mutate 3ds Max mesh topology, cleanup, smoothing
  groups, modifier stacks, proxy meshes, explicit normals, and native boolean
  solids through atomic host-native operations.
license: MIT
compatibility: "dcc-mcp-core 0.17+, 3ds Max 2024+"
metadata:
  dcc-mcp:
    dcc: 3dsmax
    version: "1.1.0"
    layer: domain
    stage: authoring
    search-hint: "3ds Max mesh cleanup topology normals smoothing groups modifiers triangulate attach detach proxy subdivision add remove modifier stack collapse make unique modifier properties viewport render enable boolean union intersection subtraction cut operand extract editable poly component vertex edge face create mesh inspect mesh edit vertices mesh edit pick component weld align"
    tags: "3dsmax, mesh, topology, cleanup, normals, smoothing, modifiers, modifier_stack, boolean, editable_poly, components"
    tools: tools.yaml
    intent: "Inspect and mutate 3ds Max mesh topology, cleanup, smoothing groups, modifiers, and normals."
    search_aliases: ["mesh_operations", "mesh-ops", "boolean"]
    recall_context:
      app_type: "3dsmax"
      domain: "mesh_operations"
      workflow_stage: "authoring"
      task_category: "mutate"
    preconditions:
      - type: software
        name: "3ds Max"
        version: ">=2024"
    side_effects:
      creates: true
      modifies: true
      deletes: true
      exports: false
      imports: false
      file_output: false
      render: false
      targets: ["mesh", "scene_node", "modifier", "smoothing_group", "mesh_component"]
    produces: ["mesh_topology", "smoothing_group", "modifier_stack", "modifier_parameters", "proxy_mesh", "boolean_state", "component_indices", "ray_hit"]
---

# 3ds Max Mesh Operations Skill

Inspect mesh topology and apply focused mesh cleanup, subdivision, proxy, attach,
detach, smoothing group, and normal operations through `pymxs`. Also provides
general modifier stack CRUD: add, remove, enable/disable (viewport and render
granularity), set properties, collapse, and make-unique.

Five tools add Editable Poly **component-level** modelling: `create_mesh`,
`inspect_mesh`, `edit_vertices`, `mesh_edit`, and `pick_component`.

Mutating tools require explicit node names, stable object handles, or an
explicit `use_selection=true` argument. They return changed-node summaries so
agents can report what changed without relying on opaque macros.

## Boolean solids

`boolean_operation` drives the native ProBoolean or Boolean / Boolean2 compound
object. `create` registers `base_node` as the first operand followed by
`operands`, and sets the mode (`union`, `intersection`, `subtraction`, `cut`).
Because the operands stay live, `set_operand`, `extract_operand`,
`remove_operand`, and `add_operands` re-adjust an existing boolean without
rebuilding it, and `set_operation` switches the mode in place.

The two classes are reached through **class-specific adapters**, because they do
not agree on anything:

| | ProBoolean | Boolean / Boolean2 |
| --- | --- | --- |
| Reached through | the `ProBoolean` interface struct | methods on the object |
| Operand add | `SetOperandB` | `setOperandB` |
| Mode set / get | `SetBoolOp` / `GetBoolOp` | `setBoolOp` / `getBoolOp` |
| union / intersection / subtraction | 0 / 1 / 2 | 1 / 2 / 3 |
| cut | **unsupported** (3 is Merge there) | 5 |

Sharing one code path would silently produce the wrong solid, so `cut` is
rejected when only ProBoolean is available rather than being mapped onto Merge.

Both the mode and the registered operand count are read back. A mode the host
coerced - or that it accepts but will not report - an operand that did not
register, and an operand count the host does not expose all fail the call.
Every operand mutation is verified against the count read **before** the call:
an add must grow it by one, a removal must shrink it by one, and an extraction
must leave it unchanged. A failed `create` removes the node it made and reports
whether that removal was actually confirmed - including when the failure is an
operand that could not be resolved - so a rollback is never claimed unless the
deletion is verified.

`set_operand` is the one exception to full verification. It always confirms the
operand count, but whether the requested slot really holds the replacement
depends on an operand getter the host may not expose. When that getter is
missing the result carries `operand_identity_verified: false` plus a warning
instead of failing, because the common case - a host that applied the change -
would otherwise be unusable. Read that flag as "re-read before trusting the
slot", not as a confirmed replacement.

## Editable Poly component editing

Five tools cover component-level modelling on an Editable Poly. They share one
index space - the 1-based `polyOp` indices `inspect_mesh` hands out - and one
coordinate convention: agents work in world coordinates, and the object-space
mapping goes through the node transform.

| Tool | Role |
| --- | --- |
| `create_mesh` | Build an Editable Poly from explicit world vertices and ordered faces. |
| `inspect_mesh` | Read vertices / edges / faces with their actionable indices. |
| `edit_vertices` | `read`, `move`, `set`, or `align` vertices in world space. |
| `mesh_edit` | Apply a batch of component edits as one undoable step. |
| `pick_component` | Map an image position or a world ray onto a face, vertex, or edge. |

### Construction is verified, and a failed build leaves nothing behind

`create_mesh` assigns a TriMesh, converts the node to an Editable Poly, and then
reads **every** vertex back in world space and compares it against what was
asked for. A host that merged, rounded, or dropped a vertex fails the call
instead of returning a mesh that only looks right. Construction can fail after
the node already exists, so a failure deletes the node and reports whether that
deletion was confirmed through the handle lookup.

Faces with more than three vertices are fan-triangulated, because the TriMesh
constructor only accepts triangles. Which faces were split is reported in
`triangulated_faces` - a silent conversion would change the topology the agent
asked for.

### `mesh_edit` is one call, one undo step

The accepted ops run inside `dcc_mcp_3dsmax._undo_utils.undo_step()`, so one
call leaves one host undo entry and a failure part-way through cancels the hold
so the host rolls the whole batch back.

Grouping is a capability, not a given, and the tool never bluffs:

* The host cannot open a hold -> the batch is **refused** unless
  `allow_ungrouped` is true, and even then the result carries a warning saying
  the batch was not grouped.
* A failure part-way through an **ungrouped** batch reports
  `rolled_back: false`, `rollback: "unavailable"`, and the ops that did land.
  There is no automatic rollback to claim there, so the result does not claim
  one - a clean `rolled_back` over a partially edited mesh is exactly the
  misleading success this adapter refuses to ship.

Every op is range-checked against the live component counts during preflight, so
a bad index fails the whole batch before the first write rather than leaving a
partial edit behind.

### `pick_component` and what it will not guess

A pick is two stages: a world ray, then a ray cast.

The ray cast is fully verified. `intersectRayEx` names the face it hit, and the
hit point is then checked against that face's plane and its polygon outline, so
a host that names a face the point is not on is reported as
`face_verified: false` plus a warning rather than trusted. When only
`intersectRay` is available the host reports a point and no face, so the face is
resolved by proximity and flagged `face_reported: false` - an inferred index is
never presented as one the host named. A clean miss is a successful result with
`hit: false`; a miss is information, not an error.

The image-position-to-ray mapping is a host capability. When the host exposes no
entry point, the call **fails and names every candidate it probed** rather than
fabricating a projection, because a plausible-but-wrong ray produces a
plausible-but-wrong component. Supplying `ray_origin` and `ray_direction` needs
no host mapping and always works, so the tool is usable and fully testable on any
host.

## No silent success

Write paths never report success for a value the host did not accept. Every
property write is read back and compared, every add/remove is verified against
the stack length, and a host that exposes no usable entry point returns an
error. When a host runs an operation that cannot be confirmed programmatically
(`make_modifier_unique`), the result carries an explicit `warning` instead of a
bare success.

For the component tools that rule is per component, not per call: one vertex the
host refused fails the whole batch and names the vertex, rather than being
swallowed so the rest can report success.
