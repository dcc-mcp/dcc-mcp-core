---
name: imagemagick-tools
description: "Image processing and manipulation powered by ImageMagick — resize, composite, convert formats, add watermarks. Use when batch-processing textures, thumbnails, or rendered images in DCC pipelines."
license: MIT
compatibility: Requires ImageMagick (magick binary) on PATH
allowed-tools: Bash Read Write
metadata:
  category: image
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
tags: [image, processing, imagemagick, texture, compositing]
dcc: python
version: "1.0.0"
tools:
  - name: resize
    description: Resize an image to specified dimensions with fit mode control
    input_schema:
      type: object
      required: [input, output]
      properties:
        input:
          type: string
          description: Input image path
        output:
          type: string
          description: Output image path
        width:
          type: integer
          description: Target width in pixels
        height:
          type: integer
          description: Target height in pixels
        fit:
          type: string
          enum: [cover, contain, exact, fill]
          description: Resize mode
          default: contain
    read_only: false
    destructive: false
    idempotent: true
    source_file: scripts/resize.py

  - name: composite
    description: Composite (overlay) two images with a blend mode
    input_schema:
      type: object
      required: [base, overlay, output]
      properties:
        base:
          type: string
          description: Base image path
        overlay:
          type: string
          description: Overlay image path
        output:
          type: string
          description: Output image path
        blend:
          type: string
          enum: [over, multiply, screen, overlay, dissolve]
          description: Blend mode
          default: over
        opacity:
          type: number
          description: Overlay opacity (0.0-1.0)
          default: 1.0
    read_only: false
    destructive: false
    idempotent: true
    source_file: scripts/composite.py
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
