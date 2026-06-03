# Core Escalation Checklist

Use this before adding adapter-local framework code. If the answer is "yes" for
two or more adapters, prefer a core issue/RFC.

## Escalate to Core

Open a core issue when the adapter needs:

- a lifecycle hook around skill discovery, skill load, unload, group activation,
  resource subscription, client initialize, or tool dispatch;
- a typed skill object transform that must apply to programmatic, MCP, REST, and
  gateway load paths;
- a public `DccServerBase` wrapper over a private inner server API;
- a reusable resource/prompt/project registration pattern;
- a readiness bit or health check shared by host dispatchers;
- a gateway search/describe/call response field;
- install, uninstall, or sidecar lifecycle behavior;
- cross-DCC app UI automation contracts;
- common artefact/file handoff and retention behavior;
- policy, audit, telemetry, or debug bundle fields.

## Keep Local to the Adapter

Keep code adapter-local when it is only:

- the host's import path, version query, or startup hook;
- the exact host API call, such as `bpy.ops`, `pymxs.runtime`, or Unreal editor APIs;
- a DCC-specific menu, shelf, plugin, or bootstrap script;
- domain tool behavior that belongs to one DCC skill package;
- a studio-specific deployment policy.

## Current Core Requests From Adapter Review

- [RFC: add adapter skill-load transform hooks](https://github.com/dcc-mcp/dcc-mcp-core/issues/1204): adapters need a core-owned hook so metadata transforms apply consistently to programmatic `load_skill`, MCP `load_skill`, REST `/v1/load_skill`, and gateway-mediated loads.
- [RFC: expose public DccServerBase resource registration surface](https://github.com/dcc-mcp/dcc-mcp-core/issues/1205): adapters need a public resource handle/helper instead of private inner-server access.
- [RFC: add reusable adapter readiness binder](https://github.com/dcc-mcp/dcc-mcp-core/issues/1206): embedded adapters need a shared readiness binder for process, dispatcher, host-execution, main-thread, and DCC-ready state.

## Issue Template

```markdown
## Problem

Describe the adapter-local code that should not be repeated in every DCC repo.

## Requested Core Surface

- API name or shape.
- Which load/call/resource/readiness paths it must cover.
- Expected error and observability behavior.

## Acceptance Criteria

- At least one Python adapter test.
- At least one MCP or REST path test when the behavior crosses HTTP.
- Backward compatibility notes for existing adapters.
```

Public issue text must be portable. Do not include local paths, private
hostnames, machine-specific logs, or source-attribution markers.
