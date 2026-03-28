---
name: imagemagick-tools
description: "Image processing and manipulation powered by ImageMagick"
tools: ["Bash", "Read", "Write"]
tags: ["image", "processing", "imagemagick", "texture", "compositing"]
dcc: python
version: "1.0.0"
metadata:
  openclaw:
    requires:
      bins:
        - magick
    install:
      - kind: brew
        formula: imagemagick
        bins: [magick]
---

# ImageMagick Tools Skill

Wraps [ImageMagick](https://imagemagick.org/) — the swiss army knife of image
processing — as MCP-discoverable tools. Particularly useful for DCC pipelines
where texture batch processing, thumbnail generation, and format conversion
are common tasks.

## Scripts

- **resize.py** — Resize images with various fit modes (cover, contain, exact)
- **composite.py** — Composite/overlay images with blend modes

## Use Cases in DCC Pipelines

- Batch-resize texture maps for LOD levels
- Generate contact sheets from rendered frames
- Convert between texture formats (EXR → PNG, TIFF → JPEG)
- Add watermarks to preview renders
