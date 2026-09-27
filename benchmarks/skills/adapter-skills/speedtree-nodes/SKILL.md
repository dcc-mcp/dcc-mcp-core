---
name: speedtree-nodes
description: Inspect and edit SpeedTree SPM and STT generator graphs, links, properties and spline components using new native files; requires Modeler regeneration for geometry validation.
license: MIT
metadata:
  dcc-mcp:
    dcc: speedtree
    version: "0.1.0"
    tools: tools.yaml
    tags: "speedtree,generators,nodes,branches,leaves,grass,willow,templates,properties"
---

# SpeedTree generator files

Use `discover_templates` with the installed content directory. Inspect the
source graph and generator properties before editing. Copy returned IDs,
template paths and component paths exactly; do not guess property names.
Use `inspect_authoring_catalog` to enumerate installed generator types and
stored property names, including unreadable templates and coverage limits.

`edit_graph` applies a bounded operation list to a copy and exclusively creates
a new output file. The output must retain the input suffix. Each operation has
an `op` selector: add, duplicate, remove, connect, disconnect, rename,
set_hidden, or set_property. For add, specify template_path and optionally
template_node_id, node_id, name and parent_id. Duplicate uses node_id and
optionally new_id, name and parent_id; descendants are not duplicated. Remove
uses node_id and an explicit cascade boolean when descendants should be removed.
Connect/disconnect use source_id and target_id. Set_property uses node_id,
name, value, and an optional component (default Value). Set_hidden uses node_id
and hidden. Rename uses node_id and name.

No operation edits a live Modeler document. SPM/STT XML versions 5 and 8 are
observed formats, not a published SDK contract. File readback proves structure
only. Use the official exporter on each changed SPM and inspect the resulting
geometry. Never accept cached Statistics, thumbnail pixels, or process exit
alone as evidence of regenerated geometry. Do not substitute UI automation.
