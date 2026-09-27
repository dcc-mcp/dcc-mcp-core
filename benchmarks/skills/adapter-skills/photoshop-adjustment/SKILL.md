---
name: photoshop-adjustment
description: Adobe Photoshop adjustment and batch operations — channels and batchPlay
license: MIT
allowed-tools:
- Bash
- Read
metadata:
  dcc-mcp:
    dcc: photoshop
    version: 0.1.0
    layer: domain
    tags:
    - photoshop
    - adjustment
    - channel
    - batchplay
    - adobe
    search-hint: batch play, channels, add channel, remove channel, action descriptor, photoshop
    tools: tools.yaml
---
# photoshop-adjustment

Adobe Photoshop adjustment and batch operations via the adobepy broker.

## Tools

- `batch_play` — Execute raw ActionDescriptor batchPlay commands
- `get_channels` — List document channels (active and component)
- `add_channel` — Create a new alpha channel
- `remove_channel` — Delete a named channel
