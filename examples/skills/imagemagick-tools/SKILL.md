---
name: imagemagick-tools
description: >-
  Infrastructure skill — image processing and manipulation via ImageMagick:
  resize, composite, convert formats, add watermarks. Use when batch-processing
  textures, thumbnails, or rendered images at the file level. Not for
  in-DCC texture or material editing — use a domain skill bound to the specific
  DCC for that.
license: MIT
compatibility: Requires ImageMagick (magick binary) on PATH
allowed-tools: Bash Read Write
metadata:
  dcc-mcp.dcc: python
  dcc-mcp.version: "1.0.0"
  dcc-mcp.layer: infrastructure
  dcc-mcp.search-hint: "imagemagick, resize image, convert texture, composite, watermark, batch image, thumbnail"
  dcc-mcp.tags: "image, processing, imagemagick, texture, compositing, infrastructure"
  dcc-mcp.tools: tools.yaml
  openclaw:
    requires:
      bins:
        - magick
    install:
      - kind: brew
        formula: imagemagick
        bins: [magick]
    emoji: "🖼️"
    homepage: https://imagemagick.org
---

# ImageMagick Tools

Batch image processing for DCC pipelines using ImageMagick.

## Tools

### `imagemagick_tools__resize`
Resize textures for LOD levels, thumbnails, or export targets.

```bash
# Example invocation via MCP
{"name": "imagemagick_tools__resize",
 "arguments": {"input": "tex_4k.exr", "output": "tex_1k.png",
                "width": 1024, "height": 1024, "fit": "cover"}}
```

### `imagemagick_tools__composite`
Overlay images with blend mode control — watermarks, decals, previews.

## Prerequisites

Install ImageMagick:
- **macOS**: `brew install imagemagick`
- **Linux**: `apt install imagemagick`
- **Windows**: Download from https://imagemagick.org/script/download.php

Verify: `magick -version`

## Common DCC use cases

- Batch-resize texture maps for LOD levels
- Generate contact sheets from rendered frames
- Convert between texture formats (EXR → PNG, TIFF → JPEG)
- Add watermarks or version stamps to preview renders
