---
name: my-grouped-skill
description: "A skill template demonstrating tool groups for progressive exposure. Basic tools are active by default; advanced tools require explicit activation via activate_tool_group."
license: MIT
tags: [example, groups]
dcc: python
version: "1.0.0"
search-hint: "groups, progressive, exposure, example"
groups:
  - name: basic
    description: Core tools available by default
    default-active: true
    tools: [basic_action]
  - name: advanced
    description: Power-user tools (activate with activate_tool_group)
    default-active: false
    tools: [advanced_action]
tools:
  - name: basic_action
    description: "A simple action available by default. Replace with your basic tool."
    group: basic
    input_schema:
      type: object
      properties:
        input:
          type: string
          description: "Input value"
          default: "default"
    read_only: true
    idempotent: true
    source_file: scripts/basic_action.py
  - name: advanced_action
    description: "An advanced action hidden until the group is activated. Replace with your power-user tool."
    group: advanced
    input_schema:
      type: object
      properties:
        input:
          type: string
          description: "Input value"
        mode:
          type: string
          description: "Processing mode"
          enum: [fast, quality, balanced]
          default: balanced
      required: [input]
    read_only: false
    destructive: false
    idempotent: false
    source_file: scripts/advanced_action.py
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
