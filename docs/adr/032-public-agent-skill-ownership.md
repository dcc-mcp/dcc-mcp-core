# ADR 032: Keep Public Agent Skills in dcc-mcp-agent-plugins

- Status: Accepted
- Date: 2026-09-04

## Context

The public `dcc-mcp`, `dcc-mcp-creator`, and `dcc-mcp-skills-creator` packages
were maintained in `dcc-mcp-agent-plugins` while frozen copies also lived in
this repository. The copies required a cross-repository sync workflow and made
it possible to edit or release the wrong source.

Core also owns runtime Skills that ship with the Python package. Those Skills
describe Core behavior and must remain versioned with the runtime.

## Decision

`dcc-mcp-agent-plugins` is the sole source and publisher for the three public
Agent Skills. This repository links to those packages and does not vendor
copies of them.

Core continues to own its bundled runtime Skills under
`python/dcc_mcp_core/skills`, along with Core-specific marketplace and reference
Skills. Changes that affect public guidance must be implemented in
`dcc-mcp-agent-plugins` and coordinated through linked pull requests when a
Core change is also required.

## Consequences

- Public Skill releases no longer depend on a Core-to-plugins mirror sync.
- Contributors have one editable source for each public Agent Skill.
- Core runtime Skills remain tested and released with Core.
- Reintroducing public Skill mirrors requires a new architecture decision.
