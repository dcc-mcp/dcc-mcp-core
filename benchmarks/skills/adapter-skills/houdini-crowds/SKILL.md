---
name: houdini-crowds
description: >-
  Typed crowd tools for Houdini. Create a crowd DOP network, attach steering and
  avoidance behaviors, and inspect the crowd graph without simulating it.
license: MIT
compatibility: "dcc-mcp-houdini 0.41+, Houdini 19.5+, dcc-mcp-core 0.20.14+"
allowed-tools: Bash Read Write Edit
metadata:
  dcc-mcp:
    dcc: houdini
    layer: domain
    stage: pipeline
    version: "1.0.0"
    tags: [houdini, crowd, agents, dop, steering, simulation, validation]
    search-hint: "crowd agents simulate steer avoid seek follow trigger dop network inspect"
    search-aliases:
      - create crowd network
      - add crowd behavior
      - inspect crowd
      - agent simulation
    tools: tools.yaml
---

# houdini-crowds

Typed crowd authoring for Houdini's native agent simulation. Before this package
the adapter had no crowd coverage at all, so agent work had to go through
`execute_python` or generic node creation.

## Tracer-bullet flow

1. `create_crowd_network(parent_path="/obj")`
2. `add_crowd_behavior(network_path="/obj/crowdsim1", behavior_type="steer")`
3. `add_crowd_behavior(network_path="/obj/crowdsim1", behavior_type="avoid")`
4. `inspect_crowd(network_path="/obj/crowdsim1")`

## Behaviors

| `behavior_type` | SOP/DOP |
|---|---|
| `steer` | `crowd_steer` |
| `avoid` | `crowd_avoid` |
| `seek` | `crowd_seek` |
| `follow` | `crowd_follow` |
| `trigger` | `crowd_trigger` |
| `geometry` | `crowd_geometry` |
| `pop` | `crowd_pop` |

`add_crowd_behavior` attaches the node to the next free input of the network's
`crowdsolver`; pass `connect_to` to target a different node. With no solver the
node is created unattached and reported with `setup_state="unattached"`.

## Honesty contract

- `create_crowd_network` returns `setup_state="skeleton"` and never claims
  `simulation_verified=true`. Building the network does not run or verify a crowd
  simulation: an agent definition, agent source geometry and a cook are still
  required, and none of them are performed here.
- `inspect_crowd` reports structure only (`validation_scope="graph_structure"`).
  It answers "is the graph wired up" — whether a solver, a crowd object and
  behaviors are present — and **not** whether the crowd simulates. A clean
  inspection is not evidence that agents move, and no tool here cooks, steps or
  validates a crowd simulation.
- This package has no `skipped_parameters` field: parameter overrides go through
  `parameter_edit`, so an override the node does not expose **fails the call and
  rolls back** rather than being skipped. See the naming rule in the bake and
  light-rig skills.
- Creation failures destroy only the nodes owned by the failed request.
