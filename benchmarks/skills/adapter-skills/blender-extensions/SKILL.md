---
name: blender-extensions
description: "Plan and install downloaded Blender extension or legacy add-on packages"
license: "MIT"
allowed-tools: ["Read"]
metadata:
  dcc-mcp:
    dcc: blender
    layer: infrastructure
    stage: bootstrap
    version: "0.1.0"
    tags: [blender, extension, addon, package, install]
    search-hint: "install Blender extension ZIP legacy addon package from disk"
    intent: "Validate a provider-downloaded add-on package and install it through Blender's supported extension APIs."
    preconditions:
      - type: software
        name: blender
        version: ">=4.5"
    side-effects:
      modifies: true
      imports: true
      targets: [addon_installation, user_preferences]
    produces: [blender_extension_install]
    requires: []
    tools: tools.yaml
---

# Blender Extensions

Use after a provider skill has downloaded a Blender `.zip` or legacy `.py`
add-on package. Plan first, then install into an enabled user repository.

Blender 4.5+ packages with `blender_manifest.toml` use Blender Extensions.
Legacy add-ons remain supported through Blender's compatibility operator.
Remote marketplace discovery and license acceptance stay in provider skills.
