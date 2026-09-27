---
name: marmoset-lookdev
description: >-
  Domain skill - inspect Toolbag PBR materials and frame either the whole scene
  or an exact object for lookdev and render preparation. Use before material
  validation or camera rendering. Not for arbitrary Python execution.
license: MIT
compatibility: "Marmoset Toolbag 4.03+ or 5.x; Python 3.9+; dcc-mcp-core 0.20.14+"
allowed-tools: Python
metadata:
  dcc-mcp:
    dcc: marmoset
    layer: domain
    version: "0.1.2"  # x-release-please-version
    stage: lookdev
    search-hint: "Marmoset Toolbag lookdev frame object frame scene inspect PBR materials texture maps camera TA"
    tags: "marmoset,toolbag,lookdev,materials,camera,technical-art"
    tools: tools.yaml
---

# Marmoset Toolbag Lookdev

Inspect material bindings first. Frame by exact object UID when preparing a
hero asset; omit the UID only when the full scene should determine the camera.

`configure_color_output` can select Toolbag's ACES tone mapper for visual
comparison. Toolbag does not expose an OCIO/ACEScg transform API, so this is
an output-boundary check rather than an OCIO-managed production workflow.
