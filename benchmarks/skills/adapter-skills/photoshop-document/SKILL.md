---
name: photoshop-document
description: Adobe Photoshop document management — open, query, list, and close documents and layers
license: MIT
allowed-tools:
- Bash
- Read
metadata:
  dcc-mcp:
    dcc: photoshop
    version: 0.2.0
    layer: domain
    tags:
    - photoshop
    - document
    - layers
    - adobe
    search-hint: open file, document info, list documents, list layers, close document, photoshop
    tools: tools.yaml
---
# photoshop-document

Adobe Photoshop document management skill. Uses the adobepy broker to communicate with Photoshop.

## Tools

- `get_document_info` — Get name, dimensions, resolution, color mode of the active document
- `list_documents` — List all open documents with metadata
- `open_document` — Open an existing local image or Photoshop document
- `list_layers` — List all layers (with optional hidden layer filter)
- `close_document` — Close a document by ID or close active document
