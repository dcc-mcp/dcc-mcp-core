---
name: my-skill
description: "A minimal skill template. Replace this description with what your skill does — this text is shown to AI agents during discovery."
license: MIT
tags: [example]
dcc: python
version: "1.0.0"
search-hint: "keyword1, keyword2, keyword3"
tools:
  - name: hello
    description: "Greet the user by name. Replace with your tool's description."
    input_schema:
      type: object
      properties:
        name:
          type: string
          description: "Name to greet"
          default: "World"
    read_only: true
    idempotent: true
    source_file: scripts/hello.py
---

# my-skill

Replace this body with documentation about your skill.
This text is available via `get_skill_info` but not shown in `tools/list`.
