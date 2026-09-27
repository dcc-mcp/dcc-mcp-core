---
name: gimp-session
description: >-
  Inspect and author bounded GIMP 3 images through the authenticated DCC-MCP
  persistent plug-in bridge. Use for image/layer lifecycle, solid-color layers,
  XCF saves, deterministic exports, validation, and safe bridge-owned cleanup.
license: MIT
compatibility: "GIMP 3.0+; dcc-mcp-core 0.19.91+"
allowed-tools: "python"
metadata:
  dcc-mcp:
    dcc: gimp
    layer: domain
    version: "0.4.1"  # x-release-please-version
    search-hint: "GIMP image editor image layer XCF PNG export authoring"
    tags: "gimp,image-editing,layers,export"
    tools: tools.yaml
    depends: "dcc-diagnostics"
---

# GIMP Image Authoring

Use the agent-first lifecycle before loading this Skill. Run
`dcc-mcp-gimp install --dry-run --json` to inspect the selected GIMP executable,
target interpreter, profile, and receipt; apply it with `install --yes`, then
follow the machine-readable `next_steps[]` and require
`verify.directly_usable: true`. Use `status`, `upgrade`, and `uninstall` for the
same receipted profile. Configure `DCC_MCP_GIMP_ALLOWED_ROOTS`, and treat
exit 50 as a fail-closed restart/lock boundary rather than proof of readiness.
Use the exact `next_steps[]` command, including any explicit `--instance-id` or
`--host-pid` selectors. A legacy receipt without `entry_point_executable` is
not migrated: status reports `repair`, upgrade and uninstall refuse mutation.
All host API calls are typed and marshalled onto GIMP's GLib main thread. The
bridge
accepts only authenticated loopback JSON-lines requests and never executes
arbitrary Python, Script-Fu, PDB procedure names, or actions supplied by a
caller.

Use instance-scoped `image_id` and `layer_id` values only within the current
GIMP process. Save layered work as XCF before exporting a delivery format.
Flattening, overwriting, deleting, and closing require the explicit typed
contracts in `tools.yaml`; `close_image` refuses displays not opened by this
bridge.
