---
name: houdini-chops
description: >-
  Pipeline skill — typed CHOP (Channel Operator) tools for motion FX,
  audio-driven animation, procedural filtering, and CHOP-to-keyframe
  export. Create and manage CHOP networks, motion clips, audio-driven
  channels, and filter effects.
license: MIT
compatibility: "dcc-mcp-houdini 0.1+, Houdini 20.5+, dcc-mcp-core 0.20.14+"
allowed-tools: Bash Read Write Edit
metadata:
  dcc-mcp:
    dcc: houdini
    layer: domain
    stage: pipeline
    version: "1.0.0"
    tags: [houdini, chops, motion-fx, audio, filter, envelope, keyframe, channel, pipeline]
    search-hint: "chop channel motion clip audio filter lag spring noise envelope keyframe export"
    tools: tools.yaml
---

# houdini-chops

Typed CHOP tools for agents. All tools are `affinity: main`.

## Tool groups

- **`network`:** `create_chop_network` — create and manage CHOP container networks.
- **`motion`:** `create_motionclip` — import or create motion clip CHOPs for
  animation data.
- **`audio`:** `create_audio_driven` — set up audio-driven animation channels
  using Audio/Envelope CHOPs.
- **`filter`:** `apply_filter` — apply procedural CHOP filters (lag, spring,
  noise, smooth).
- **`export`:** `export_to_keyframes` — bake CHOP channel data to object
  keyframes.
- **`inspect`:** `get_channel_info` — inspect CHOP node channels (names, sample
  rate, length, value range).

## Tracer-bullet flow

1. `create_chop_network(network_path="/ch/motion_fx")`
2. `create_motionclip(network_path="/ch/motion_fx", node_name="walk_cycle", clip_file="/tmp/walk.bclip")`
3. `apply_filter(network_path="/ch/motion_fx", source_node="walk_cycle", filter_type="lag", amount=0.5)`
4. `get_channel_info(node_path="/ch/motion_fx/lag1")`
5. `export_to_keyframes(node_path="/ch/motion_fx/lag1", target_path="/obj/geo1", parm_names=["tx", "ty"])`

`create_audio_driven` can point at a `.wav` file or use an existing audio node.
The envelope follower produces amplitude-driven channels that can be wired to
any object parameter.
