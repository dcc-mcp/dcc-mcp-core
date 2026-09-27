---
name: premiere-project
description: >-
  Host skill - inspect and author bounded Premiere projects, bins, media,
  sequences, edits, markers, saves, frames, and AME export jobs through the
  official UXP-backed typed facade. Not for raw JavaScript execution.
license: MIT
compatibility: "Premiere Pro 25.6+ UXP; adobepy 0.6.2+; dcc-mcp-core 0.19+"
allowed-tools: Python
metadata:
  dcc-mcp:
    dcc: premiere
    version: "0.6.0"  # x-release-please-version
    layer: domain
    stage: scene
    search-hint: "premiere project media sequence timeline marker export"
    tags: "adobe, premiere, video, sequence, timeline, ux p"
    tools: tools.yaml
---

# Premiere Project

Use this skill for bounded project and timeline work in a connected Premiere
Pro host. Inspect before editing, identify sequences and project items by the
IDs returned by the read tools, and save to a new project path before making
destructive overwrite edits in valuable projects.

## Installation and readiness

When the adapter is not connected, use the adapter-owned install SOP for
packaged installs and generated bridge builds. For an Internal deployment that
already has an approved prebuilt UXP bridge, the shared CLI can provide the
debug-link path:

1. Run `dcc-mcp-cli doctor` to inspect the local CLI and gateway.
2. Run `dcc-mcp-cli install --dcc-type premiere` with the approved bridge root
   in `--plugin-source` and the Adobe debug root in `--adobe-debug-root`.
   Internal deployments may supply them through `DCC_MCP_PLUGIN_SOURCE` and
   `DCC_MCP_ADOBE_DEBUG_ROOT`. The bridge root must contain `manifest.json`;
   this adapter repository does not claim to package that generated bridge.
3. Restart Premiere after the UXP debug link is created.
4. Run `dcc-mcp-cli list` and
   `dcc-mcp-cli wait-ready --dcc-type premiere` before loading this skill.

The CLI owns debug-link policy. This skill does not create links, copy plugin
files, or invoke UXP Developer Tool. A link on disk is not proof that Premiere
loaded the panel; readiness and a connected session must be observed through
the CLI and adapter status. The adapter-owned installer remains the canonical
path for acquiring and generating a bridge.

## Recommended flow

1. Call `get_status`, then `inspect_project`.
2. Use `list_project_items` and `list_sequences` to resolve stable IDs.
3. Use `create_bin`, `import_media`, or `create_sequence` as needed.
4. Use `insert_project_item` for a ripple edit or
   `overwrite_project_item` only when replacing timeline content is intended.
5. Validate with `inspect_sequence` and `list_selected_clips`.
6. Save with `save_project_as` for a new verified `.prproj`, or
   `save_project` when updating the active project is intentional.
7. Use `export_frame` for a verified still. `queue_sequence_export` reports a
   queued AME job; it does not claim the encoded media is complete.

## Path policy

Media imports must be inside `DCC_MCP_PREMIERE_ALLOWED_INPUT_ROOTS`. Project,
frame, and media exports must be inside
`DCC_MCP_PREMIERE_ALLOWED_OUTPUT_ROOTS`. Sequence and AME presets must be
inside `DCC_MCP_PREMIERE_ALLOWED_PRESET_ROOTS`. Each variable uses the platform
path separator and defaults to the current user's home directory.

Existing outputs require `overwrite=true`; missing output parents require
`create_parents=true`. The public tools never expose raw JavaScript, `evalJs`,
shell commands, or arbitrary UXP calls.
