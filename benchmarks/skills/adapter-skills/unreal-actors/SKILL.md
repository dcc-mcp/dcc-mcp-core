---
name: unreal-actors
description: >-
  Domain skill - Unreal Engine actor management: list, spawn, transform, and
  delete level actors. Use when inspecting or editing actors in the current
  Unreal Editor level. Not for Content Browser asset import/export - use
  unreal-assets for that.
license: MIT
compatibility: Unreal Engine 5.0+, Python 3.9+
allowed-tools: Bash Read
metadata:
  dcc-mcp:
    dcc: unreal
    version: "0.1.0"
    layer: domain
    stage: authoring
    search-hint: "unreal actor list spawn delete transform level editor static mesh blueprint"
    tags: "unreal, actors, level, gameplay, transform"
    tools: tools.yaml
---

# Unreal Actors

Tools for actor-level operations in the active Unreal Editor level.

## Scripts

- `list_actors` - List all actors in the current level.
- `spawn_actor` - Spawn an actor of a given class at a world position.
- `delete_actor` - Delete an actor from the level by name.
- `get_actor_transform` - Get the world-space location, rotation, and scale of an actor.
- `set_actor_transform` - Set the world-space location, rotation, and/or scale of an actor.

Actor lookup targets the active PIE game world while play is running and
falls back to the editor level otherwise. `list_actors`, transform tools, and
project-owned playtest skills therefore use the same exact actor names.
- `configure_sky_light` - Assign an imported HDR TextureCube to a level Sky Light.
- `configure_light` - Set allowlisted intensity, color, temperature, and softness controls on a light.
- `configure_camera_exposure` - Disable eye adaptation and lock a camera to a fixed manual exposure.

## Usage Examples

### List all Static Mesh actors

```python
# MCP tool call: unreal_actors__list_actors
# params: {"actor_class_filter": "StaticMeshActor"}
```

### Spawn a cube at the origin

```python
# MCP tool call: unreal_actors__spawn_actor
# params: {
#   "actor_class": "/Script/Engine.StaticMeshActor",
#   "location_x": 0.0, "location_y": 0.0, "location_z": 0.0,
#   "label": "MyCube"
# }
```

### Delete an actor

```python
# MCP tool call: unreal_actors__delete_actor
# params: {"actor_name": "SM_Cube_1"}
```

### Get actor transform

```python
# MCP tool call: unreal_actors__get_actor_transform
# params: {"actor_name": "SM_Cube_1"}
```

### Move an actor to a new position

```python
# MCP tool call: unreal_actors__set_actor_transform
# params: {"actor_name": "SM_Cube_1", "location_x": 100.0, "location_y": 200.0}
```

### Configure HDR image-based lighting

```python
# MCP tool call: unreal_actors__configure_sky_light
# params: {
#   "actor_name": "Lookdev_SkyLight",
#   "cubemap_path": "/Game/Lookdev/T_Studio_HDRI",
#   "intensity_scale": 1.0
# }
```

### Lock a camera exposure

```python
# MCP tool call: unreal_actors__configure_camera_exposure
# params: {"actor_name": "Lookdev_Camera", "exposure_compensation": -2.0}
```
