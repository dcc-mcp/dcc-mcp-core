---
name: 3dsmax-camera-lighting
description: >-
  Domain skill - create, inspect, and adjust 3ds Max cameras, lights, and
  simple review lighting rigs.
license: MIT
compatibility: "dcc-mcp-core 0.17+, 3ds Max 2024+"
metadata:
  dcc-mcp:
    dcc: 3dsmax
    version: "1.0.0"
    layer: domain
    stage: authoring
    search-hint: "3ds Max camera light lighting three point rig active render camera intensity color shadows arnold corona photometric area light kelvin"
    tags: "3dsmax, camera, lighting, render-preview, arnold, corona, photometric"
    tools: tools.yaml
    intent: "Create, inspect, and configure 3ds Max cameras, lights, and review lighting rigs."
    search_aliases: ["camera_lighting", "camera-lighting"]
    recall_context:
      app_type: "3dsmax"
      domain: "camera_lighting"
      workflow_stage: "authoring"
      task_category: "mutate"
    preconditions:
      - type: software
        name: "3ds Max"
        version: ">=2024"
    side_effects:
      creates: true
      modifies: true
      deletes: false
      exports: false
      imports: false
      file_output: false
      render: false
      targets: ["scene_node", "camera", "light"]
    produces: ["scene_node:camera", "scene_node:light", "lighting_rig"]
---

# 3ds Max Camera And Lighting Tools

Create cameras and basic lights, inspect camera/light properties, set the
active render camera, build a simple three-point review light rig, and create
Arnold / Corona / photometric lights through `create_renderer_light`. Tools
validate camera and light targets before scene mutation and read every written
control back from the host.

## Light providers

`lighting_capabilities` reports which providers the active host can build, the
light shapes and photometric units each provider declares, how the active
renderer is routed, and the native property candidates for every control. Pass
`probe=true` to build one throwaway light per available factory, record which
controls the host exposes and which enum indices it accepts, and then delete it
again (the deletion is verified). Because `probe=true` creates and deletes
nodes, the tool is declared as a mutating action rather than a read-only tool.

`create_renderer_light` builds up to 32 lights in one transaction through one
provider (`auto` follows the active renderer):

| Provider | Kinds | Factories |
|---|---|---|
| `arnold` | `area`, `dome`, `photometric` | `aiAreaLight`, `aiSkyDomeLight`, `aiPhotometricLight` |
| `corona` | `area`, `sun`, `sky` | `CoronaLight`, `CoronaSun`, `CoronaSky` |
| `photometric` | `area`, `photometric`, `sun` | `mrAreaOmni`, `mrAreaSpot`, `FreeLight`, `TargetLight` |
| `standard` | `omni`, `spot`, `directional`, `skylight` | `OmniLight`, `FreeSpot`, `DirectionalLight`, `Skylight` |
| `vray` | `area`, `sun` | `VRayLight` (delegates to `create_vray_light`) |

Supported controls and the enum tables each provider declares:

| Control | Arnold | Corona | Photometric | Standard |
|---|---|---|---|---|
| `shape` | `quad` = 0, `disk` = 1, `cylinder` = 2, `sphere` = 3 | `rectangle` = 0, `disc` = 1, `sphere` = 2, `cylinder` = 3, `mesh` = 4 | `rectangle` = 0, `disc` = 1, `sphere` = 2, `cylinder` = 3 | n/a |
| `units` | `renderer` = 0, `w` = 1, `lm` = 2, `cd` = 3, `lx` = 4, `radiance` = 5 | `renderer` = 0, `w` = 1, `lm` = 2, `cd` = 3, `lx` = 4 | `renderer` = 0, `cd` = 1, `lm` = 2, `lx` = 3 | n/a |
| `intensity` / `exposure` | `intensity`, `exposure` | `intensity`, `exposure` | `intensity` | `multiplier` |
| `color_temperature` | `color_temperature`, `kelvin` | `temperature` | `kelvin` | n/a |
| `size_u` / `size_v` / `radius` | `U_size`, `V_size`, `radius` | `width`, `length`, `radius` | `mr_Width`, `mr_Length`, `mr_Radius` | `width`, `length` |
| `cast_shadows` | `castShadows` | `castShadows`, `shadowsOn` | `shadowsOn` | `castShadows` |
| `samples` / `spread` | `samples`, `spread` | `samples`, `directionality` | `mr_Samples`, `hotspot` | `samples`, `spread` |
| `texture` | `color_texmap`, `texmap`, `shader` | `texmap` | `texmap`, `projectorMap` | `texmap` |

The shape and unit indices are provider-declared defaults. A host that indexes
its dropdown differently is detected by `lighting_capabilities(probe=true)`,
which reports the accepted indices; pass `shape_value` / `units_value` to write
the native index directly and bypass the table.

`set_light_properties` shares the same provider table. It detects the provider
from the target light's native class (override with `provider`), so shape,
units, emitter size, color temperature, and texture color space can be adjusted
on an existing light with the same verification guarantees. `color_space` is
applied to the texture map already wired on the light and fails when the light
has no texture slot. When a control fails, the values written earlier in the
same call — including the texture slot and its color space — are restored and
the restore result is reported in the failure payload.

## Failure semantics for every light tool

Every requested control is read back from the host — including `name`,
`position`, and `target_position`. A control that is missing, rejected, or
silently ignored fails the whole call and rolls every light created in that
call back.

Values are validated for every spec before the first light is built, so a
malformed spec (`intensity: "bright"`, a two-channel `color`, an unknown
`shape`) is rejected with per-field detail and creates nothing. Mixed providers
in one call, an unknown `provider`, and more than 32 lights are rejected before
anything is created. If the host raises an unexpected error after a light
exists, that light is still rolled back rather than left in the scene.

## V-Ray lights

`create_vray_light` builds V-Ray lights of any supported shape in one
transaction (up to 32 lights per call):

| Control | Native property candidates | Values |
|---|---|---|
| shape | `type`, `shape` | `rectangle`/`plane` = 0, `environment`/`dome` = 1, `sphere` = 2, `mesh` = 3, `disk`/`disc` = 4 |
| units | `units` | `renderer` = 0, `lm` = 1, `cd_m2` = 2, `w` = 3, `radiance` = 4 |
| multiplier | `multiplier`, `intensity` | float |
| color | `color` | RGB 0-255 |
| shadows | `castShadows` | bool |
| normalize color | `normalizeColor` | bool |
| targeted | `targeted` | bool |
| local size | `U_size`/`V_size` (then `sizeU`/`sizeV`, `size0`/`size1`) | float |
| dome texture | `texmap` (then `dome_tex`) + `VRayBitmap` | `map_type`, `gamma`, `color_space`, `horizontal_rotation` |

See the failure semantics above: the same readback, validation, and rollback
rules apply. V-Ray only accepts its own controls, so `radius`, `exposure`,
`samples`, `spread`, `color_temperature`, and `kind` are rejected per field
before anything is created rather than being ignored. `shape_value` and
`units_value` are resolved through the declared V-Ray tables into `shape` /
`units`, so a raw index reaches the host; an index outside the table is
rejected per field instead of being dropped.
