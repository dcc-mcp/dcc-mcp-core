---
name: photoshop-image
description: Adobe Photoshop image operations — create new documents, export, resize canvas, flatten, merge layers
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
    - image
    - document
    - export
    - resize
    - flatten
    - adobe
    search-hint: create document new canvas export png jpg resize flatten merge photoshop
    tools: tools.yaml
---
# photoshop-image

Image-level operations for Adobe Photoshop: create documents, export to
various formats, resize canvas/image, and merge/flatten layers.

## Tools

- `create_document` — New document with custom size, resolution, color mode
- `export_document` — Export as PNG / JPG / TIFF / PSD
- `save_document` — Save in place
- `resize_canvas` — Change canvas size without resampling
- `resize_image` — Scale the image (resamples)
- `flatten_image` — Flatten to single layer
- `merge_visible_layers` — Merge visible layers
