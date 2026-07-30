---
name: hello-world
description: >-
  Example skill - minimal greeting tool. Use only for testing standalone
  internal service connectivity and Skill discovery. Not for production use.
license: MIT
metadata:
  dcc-mcp:
    layer: example
    search-hint: "greeting, hello, test, connectivity check"
    tags: "example, demo"
    tools: "tools.yaml"
---

# Hello World

Minimal Skill bundled with the standalone internal-service example.

After loading with `load_skill("hello-world")`, the tool `hello_world__greet`
is available.
