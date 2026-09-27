---
name: speedtree-official
description: Route SpeedTree work through documented Modeler capabilities and verified asset exports.
metadata:
  dcc-mcp:
    dcc: speedtree
    layer: domain
    version: "0.1.0"
    search-hint: "SpeedTree official workflow capabilities coverage limitations batch export"
    depends: [speedtree-discovery, speedtree-export]
---

# Official SpeedTree capabilities

Use speedtree-discovery for installed content and preset inspection, and
speedtree-export for the official standalone CLI. CLI exports do not require
an attached GUI. They preserve input identity and produce an asset manifest
for target adapter import/readback. Live in-process generator edits and queries
remain unavailable until a supported vendor integration is implemented.

For experimental SPM/STT generator file authoring, use speedtree-nodes and
validate the resulting new file through the official exporter. File edits
do not modify a live Modeler document or establish a published scripting API.

Before UI observation or input, report provider=dcc-cua, runtime version,
exact PID and HWND. Use only the project ui-control route with fresh binding.
Do not fall back to generic computer use or undocumented process injection.
Licensing and plugin changes require the corresponding operator workflow.
