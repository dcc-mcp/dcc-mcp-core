---
name: houdini-automation
description: >-
  Pipeline skill - run file-based Houdini Python automation, manage hip files,
  set timeline ranges, and build small node chains from structured specs. Use
  when orchestrating repeatable Houdini tasks. Not for inspecting a scene only.
license: MIT
compatibility: "dcc-mcp-houdini 0.1+, Houdini 18.5+, dcc-mcp-core 0.20.14+"
allowed-tools: Bash Read Write Edit
metadata:
  dcc-mcp:
    dcc: houdini
    layer: pipeline
    stage: pipeline
    version: "1.3.0"
    tags: [houdini, automation, hip, timeline, pipeline, scripts]
    search-hint: "run python file, save hip, load hip, timeline, build node chain"
    search-aliases: [run script file, save hip, open hip, load scene, set frame range, timeline, build node chain, automate houdini]
    example-prompts:
      - "Run this Python file in Houdini"
      - "Save the hip file to /projects/showA/shot.hip"
      - "Set the frame range to 1-120 and build a box→transform chain"
    intent: "Run repeatable file-based Houdini automation: scripts, hip I/O, timeline, and node-chain recipes."
    recall-context:
      app_type: houdini
      domain: pipeline
      workflow-stage: pipeline
      task-category: mutate
    requires: []
    produces: [scene_state, node_network, "file:hip"]
    preconditions:
      - type: software
        name: houdini
        version: ">=18.5"
    side-effects:
      creates: true
      modifies: true
      file-output: true
      targets: [scene_state, node_network, "file:hip"]
    tools: tools.yaml
---

# houdini-automation

Higher-level repeatable automation for Houdini sessions. Prefer `run_python_file`
for reviewed scripts on disk, and `build_node_chain` for compact graph recipes.

`run_python_file` bounds inline stdout, stderr, and result payloads. Its default
`output_mode=full` keeps the legacy string result while applying the configured
character caps. Use `output_mode=structured` to preserve JSON-safe result types,
or `output_mode=summary` to return counts and artifact metadata without inline
bodies. When `spill_overflow_to_artifact=true`, truncated channels are written
as complete UTF-8 temporary artifacts with byte counts and SHA-256; summary mode
persists every non-empty channel. Treat those files as potentially sensitive
script output and delete them after collection.

`build_node_chain` is a structured atomic mutation surface, not an arbitrary
code executor. It validates the parent, every node type/name/reference, and all
connection references/port indices before opening an undo group. Use
`dry_run=true` to inspect `validated` and predicted `affected_paths` with zero
scene mutation.

On execution, the complete recipe uses one named Houdini undo group. Results
include `transaction_id`, `undo_label`, validation evidence, and post-cook
`readback`. If creation, parameter assignment, connection, layout, cook, or
readback fails, the tool explicitly removes created nodes and restores any
existing input connections and existing node positions touched by layout. Check
`rollback.complete` and `rollback.errors` before retrying a failed recipe.

Successful responses expose a compact `summary` with the created nodes,
readback-verified connections, submitted parameter values, and counts. This is
the preferred proof for generated MaterialX networks.

## MaterialX displacement acceptance

For each existing MaterialX builder, use one `build_node_chain` recipe with an
`mtlximage` feeding `mtlxrange`, then `mtlxdisplacement`. Put texture paths,
range bounds, clamping, and displacement scale in each node's `parameters` and
connect nodes by their recipe-local `ref` or `node_name`. Four materials need
four such calls because they have four different parent networks; each call is
independently prevalidated and leaves no partial network when rollback is
complete.

Use a separate atomic recipe under `/stage` for the
`rendergeometrysettings` node and its upstream/downstream LOP connections.
Resolve the current Houdini/Karma true-displacement and dicing parameter names
with the parameter inspection tools, then pass those names through the same
structured `parameters` object; no renderer-specific Python is required.

Do not save between batch calls. After every material and stage response has a
successful summary, call `save_hip_file`. It writes a sibling temporary HIP and
uses an atomic filesystem replacement, so a save or replace failure preserves
the prior target. In graphical Houdini, the tool verifies the dirty flag after
replacement. If Houdini still reports unsaved changes, it performs a documented
Save As to the target, returns `atomic_replace=false`, and verifies that the
dirty flag cleared. Headless Hython reports `unsaved_changes=null` because HOM
cannot observe that state there. If a save or verification fails after a valid
file was staged, use the returned `recovery_file`. In-memory recipes remain
separate transactions; reload the prior saved HIP when whole-batch rollback is
required.
