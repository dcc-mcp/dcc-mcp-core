---
name: my-grouped-skill
description: "A skill template demonstrating tool groups for progressive exposure. Basic tools are active by default; advanced tools require explicit activation via activate_tool_group."
license: MIT
metadata:
  dcc-mcp.dcc: python
  dcc-mcp.version: "1.0.0"
  dcc-mcp.tags: "example, groups"
  dcc-mcp.search-hint: "groups, progressive, exposure, example"
  dcc-mcp.tools: tools.yaml
  dcc-mcp.groups: groups.yaml
---

# my-grouped-skill

Demonstrates **tool groups** for progressive exposure. The AI agent initially
sees only the `basic` group's tools. When the user needs advanced features, the
agent calls `activate_tool_group("my-grouped-skill", "advanced")` to reveal
the `advanced` tools.

## Groups

| Group | Default Active | Tools |
|-------|---------------|-------|
| `basic` | Yes | `basic_action` |
| `advanced` | No | `advanced_action` |

## Why Groups?

Groups reduce context window usage by hiding tools the AI doesn't need yet.
A skill with 20 tools can expose 5 by default and reveal the rest on demand.
