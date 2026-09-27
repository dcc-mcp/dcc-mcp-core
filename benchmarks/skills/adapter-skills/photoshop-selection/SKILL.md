---
name: photoshop-selection
description: Adobe Photoshop selection operations — get, create, modify, and save selections
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
    - selection
    - marquee
    - adobe
    search-hint: select all, deselect, inverse, rectangle, ellipse, expand, contract, feather, save selection, photoshop
    tools: tools.yaml
---
# photoshop-selection

Adobe Photoshop selection operations via the adobepy broker.

## Tools

- `get_selection` — Get current selection bounds
- `select_all` — Select entire canvas
- `deselect` — Clear active selection
- `select_rectangle` — Create rectangular marquee (top/left/bottom/right)
- `select_ellipse` — Create elliptical marquee (top/left/bottom/right)
- `inverse_selection` — Invert current selection
- `expand_selection` — Expand selection by N pixels
- `contract_selection` — Contract selection by N pixels
- `feather_selection` — Feather selection by N pixels
- `save_selection` — Save selection as alpha channel
