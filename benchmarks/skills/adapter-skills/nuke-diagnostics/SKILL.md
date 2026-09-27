---
name: nuke-diagnostics
description: >-
  Host readiness diagnostics for a live Nuke adapter. Use for a bounded,
  read-only main-thread ping, host flavor detection, and capability
  reporting across Nuke, NukeX, and Nuke Studio. Not for script execution or
  graph mutation.
license: MIT
compatibility: "Nuke 14+; Python 3.9+; dcc-mcp-core 0.20.8+"
allowed-tools: Python
metadata:
  dcc-mcp:
    dcc: nuke
    version: "0.0.0"
    layer: infrastructure
    search-hint: "nuke diagnostics ping readiness main thread host version flavor nukex studio"
    tags: "nuke, diagnostics, read-only, host-flavor"
    tools: tools.yaml
---

# Nuke Diagnostics

Use `nuke_diagnostics__ping` to prove that the selected live adapter can enter
Nuke's main thread and read its host version. It does not mutate the current
script or expose a scripting fallback.

Use `nuke_diagnostics__host_flavor` to report which Nuke entry point is
running: `nuke`, `nukex`, or `nukestudio`. The result carries the capability
set that entry point provides, so an agent can decide whether Studio-only
skills such as `nuke-studio-timeline` are available before loading them. The
three entry points share one package, one `dcc_type` (`nuke`), and one
`~/.nuke` plug-in profile; only the advertised capability surface differs.

Both probes are read-only. Neither mutates the script nor starts a server.
