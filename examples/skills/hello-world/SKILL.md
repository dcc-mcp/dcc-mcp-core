---
name: hello-world
description: "A minimal example skill that prints a greeting message. Use when demonstrating the dcc-mcp-core skill system or testing a new skill installation."
license: MIT
compatibility: Python 3.7+
allowed-tools: Bash Read
tags: [example, beginner]
dcc: python
version: "1.0.0"
search-hint: "greeting, hello, example, demo, test skill"
---

# Hello World

A minimal demonstration skill for the dcc-mcp-core Skills system.

## Usage

After loading this skill with `load_skill("hello-world")`, the following tool becomes available:

- `hello_world__greet` — Print a greeting message

## Example

```python
# Via MCP tools/call
{"name": "hello_world__greet", "arguments": {"name": "Maya"}}
# → {"success": true, "message": "Hello, Maya!"}
```

## Script convention

Scripts in this skill read JSON parameters from stdin and write JSON results to stdout:

```python
import json, sys
params = json.load(sys.stdin)
name = params.get("name", "World")
print(json.dumps({"success": True, "message": f"Hello, {name}!"}))
```
