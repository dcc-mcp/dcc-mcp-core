---
name: my-domain-skill
description: >-
  Domain skill — <one-sentence what this skill does and its scope keywords>.
  Use when <trigger phrase — user intent or task keywords>.
  Not for <counter-example A> — use <infrastructure-or-other-skill> for that.
  Not for <counter-example B> — use <other-skill> for that.
license: MIT
compatibility: <DCC> <version>+, Python 3.7+
metadata:
  dcc-mcp.dcc: maya
  dcc-mcp.version: "1.0.0"
  dcc-mcp.layer: domain
  # Intent-oriented keywords — describe the user's goal, not the mechanism.
  # Do NOT duplicate keywords that belong to an infrastructure skill
  # (e.g. "usd stage", "ffmpeg", "git commit").
  dcc-mcp.search-hint: "intent keyword 1, intent keyword 2, task phrase 3"
  dcc-mcp.tags: "maya, your-category, domain"
  dcc-mcp.tools: tools.yaml
  # Declare infrastructure skills this domain skill depends on.
  # Load them before loading this skill.
  dcc-mcp.depends: "dcc-diagnostics"
  # Uncomment when tools export USD:
  # dcc-mcp.depends: "dcc-diagnostics, usd-tools"
---

# my-domain-skill

> **Layer**: Domain — depends on `dcc-diagnostics` (infrastructure).

Replace this body with documentation about your domain skill.
Keep it under 500 lines / 5000 tokens.

## When to Use This Skill

- <Trigger scenario 1 — specific user intent or task>
- <Trigger scenario 2>

## When NOT to Use This Skill

- **<Counter-example A>** → use `<other-skill>` instead
- **<Counter-example B>** → use `<other-skill>` instead

## Tools

### `my_domain_skill__primary_action`

<Describe what the tool does, its inputs, and expected output.>

### `my_domain_skill__read_only_query`

<Describe what the tool queries and the shape of its output.>

## Prerequisites

- <DCC> <version> or later
- `dcc-diagnostics` skill loaded (for failure recovery chains)

## Failure Recovery

All tools chain to `dcc_diagnostics__screenshot` and
`dcc_diagnostics__audit_log` on failure via `next-tools.on-failure`.
