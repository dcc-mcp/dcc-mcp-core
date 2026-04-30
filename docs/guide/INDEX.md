# Documentation Guide Index

Quick-reference index for the `docs/guide/` directory. Use this to find the right
document without scanning every file.

## AI Agent Quick Path

**Start here if you're an AI agent** — read these documents in order:

| Priority | Document | Why |
|----------|----------|-----|
| 1 | [agents-reference.md](agents-reference.md) | **Critical** — traps, do/don't, code style, constants |
| 2 | [skills.md](skills.md) | How to write and register skills |
| 3 | [getting-started.md](getting-started.md) | Install, first server, AI agent best practices |
| 4 | [thin-harness.md](thin-harness.md) | Thin-harness layer pattern for scripting skills |
| 5 | [skill-scopes-policies.md](skill-scopes-policies.md) | SkillScope (trust levels) and SkillPolicy |

## Getting Started

| Document | Purpose |
|----------|---------|
| [getting-started.md](getting-started.md) | Install, first server, first tool |
| [what-is-dcc-mcp-core.md](what-is-dcc-mcp-core.md) | High-level project overview and motivation |
| [architecture.md](architecture.md) | Rust workspace layout, crate boundaries, PyO3 bridge |

## AI Agent & Skill Authoring

| Document | Purpose |
|----------|---------|
| [agents-reference.md](agents-reference.md) | **Critical** — traps, do/don't, code style, full example list |
| [skills.md](skills.md) | Skill system: scanning, loading, lifecycle |
| [thin-harness.md](thin-harness.md) | Thin-harness layer: `execute_python` + recipes pattern |
| [mcp-skills-integration.md](mcp-skills-integration.md) | How skills integrate with the MCP HTTP server |
| [skill-scopes-policies.md](skill-scopes-policies.md) | SkillScope (trust levels) and SkillPolicy |
| [context-bundles.md](context-bundles.md) | Project/task/asset-specific skill loading via resolved launch context |
| [rez-skill-packages.md](rez-skill-packages.md) | Rez package layout and env-var contract for distributing skills |

## MCP Server & HTTP

| Document | Purpose |
|----------|---------|
| [remote-server.md](remote-server.md) | Cloud-hosted MCP agents: auth, batch, elicitation, rich content |
| [gateway.md](gateway.md) | Multi-DCC gateway: aggregation, tool routing |
| [gateway-election.md](gateway-election.md) | `DccGatewayElection` — automatic failover |
| [tunnel-relay.md](tunnel-relay.md) | Zero-config remote MCP relay (`RelayServer` + tunnel agent) |
| [production-deployment.md](production-deployment.md) | Production checklist: logging, health probes, monitoring |
| [protocols.md](protocols.md) | MCP protocol types and versioning |

## Core Subsystems

| Document | Purpose |
|----------|---------|
| [actions.md](actions.md) | ToolRegistry, ToolDispatcher, ToolPipeline, VersionedRegistry |
| [custom-actions.md](custom-actions.md) | Adding custom tool types and validation strategies |
| [events.md](events.md) | EventBus pub/sub system |
| [naming.md](naming.md) | SEP-986 tool name and action ID validation rules |
| [transport.md](transport.md) | IPC transport: DccLinkFrame, IpcChannelAdapter, SocketServerAdapter |
| [process.md](process.md) | Process management: launch, monitor, crash recovery |
| [capture.md](capture.md) | Screen/window capture APIs |
| [sandbox.md](sandbox.md) | SandboxPolicy, InputValidator, AuditLog |
| [shm.md](shm.md) | Shared memory and zero-copy scene data |
| [usd.md](usd.md) | OpenUSD bridge: UsdStage, scene info JSON |
| [artefacts.md](artefacts.md) | FileRef + ArtefactStore — cross-tool file handoff |
| [telemetry.md](telemetry.md) | ToolMetrics, ToolRecorder, RecordingGuard |
| [scheduler.md](scheduler.md) | ScheduleSpec, TriggerSpec, cron/webhook scheduling |
| [workflows.md](workflows.md) | WorkflowSpec engine: step kinds, policies, persistence |
| [job-persistence.md](job-persistence.md) | SQLite-backed job/workflow persistence and resume |
| [prompts.md](prompts.md) | MCP Prompt definitions |
| [capabilities.md](capabilities.md) | DccCapabilities and feature detection |
| [faq.md](faq.md) | Frequently asked questions |

## Thread Safety & Concurrency

| Document | Purpose |
|----------|---------|
| [dcc-thread-safety.md](dcc-thread-safety.md) | DCC main-thread dispatch, cooperative cancellation |

## Integration

| Document | Purpose |
|----------|---------|
| [mcp-skills-integration.md](mcp-skills-integration.md) | Registering skills on an MCP HTTP server |
