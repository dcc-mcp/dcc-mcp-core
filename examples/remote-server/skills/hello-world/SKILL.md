---
name: hello-world
description: >-
  Example skill — minimal greeting tool. Use only for testing remote server
  connectivity and skill discovery. Not for production use.
license: MIT
metadata:
  dcc-mcp:
    layer: example
    search-hint: "greeting, hello, test, connectivity check"
    tags: "example, demo"
    tools: "tools.yaml"
---

# Hello World

Minimal skill bundled with the remote-server example.

After loading with `load_skill("hello-world")`, the tool `hello_world__greet`
is available.
