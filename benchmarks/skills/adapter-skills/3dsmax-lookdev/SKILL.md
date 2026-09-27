---
name: 3dsmax-lookdev
description: >-
  Domain skill - configure OCIO color management, HDR environment lighting, preview materials, and
  assign renderer-specific materials (Arnold, V-Ray, Scanline) in 3ds Max.
license: MIT
compatibility: "dcc-mcp-core 0.17+, 3ds Max 2024+"
metadata:
  dcc-mcp:
    dcc: 3dsmax
    version: "1.0.0"
    layer: domain
    stage: authoring
    search-hint: "3ds Max lookdev HDR environment lighting material preview arnold v-ray scanline"
    tags: "3dsmax, lookdev, hdr, environment, lighting, material, arnold, vray"
    tools: tools.yaml
    intent: "Configure HDR environment lighting, preview materials, and assign renderer-specific materials."
    search_aliases: ["lookdev", "look development", "look dev"]
    recall_context:
      app_type: "3dsmax"
      domain: "lookdev"
      workflow_stage: "authoring"
      task_category: "mutate"
    preconditions:
      - type: software
        name: "3ds Max"
        version: ">=2024"
    side_effects:
      creates: false
      modifies: true
      deletes: false
      exports: false
      imports: false
      file_output: false
      render: false
      targets: ["scene_node", "environment", "material"]
    produces: ["environment_lighting", "material_assignment"]
---

# 3ds Max Look Development Skill

Configure environment lighting from HDR/EXR images, set up a three-point light
rig, preview materials on test geometry, and assign materials with renderer
detection (Arnold, V-Ray, Scanline).

## Tools

- **`get_color_management` / `set_color_management`** — Inspect or configure the
  3ds Max 2024+ scene OCIO config and scene-linear rendering color space. The
  setter can also select a native frame-buffer display/view pair (for example,
  `sRGB` / `un-tone-mapped`), validates it against the loaded config, and
  requires an exact host readback. Failed configuration is rolled back.
- **`setup_hdr_lighting`** — Load an HDR/EXR image as the environment map,
  configure rotation and intensity, and create a renderer-compatible
  three-point light rig. Arnold scenes use native Arnold lights with host
  property readback; an unavailable or incompatible light capability fails
  closed before the environment is changed. On a V-Ray scene the HDRI is built
  as a `VRayBitmap` with `map_type` (angular / cubic / spherical /
  mirrored_ball / max_standard), `gamma`, `color_space`, and
  `horizontal_rotation` control; a projection control the bitmap refuses fails
  the call before anything in the scene is changed.
- **`set_hdri_rotation`** — Rotate the active environment for a lighting
  turntable without rebuilding the rig.
- **`preview_material`** — Create a test sphere or quad, apply a named scene
  material, and report the viewport-visible result.
- **`assign_renderer_material`** — Detect the active renderer
  (Arnold/V-Ray/Scanline), create or find a suitable material class, apply it
  to named nodes, and return a summary.

## V-Ray roughness rule

VRayMtl roughness is not a `roughness` property. The tool sets
`brdf_useRoughness = true` first and then writes `reflectionRoughness`; on
V-Ray 4 hosts that only expose `reflection_glossiness` it writes the inverted
value instead. Both writes are verified by readback, and a VRayMtl that accepts
neither spelling fails the call and rolls the new material back instead of
returning a success the agent cannot trust.
