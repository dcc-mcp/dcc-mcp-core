---
name: photoshop-filter
description: Adobe Photoshop filter operations — apply blur, sharpen, and other filters to layers
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
    - filter
    - blur
    - sharpen
    - adobe
    search-hint: gaussian blur, high pass, sharpen, smart blur, apply filter, photoshop
    tools: tools.yaml
---
# photoshop-filter

Adobe Photoshop filter operations via the adobepy broker.

## Tools

- `apply_gaussian_blur` — Apply Gaussian Blur (radius in pixels)
- `apply_high_pass` — Apply High Pass filter (radius in pixels)
- `apply_sharpen` — Apply Sharpen filter
- `apply_smart_blur` — Apply Smart Blur (radius + threshold)
- `apply_filter` — Generic filter by method name
